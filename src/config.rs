// Copyright 2026 Marco Porcellato
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::Path;

use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};

use crate::receipt::{ReceiptError, canonical_digest, canonical_json};

pub const CONFIG_SCHEMA_VERSION: &str = "1.0";
pub const MAX_CONFIG_BYTES: usize = 1_048_576;
pub const MAX_CHECKS: usize = 128;
pub const MAX_CACHES: usize = 32;
pub const MAX_ARGV_PARTS: usize = 64;
pub const MAX_STRING_BYTES: usize = 4096;
pub const MAX_TIMEOUT_SECONDS: u64 = 86_400;
pub const MAX_MEMORY_MIB: u64 = 262_144;
pub const MAX_CPU_COUNT: u16 = 256;
pub const MAX_PIDS: u32 = 65_536;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigV1 {
    pub schema_version: String,
    pub project: String,
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub receipt: ReceiptConfig,
    #[serde(default)]
    pub environment: EnvironmentConfig,
    #[serde(default)]
    pub caches: Vec<CacheConfig>,
    pub checks: Vec<CheckConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    pub kind: RuntimeKind,
    pub image: String,
    pub cpu_count: u16,
    pub memory_mib: u64,
    pub pids_limit: u32,
    #[serde(default)]
    pub network: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    DockerCompatible,
    Host,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ReceiptConfig {
    pub output: String,
    pub freshness_seconds: u64,
}

impl Default for ReceiptConfig {
    fn default() -> Self {
        Self {
            output: ".ccp/receipt.json".to_owned(),
            freshness_seconds: 86_400,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct EnvironmentConfig {
    pub allow: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CacheConfig {
    pub id: String,
    pub mount_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckConfig {
    pub id: String,
    pub required: bool,
    pub argv: Vec<String>,
    pub working_directory: String,
    pub timeout_seconds: u64,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionPlanEnvelopeV1 {
    pub plan_digest: String,
    pub plan: ExecutionPlanV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionPlanV1 {
    pub schema_version: String,
    pub project: String,
    pub runtime: NormalizedRuntime,
    pub receipt: NormalizedReceipt,
    pub environment_allow: Vec<String>,
    pub caches: Vec<NormalizedCache>,
    pub checks: Vec<NormalizedCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NormalizedRuntime {
    pub kind: RuntimeKind,
    pub image: String,
    pub cpu_count: u16,
    pub memory_mib: u64,
    pub pids_limit: u32,
    pub network: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NormalizedReceipt {
    pub output: String,
    pub freshness_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NormalizedCache {
    pub id: String,
    pub mount_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NormalizedCheck {
    pub id: String,
    pub required: bool,
    pub argv: Vec<String>,
    pub working_directory: String,
    pub timeout_seconds: u64,
    pub depends_on: Vec<String>,
    pub artifacts: Vec<String>,
}

impl ConfigV1 {
    pub fn parse(input: &str) -> Result<Self, ConfigError> {
        if input.len() > MAX_CONFIG_BYTES {
            return Err(ConfigError::ConfigTooLarge {
                actual: input.len(),
                maximum: MAX_CONFIG_BYTES,
            });
        }
        toml::from_str(input).map_err(ConfigError::Parse)
    }

    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let metadata = fs::metadata(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let size = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
        if size > MAX_CONFIG_BYTES {
            return Err(ConfigError::ConfigTooLarge {
                actual: size,
                maximum: MAX_CONFIG_BYTES,
            });
        }
        let input = fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&input)
    }

    pub fn into_plan(self) -> Result<ExecutionPlanEnvelopeV1, ConfigError> {
        self.validate_top_level()?;
        let environment_allow = unique_sorted(
            "environment.allow",
            self.environment.allow,
            validate_environment_name,
        )?;
        let caches = normalize_caches(self.caches)?;
        let checks = normalize_checks(self.checks)?;
        let receipt = NormalizedReceipt {
            output: self.receipt.output,
            freshness_seconds: self.receipt.freshness_seconds,
        };
        validate_path_isolation(&receipt, &caches, &checks)?;
        let plan = ExecutionPlanV1 {
            schema_version: self.schema_version,
            project: self.project,
            runtime: NormalizedRuntime {
                kind: self.runtime.kind,
                image: self.runtime.image,
                cpu_count: self.runtime.cpu_count,
                memory_mib: self.runtime.memory_mib,
                pids_limit: self.runtime.pids_limit,
                network: self.runtime.network,
            },
            receipt,
            environment_allow,
            caches,
            checks,
        };
        let plan_digest = canonical_digest(&plan).map_err(ConfigError::Receipt)?;
        Ok(ExecutionPlanEnvelopeV1 { plan_digest, plan })
    }

    fn validate_top_level(&self) -> Result<(), ConfigError> {
        if self.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchemaVersion(
                self.schema_version.clone(),
            ));
        }
        validate_repository_identity(&self.project)?;
        validate_image_reference(&self.runtime.image)?;
        validate_bounded(
            "runtime.cpu_count",
            u64::from(self.runtime.cpu_count),
            1,
            u64::from(MAX_CPU_COUNT),
        )?;
        validate_bounded(
            "runtime.memory_mib",
            self.runtime.memory_mib,
            64,
            MAX_MEMORY_MIB,
        )?;
        validate_bounded(
            "runtime.pids_limit",
            u64::from(self.runtime.pids_limit),
            1,
            u64::from(MAX_PIDS),
        )?;
        validate_relative_path("receipt.output", &self.receipt.output)?;
        if self.receipt.output == "." {
            return Err(ConfigError::InvalidField("receipt.output"));
        }
        validate_bounded(
            "receipt.freshness_seconds",
            self.receipt.freshness_seconds,
            1,
            31_536_000,
        )
    }
}

impl ExecutionPlanEnvelopeV1 {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ConfigError> {
        let expected = canonical_digest(&self.plan).map_err(ConfigError::Receipt)?;
        if self.plan_digest != expected {
            return Err(ConfigError::PlanDigestMismatch);
        }
        canonical_json(self).map_err(ConfigError::Receipt)
    }
}

pub fn config_schema_json() -> Result<String, ConfigError> {
    let schema = schema_for!(ConfigV1);
    serde_json::to_string_pretty(&schema)
        .map_err(ReceiptError::Serialization)
        .map_err(ConfigError::Receipt)
}

fn normalize_caches(caches: Vec<CacheConfig>) -> Result<Vec<NormalizedCache>, ConfigError> {
    if caches.len() > MAX_CACHES {
        return Err(ConfigError::TooManyItems {
            field: "caches",
            actual: caches.len(),
            maximum: MAX_CACHES,
        });
    }
    let mut by_id = BTreeMap::new();
    let mut mount_paths = BTreeSet::new();
    for cache in caches {
        validate_identifier("cache.id", &cache.id)?;
        validate_relative_path("cache.mount_path", &cache.mount_path)?;
        if cache.mount_path == "." {
            return Err(ConfigError::InvalidField("cache.mount_path"));
        }
        if by_id.contains_key(&cache.id) {
            return Err(ConfigError::DuplicateId {
                field: "cache.id",
                id: cache.id,
            });
        }
        if !mount_paths.insert(cache.mount_path.clone()) {
            return Err(ConfigError::DuplicateValue("cache.mount_path"));
        }
        by_id.insert(
            cache.id.clone(),
            NormalizedCache {
                id: cache.id,
                mount_path: cache.mount_path,
            },
        );
    }
    Ok(by_id.into_values().collect())
}

fn normalize_checks(checks: Vec<CheckConfig>) -> Result<Vec<NormalizedCheck>, ConfigError> {
    if checks.is_empty() {
        return Err(ConfigError::NoChecks);
    }
    if checks.len() > MAX_CHECKS {
        return Err(ConfigError::TooManyItems {
            field: "checks",
            actual: checks.len(),
            maximum: MAX_CHECKS,
        });
    }
    if !checks.iter().any(|check| check.required) {
        return Err(ConfigError::NoRequiredChecks);
    }

    let mut by_id = BTreeMap::new();
    for mut check in checks {
        validate_check(&check)?;
        check.depends_on = unique_sorted("check.depends_on", check.depends_on, |value| {
            validate_identifier("check.depends_on", value)
        })?;
        check.artifacts = unique_sorted("check.artifacts", check.artifacts, |value| {
            validate_relative_path("check.artifacts", value)
        })?;
        if check.artifacts.iter().any(|artifact| artifact == ".") {
            return Err(ConfigError::InvalidField("check.artifacts"));
        }
        let id = check.id.clone();
        if by_id.insert(id.clone(), check).is_some() {
            return Err(ConfigError::DuplicateId {
                field: "check.id",
                id,
            });
        }
    }

    validate_dependencies(&by_id)?;
    topological_checks(&by_id)
}

fn validate_path_isolation(
    receipt: &NormalizedReceipt,
    caches: &[NormalizedCache],
    checks: &[NormalizedCheck],
) -> Result<(), ConfigError> {
    for (index, cache) in caches.iter().enumerate() {
        for other in &caches[index + 1..] {
            reject_path_overlap(&cache.mount_path, &other.mount_path)?;
        }
        reject_path_overlap(&cache.mount_path, &receipt.output)?;
    }

    let mut artifacts = BTreeSet::new();
    for check in checks {
        for artifact in &check.artifacts {
            if !artifacts.insert(artifact.as_str()) {
                return Err(ConfigError::DuplicateArtifact(artifact.clone()));
            }
            reject_path_overlap(artifact, &receipt.output)?;
            for cache in caches {
                reject_path_overlap(artifact, &cache.mount_path)?;
            }
        }
    }
    Ok(())
}

fn reject_path_overlap(first: &str, second: &str) -> Result<(), ConfigError> {
    let overlap = first == second
        || first
            .strip_prefix(second)
            .is_some_and(|suffix| suffix.starts_with('/'))
        || second
            .strip_prefix(first)
            .is_some_and(|suffix| suffix.starts_with('/'));
    if overlap {
        Err(ConfigError::PathOverlap {
            first: first.to_owned(),
            second: second.to_owned(),
        })
    } else {
        Ok(())
    }
}

fn validate_dependencies(by_id: &BTreeMap<String, CheckConfig>) -> Result<(), ConfigError> {
    for check in by_id.values() {
        for dependency in &check.depends_on {
            if dependency == &check.id {
                return Err(ConfigError::SelfDependency(check.id.clone()));
            }
            if !by_id.contains_key(dependency) {
                return Err(ConfigError::UnknownDependency {
                    check: check.id.clone(),
                    dependency: dependency.clone(),
                });
            }
        }
    }
    Ok(())
}

fn topological_checks(
    by_id: &BTreeMap<String, CheckConfig>,
) -> Result<Vec<NormalizedCheck>, ConfigError> {
    let mut indegree: BTreeMap<String, usize> = by_id
        .iter()
        .map(|(id, check)| (id.clone(), check.depends_on.len()))
        .collect();
    let mut dependents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (id, check) in by_id {
        for dependency in &check.depends_on {
            dependents
                .entry(dependency.clone())
                .or_default()
                .insert(id.clone());
        }
    }
    let mut ready: BTreeSet<String> = indegree
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| id.clone())
        .collect();
    let mut ordered = Vec::with_capacity(by_id.len());
    while let Some(id) = ready.pop_first() {
        let check = by_id.get(&id).expect("ready check exists");
        ordered.push(normalize_check(check));
        if let Some(children) = dependents.get(&id) {
            for child in children {
                let count = indegree.get_mut(child).expect("dependent check exists");
                *count -= 1;
                if *count == 0 {
                    ready.insert(child.clone());
                }
            }
        }
    }
    if ordered.len() != by_id.len() {
        let cycle = indegree
            .into_iter()
            .filter(|(_, count)| *count > 0)
            .map(|(id, _)| id)
            .collect();
        return Err(ConfigError::DependencyCycle(cycle));
    }
    Ok(ordered)
}

fn validate_check(check: &CheckConfig) -> Result<(), ConfigError> {
    validate_identifier("check.id", &check.id)?;
    if check.argv.is_empty() || check.argv.len() > MAX_ARGV_PARTS {
        return Err(ConfigError::InvalidField("check.argv"));
    }
    for argument in &check.argv {
        validate_text("check.argv", argument)?;
    }
    validate_relative_path("check.working_directory", &check.working_directory)?;
    validate_bounded(
        "check.timeout_seconds",
        check.timeout_seconds,
        1,
        MAX_TIMEOUT_SECONDS,
    )
}

fn normalize_check(check: &CheckConfig) -> NormalizedCheck {
    NormalizedCheck {
        id: check.id.clone(),
        required: check.required,
        argv: check.argv.clone(),
        working_directory: check.working_directory.clone(),
        timeout_seconds: check.timeout_seconds,
        depends_on: check.depends_on.clone(),
        artifacts: check.artifacts.clone(),
    }
}

fn unique_sorted<F>(
    field: &'static str,
    values: Vec<String>,
    validate: F,
) -> Result<Vec<String>, ConfigError>
where
    F: Fn(&str) -> Result<(), ConfigError>,
{
    let mut unique = BTreeSet::new();
    for value in values {
        validate(&value)?;
        if !unique.insert(value) {
            return Err(ConfigError::DuplicateValue(field));
        }
    }
    Ok(unique.into_iter().collect())
}

fn validate_text(field: &'static str, value: &str) -> Result<(), ConfigError> {
    if value.is_empty() || value.len() > MAX_STRING_BYTES || value.chars().any(char::is_control) {
        Err(ConfigError::InvalidField(field))
    } else {
        Ok(())
    }
}

pub(crate) fn validate_identifier(field: &'static str, value: &str) -> Result<(), ConfigError> {
    validate_text(field, value)?;
    if value.len() <= 64
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
        && value != "."
        && value != ".."
    {
        Ok(())
    } else {
        Err(ConfigError::InvalidField(field))
    }
}

fn validate_environment_name(value: &str) -> Result<(), ConfigError> {
    validate_text("environment.allow", value)?;
    let mut characters = value.chars();
    let valid_start = characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic());
    if valid_start
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
    {
        Ok(())
    } else {
        Err(ConfigError::InvalidField("environment.allow"))
    }
}

fn validate_repository_identity(value: &str) -> Result<(), ConfigError> {
    let mut segments = value.split('/');
    let owner = segments.next().unwrap_or_default();
    let repository = segments.next().unwrap_or_default();
    validate_identifier("project", owner)?;
    validate_identifier("project", repository)?;
    if segments.next().is_some() {
        Err(ConfigError::InvalidField("project"))
    } else {
        Ok(())
    }
}

fn validate_relative_path(field: &'static str, value: &str) -> Result<(), ConfigError> {
    validate_text(field, value)?;
    let safe = value == "."
        || (!value.starts_with('/')
            && !value.starts_with('~')
            && !value.contains('\\')
            && !value.contains(':')
            && value
                .split('/')
                .all(|segment| !segment.is_empty() && segment != "." && segment != ".."));
    if safe {
        Ok(())
    } else {
        Err(ConfigError::InvalidField(field))
    }
}

fn validate_image_reference(value: &str) -> Result<(), ConfigError> {
    validate_text("runtime.image", value)?;
    let Some((name, digest)) = value.rsplit_once('@') else {
        return Err(ConfigError::InvalidField("runtime.image"));
    };
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return Err(ConfigError::InvalidField("runtime.image"));
    };
    if !name.is_empty()
        && !name.chars().any(char::is_whitespace)
        && !name.contains('@')
        && !name.contains("://")
        && !name.contains('\\')
        && hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(ConfigError::InvalidField("runtime.image"))
    }
}

fn validate_bounded(
    field: &'static str,
    value: u64,
    minimum: u64,
    maximum: u64,
) -> Result<(), ConfigError> {
    if (minimum..=maximum).contains(&value) {
        Ok(())
    } else {
        Err(ConfigError::OutOfRange {
            field,
            minimum,
            maximum,
            actual: value,
        })
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    Parse(toml::de::Error),
    Receipt(ReceiptError),
    UnsupportedSchemaVersion(String),
    ConfigTooLarge {
        actual: usize,
        maximum: usize,
    },
    InvalidField(&'static str),
    OutOfRange {
        field: &'static str,
        minimum: u64,
        maximum: u64,
        actual: u64,
    },
    TooManyItems {
        field: &'static str,
        actual: usize,
        maximum: usize,
    },
    NoChecks,
    NoRequiredChecks,
    DuplicateId {
        field: &'static str,
        id: String,
    },
    DuplicateValue(&'static str),
    DuplicateArtifact(String),
    PathOverlap {
        first: String,
        second: String,
    },
    SelfDependency(String),
    UnknownDependency {
        check: String,
        dependency: String,
    },
    DependencyCycle(Vec<String>),
    PlanDigestMismatch,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(
                formatter,
                "cannot read configuration {}: {source}",
                path.display()
            ),
            Self::Parse(error) => write!(formatter, "invalid TOML configuration: {error}"),
            Self::Receipt(error) => {
                write!(formatter, "cannot canonicalize execution plan: {error}")
            }
            Self::UnsupportedSchemaVersion(version) => {
                write!(
                    formatter,
                    "unsupported configuration schema version: {version}"
                )
            }
            Self::ConfigTooLarge { actual, maximum } => {
                write!(
                    formatter,
                    "configuration is {actual} bytes; maximum is {maximum}"
                )
            }
            Self::InvalidField(field) => {
                write!(formatter, "invalid configuration field: {field}")
            }
            Self::OutOfRange {
                field,
                minimum,
                maximum,
                actual,
            } => write!(
                formatter,
                "configuration field {field} is {actual}; expected {minimum}..={maximum}"
            ),
            Self::TooManyItems {
                field,
                actual,
                maximum,
            } => write!(
                formatter,
                "configuration has {actual} {field}; maximum is {maximum}"
            ),
            Self::NoChecks => write!(formatter, "configuration contains no checks"),
            Self::NoRequiredChecks => {
                write!(formatter, "configuration contains no required checks")
            }
            Self::DuplicateId { field, id } => write!(formatter, "duplicate {field}: {id}"),
            Self::DuplicateValue(field) => write!(formatter, "duplicate value in {field}"),
            Self::DuplicateArtifact(path) => write!(formatter, "duplicate artifact path: {path}"),
            Self::PathOverlap { first, second } => {
                write!(
                    formatter,
                    "configuration paths overlap: {first} and {second}"
                )
            }
            Self::SelfDependency(check) => write!(formatter, "check depends on itself: {check}"),
            Self::UnknownDependency { check, dependency } => {
                write!(
                    formatter,
                    "check {check} depends on unknown check {dependency}"
                )
            }
            Self::DependencyCycle(checks) => write!(
                formatter,
                "check dependency cycle involves: {}",
                checks.join(", ")
            ),
            Self::PlanDigestMismatch => write!(formatter, "execution plan digest mismatch"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse(error) => Some(error),
            Self::Receipt(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMAGE: &str = "ghcr.io/example/ci@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn valid_config(checks: &str) -> String {
        format!(
            r#"
schema_version = "1.0"
project = "example/project"

[runtime]
kind = "docker_compatible"
image = "{IMAGE}"
cpu_count = 4
memory_mib = 4096
pids_limit = 512

{checks}
"#
        )
    }

    fn check(id: &str, dependencies: &[&str]) -> String {
        let dependencies = dependencies
            .iter()
            .map(|dependency| format!("\"{dependency}\""))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            r#"
[[checks]]
id = "{id}"
required = true
argv = ["cargo", "test"]
working_directory = "."
timeout_seconds = 300
depends_on = [{dependencies}]
"#
        )
    }

    #[test]
    fn equivalent_declaration_orders_produce_identical_plan_bytes() {
        let first = valid_config(&(check("test", &["fmt"]) + &check("fmt", &[])));
        let second = valid_config(&(check("fmt", &[]) + &check("test", &["fmt"])));
        let first = ConfigV1::parse(&first)
            .and_then(ConfigV1::into_plan)
            .expect("first plan");
        let second = ConfigV1::parse(&second)
            .and_then(ConfigV1::into_plan)
            .expect("second plan");

        assert_eq!(first, second);
        assert_eq!(
            first.canonical_bytes().expect("first bytes"),
            second.canonical_bytes().expect("second bytes")
        );
        assert_eq!(first.plan.checks[0].id, "fmt");
        assert_eq!(first.plan.checks[1].id, "test");
    }

    #[test]
    fn dependency_cycles_are_rejected_deterministically() {
        let input = valid_config(&(check("a", &["b"]) + &check("b", &["a"])));
        let error = ConfigV1::parse(&input)
            .and_then(ConfigV1::into_plan)
            .expect_err("cycle must fail");
        assert!(matches!(
            error,
            ConfigError::DependencyCycle(checks) if checks == ["a", "b"]
        ));
    }

    #[test]
    fn unknown_and_self_dependencies_are_rejected() {
        let unknown = valid_config(&check("test", &["missing"]));
        assert!(matches!(
            ConfigV1::parse(&unknown).and_then(ConfigV1::into_plan),
            Err(ConfigError::UnknownDependency { .. })
        ));

        let own = valid_config(&check("test", &["test"]));
        assert!(matches!(
            ConfigV1::parse(&own).and_then(ConfigV1::into_plan),
            Err(ConfigError::SelfDependency(id)) if id == "test"
        ));
    }

    #[test]
    fn duplicate_check_ids_are_rejected() {
        let input = valid_config(&(check("test", &[]) + &check("test", &[])));
        assert!(matches!(
            ConfigV1::parse(&input).and_then(ConfigV1::into_plan),
            Err(ConfigError::DuplicateId {
                field: "check.id",
                ..
            })
        ));
    }

    #[test]
    fn unknown_toml_fields_are_rejected() {
        let input = valid_config(&check("test", &[])) + "\nunknown = true\n";
        assert!(matches!(
            ConfigV1::parse(&input),
            Err(ConfigError::Parse(_))
        ));
    }

    #[test]
    fn unsafe_paths_and_unpinned_images_are_rejected() {
        for path in ["../escape", "/tmp/output", r"C:\output", "nested//output"] {
            let mut input = valid_config(&check("test", &[]));
            input.push_str(&format!("\n[receipt]\noutput = '{path}'\n"));
            assert!(matches!(
                ConfigV1::parse(&input).and_then(ConfigV1::into_plan),
                Err(ConfigError::InvalidField("receipt.output"))
            ));
        }

        let input = valid_config(&check("test", &[])).replace(IMAGE, "ghcr.io/example/ci:latest");
        assert!(matches!(
            ConfigV1::parse(&input).and_then(ConfigV1::into_plan),
            Err(ConfigError::InvalidField("runtime.image"))
        ));
    }

    #[test]
    fn resource_limits_are_bounded() {
        let input =
            valid_config(&check("test", &[])).replace("memory_mib = 4096", "memory_mib = 0");
        assert!(matches!(
            ConfigV1::parse(&input).and_then(ConfigV1::into_plan),
            Err(ConfigError::OutOfRange {
                field: "runtime.memory_mib",
                ..
            })
        ));
    }

    #[test]
    fn configuration_size_is_bounded_before_parsing() {
        let input = "x".repeat(MAX_CONFIG_BYTES + 1);
        assert!(matches!(
            ConfigV1::parse(&input),
            Err(ConfigError::ConfigTooLarge { .. })
        ));
    }

    #[test]
    fn environment_names_and_duplicate_values_are_rejected() {
        let base = valid_config(&check("test", &[]));
        let invalid = base.clone() + "\n[environment]\nallow = [\"VALID\", \"NOT-VALID\"]\n";
        assert!(matches!(
            ConfigV1::parse(&invalid).and_then(ConfigV1::into_plan),
            Err(ConfigError::InvalidField("environment.allow"))
        ));

        let duplicate = base + "\n[environment]\nallow = [\"CI\", \"CI\"]\n";
        assert!(matches!(
            ConfigV1::parse(&duplicate).and_then(ConfigV1::into_plan),
            Err(ConfigError::DuplicateValue("environment.allow"))
        ));
    }

    #[test]
    fn plan_digest_detects_mutation() {
        let input = valid_config(&check("test", &[]));
        let mut plan = ConfigV1::parse(&input)
            .and_then(ConfigV1::into_plan)
            .expect("plan");
        plan.plan.runtime.network = true;
        assert!(matches!(
            plan.canonical_bytes(),
            Err(ConfigError::PlanDigestMismatch)
        ));
    }

    #[test]
    fn at_least_one_required_check_is_mandatory() {
        let input =
            valid_config(&check("test", &[])).replace("required = true", "required = false");
        assert!(matches!(
            ConfigV1::parse(&input).and_then(ConfigV1::into_plan),
            Err(ConfigError::NoRequiredChecks)
        ));
    }

    #[test]
    fn image_reference_cannot_embed_credentials_or_url_scheme() {
        for image in [
            "user:password@registry.example/image@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "https://registry.example/image@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            let input = valid_config(&check("test", &[])).replace(IMAGE, image);
            assert!(matches!(
                ConfigV1::parse(&input).and_then(ConfigV1::into_plan),
                Err(ConfigError::InvalidField("runtime.image"))
            ));
        }
    }

    #[test]
    fn cache_receipt_and_artifact_paths_cannot_overlap() {
        let cache_overlap = valid_config(&check("test", &[]))
            + "\n[[caches]]\nid = \"first\"\nmount_path = \"cache\"\n"
            + "\n[[caches]]\nid = \"second\"\nmount_path = \"cache/nested\"\n";
        assert!(matches!(
            ConfigV1::parse(&cache_overlap).and_then(ConfigV1::into_plan),
            Err(ConfigError::PathOverlap { .. })
        ));

        let receipt_overlap = valid_config(&check("test", &[]))
            + "\n[[caches]]\nid = \"receipt\"\nmount_path = \".ccp\"\n";
        assert!(matches!(
            ConfigV1::parse(&receipt_overlap).and_then(ConfigV1::into_plan),
            Err(ConfigError::PathOverlap { .. })
        ));

        let duplicate_artifact = valid_config(
            &(check("first", &[]).replace(
                "depends_on = []",
                "depends_on = []\nartifacts = [\"build/output\"]",
            ) + &check("second", &[]).replace(
                "depends_on = []",
                "depends_on = []\nartifacts = [\"build/output\"]",
            )),
        );
        assert!(matches!(
            ConfigV1::parse(&duplicate_artifact).and_then(ConfigV1::into_plan),
            Err(ConfigError::DuplicateArtifact(path)) if path == "build/output"
        ));
    }
}
