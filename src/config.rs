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
pub const MAX_STORAGE_BYTES: u64 = 1_099_511_627_776;
const MIN_RECEIPT_JOURNAL_RESERVE_BYTES: u64 = 4_096;

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
    #[serde(default)]
    pub storage: Option<StorageConfig>,
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
    #[serde(default)]
    pub pull_policy: Option<RuntimePullPolicy>,
    #[serde(default)]
    pub swap_mode: Option<RuntimeSwapMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    DockerCompatible,
    Host,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimePullPolicy {
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeSwapMode {
    Disabled,
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
    pub fixed: BTreeMap<String, String>,
    pub runtime_internal: Vec<RuntimeInternalEnvironmentConfig>,
    pub remote_secret_only: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeInternalEnvironmentConfig {
    pub name: String,
    pub cache_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CacheConfig {
    pub id: String,
    pub mount_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StorageConfig {
    pub min_free_bytes: u64,
    pub receipt_journal_reserve_bytes: u64,
    pub max_cache_growth_bytes: u64,
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
    #[serde(default)]
    pub artifact_contracts: Vec<ArtifactContractConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtifactContractConfig {
    pub path: String,
    pub kind: ArtifactKind,
    pub max_bytes: u64,
    pub max_entries: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactKind {
    RegularFile,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionPlanEnvelopeV1 {
    pub plan_digest: String,
    pub plan: ExecutionPlanV1,
    #[serde(skip)]
    pub fixed_environment: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPlanV1 {
    pub schema_version: String,
    pub project: String,
    pub runtime: NormalizedRuntime,
    pub receipt: NormalizedReceipt,
    pub environment: NormalizedEnvironment,
    pub caches: Vec<NormalizedCache>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<NormalizedStorage>,
    pub checks: Vec<NormalizedCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NormalizedRuntime {
    pub kind: RuntimeKind,
    pub image: String,
    pub cpu_count: u16,
    pub memory_mib: u64,
    pub pids_limit: u32,
    pub network: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pull_policy: Option<RuntimePullPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swap_mode: Option<RuntimeSwapMode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NormalizedReceipt {
    pub output: String,
    pub freshness_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NormalizedEnvironment {
    pub inherit: Vec<String>,
    pub fixed: Vec<NormalizedFixedEnvironment>,
    pub runtime_internal: Vec<NormalizedRuntimeInternalEnvironment>,
    pub remote_secret_only: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NormalizedFixedEnvironment {
    pub name: String,
    pub value_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NormalizedRuntimeInternalEnvironment {
    pub name: String,
    pub cache_id: String,
    pub container_target: String,
}

impl NormalizedEnvironment {
    pub fn names(&self) -> Vec<String> {
        let mut names = self.inherit.clone();
        names.extend(self.fixed.iter().map(|binding| binding.name.clone()));
        names.extend(
            self.runtime_internal
                .iter()
                .map(|binding| binding.name.clone()),
        );
        names.sort();
        names
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NormalizedCache {
    pub id: String,
    pub mount_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NormalizedStorage {
    pub min_free_bytes: u64,
    pub receipt_journal_reserve_bytes: u64,
    pub max_cache_growth_bytes: u64,
    pub max_artifact_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NormalizedCheck {
    pub id: String,
    pub required: bool,
    pub argv: Vec<String>,
    pub working_directory: String,
    pub timeout_seconds: u64,
    pub depends_on: Vec<String>,
    pub artifacts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_contracts: Vec<NormalizedArtifactContract>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NormalizedArtifactContract {
    pub path: String,
    pub kind: ArtifactKind,
    pub max_bytes: u64,
    pub max_entries: u64,
    pub producer_check: String,
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
        let schema_version = self.schema_version.clone();
        let fixed_environment = self.environment.fixed.clone();
        let caches = normalize_caches(self.caches)?;
        let runtime = normalize_runtime(&schema_version, self.runtime)?;
        let environment = normalize_environment(&schema_version, self.environment, &caches)?;
        let checks = normalize_checks(self.checks)?;
        let storage = normalize_storage(&schema_version, self.storage, &checks)?;
        let receipt = NormalizedReceipt {
            output: self.receipt.output,
            freshness_seconds: self.receipt.freshness_seconds,
        };
        validate_path_isolation(&receipt, &caches, &checks)?;
        let plan = ExecutionPlanV1 {
            schema_version,
            project: self.project,
            runtime,
            receipt,
            environment,
            caches,
            storage,
            checks,
        };
        let plan_digest = canonical_digest(&plan).map_err(ConfigError::Receipt)?;
        Ok(ExecutionPlanEnvelopeV1 {
            plan_digest,
            plan,
            fixed_environment,
        })
    }

    fn validate_top_level(&self) -> Result<(), ConfigError> {
        if !matches!(
            self.schema_version.as_str(),
            CONFIG_SCHEMA_VERSION | "1.1" | "1.2" | "1.3"
        ) {
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

fn normalize_runtime(
    schema_version: &str,
    runtime: RuntimeConfig,
) -> Result<NormalizedRuntime, ConfigError> {
    let (pull_policy, swap_mode) = if schema_version == "1.3" {
        (
            Some(
                runtime
                    .pull_policy
                    .ok_or(ConfigError::MissingRuntimeCapabilityPolicy)?,
            ),
            Some(
                runtime
                    .swap_mode
                    .ok_or(ConfigError::MissingRuntimeCapabilityPolicy)?,
            ),
        )
    } else {
        if runtime.pull_policy.is_some() {
            return Err(ConfigError::InvalidField("runtime.pull_policy"));
        }
        if runtime.swap_mode.is_some() {
            return Err(ConfigError::InvalidField("runtime.swap_mode"));
        }
        (None, None)
    };

    Ok(NormalizedRuntime {
        kind: runtime.kind,
        image: runtime.image,
        cpu_count: runtime.cpu_count,
        memory_mib: runtime.memory_mib,
        pids_limit: runtime.pids_limit,
        network: runtime.network,
        pull_policy,
        swap_mode,
    })
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
        validate_artifact_contracts(&check)?;
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

fn normalize_storage(
    schema_version: &str,
    storage: Option<StorageConfig>,
    checks: &[NormalizedCheck],
) -> Result<Option<NormalizedStorage>, ConfigError> {
    if !matches!(schema_version, "1.2" | "1.3") {
        if storage.is_some() {
            return Err(ConfigError::InvalidField("storage"));
        }
        return Ok(None);
    }
    let storage = storage.ok_or(ConfigError::MissingStoragePolicy)?;
    validate_bounded(
        "storage.min_free_bytes",
        storage.min_free_bytes,
        1,
        MAX_STORAGE_BYTES,
    )?;
    validate_bounded(
        "storage.receipt_journal_reserve_bytes",
        storage.receipt_journal_reserve_bytes,
        MIN_RECEIPT_JOURNAL_RESERVE_BYTES,
        MAX_STORAGE_BYTES,
    )?;
    validate_bounded(
        "storage.max_cache_growth_bytes",
        storage.max_cache_growth_bytes,
        0,
        MAX_STORAGE_BYTES,
    )?;
    let max_artifact_bytes = checks
        .iter()
        .flat_map(|check| check.artifact_contracts.iter())
        .try_fold(0_u64, |total, artifact| {
            total.checked_add(artifact.max_bytes)
        })
        .ok_or(ConfigError::InvalidField("storage.max_artifact_bytes"))?;
    let required = storage
        .min_free_bytes
        .checked_add(storage.receipt_journal_reserve_bytes)
        .and_then(|total| total.checked_add(storage.max_cache_growth_bytes))
        .and_then(|total| total.checked_add(max_artifact_bytes))
        .ok_or(ConfigError::InvalidField("storage"))?;
    if required > MAX_STORAGE_BYTES {
        return Err(ConfigError::OutOfRange {
            field: "storage.required_free_bytes",
            minimum: 1,
            maximum: MAX_STORAGE_BYTES,
            actual: required,
        });
    }
    Ok(Some(NormalizedStorage {
        min_free_bytes: storage.min_free_bytes,
        receipt_journal_reserve_bytes: storage.receipt_journal_reserve_bytes,
        max_cache_growth_bytes: storage.max_cache_growth_bytes,
        max_artifact_bytes,
    }))
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

    let mut artifacts: BTreeSet<&str> = BTreeSet::new();
    for check in checks {
        for artifact in &check.artifacts {
            if artifacts.contains(artifact.as_str()) {
                return Err(ConfigError::DuplicateArtifact(artifact.clone()));
            }
            for other in &artifacts {
                reject_path_overlap(artifact, other)?;
            }
            reject_path_overlap(artifact, &receipt.output)?;
            for cache in caches {
                reject_path_overlap(artifact, &cache.mount_path)?;
            }
            artifacts.insert(artifact.as_str());
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
        artifact_contracts: check
            .artifact_contracts
            .iter()
            .map(|artifact| NormalizedArtifactContract {
                path: artifact.path.clone(),
                kind: artifact.kind,
                max_bytes: artifact.max_bytes,
                max_entries: artifact.max_entries,
                producer_check: check.id.clone(),
            })
            .collect(),
    }
}

fn validate_artifact_contracts(check: &CheckConfig) -> Result<(), ConfigError> {
    let mut paths = BTreeSet::new();
    for artifact in &check.artifact_contracts {
        validate_relative_path("check.artifact_contracts.path", &artifact.path)?;
        if artifact.path == "." || !check.artifacts.contains(&artifact.path) {
            return Err(ConfigError::InvalidField("check.artifact_contracts.path"));
        }
        if !paths.insert(&artifact.path) {
            return Err(ConfigError::DuplicateValue("check.artifact_contracts.path"));
        }
        validate_bounded(
            "check.artifact_contracts.max_bytes",
            artifact.max_bytes,
            1,
            1_073_741_824,
        )?;
        let maximum_entries = match artifact.kind {
            ArtifactKind::RegularFile => 1,
            ArtifactKind::Directory => 10_000,
        };
        validate_bounded(
            "check.artifact_contracts.max_entries",
            artifact.max_entries,
            1,
            maximum_entries,
        )?;
        if artifact.kind == ArtifactKind::RegularFile && artifact.max_entries != 1 {
            return Err(ConfigError::InvalidField(
                "check.artifact_contracts.max_entries",
            ));
        }
    }
    Ok(())
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

fn normalize_environment(
    schema_version: &str,
    environment: EnvironmentConfig,
    caches: &[NormalizedCache],
) -> Result<NormalizedEnvironment, ConfigError> {
    let inherit = unique_sorted(
        "environment.allow",
        environment.allow,
        validate_environment_name,
    )?;
    let remote_secret_only = unique_sorted(
        "environment.remote_secret_only",
        environment.remote_secret_only,
        validate_environment_name,
    )?;
    if schema_version == CONFIG_SCHEMA_VERSION
        && (!environment.fixed.is_empty()
            || !environment.runtime_internal.is_empty()
            || !remote_secret_only.is_empty())
    {
        return Err(ConfigError::InvalidField("environment"));
    }
    if schema_version != CONFIG_SCHEMA_VERSION && !inherit.is_empty() {
        return Err(ConfigError::InvalidField("environment.allow"));
    }

    let mut names = BTreeSet::new();
    for name in &inherit {
        names.insert(name.clone());
    }
    let mut fixed = Vec::with_capacity(environment.fixed.len());
    for (name, value) in environment.fixed {
        validate_environment_name(&name)?;
        validate_text("environment.fixed", &value)?;
        if !names.insert(name.clone()) {
            return Err(ConfigError::DuplicateValue("environment"));
        }
        fixed.push(NormalizedFixedEnvironment {
            value_digest: canonical_digest(&value).map_err(ConfigError::Receipt)?,
            name,
        });
    }
    let cache_targets = caches
        .iter()
        .map(|cache| (cache.id.as_str(), cache.mount_path.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut runtime_internal = Vec::with_capacity(environment.runtime_internal.len());
    for binding in environment.runtime_internal {
        validate_environment_name(&binding.name)?;
        validate_identifier("environment.runtime_internal.cache_id", &binding.cache_id)?;
        if !names.insert(binding.name.clone()) {
            return Err(ConfigError::DuplicateValue("environment"));
        }
        let mount_path = cache_targets
            .get(binding.cache_id.as_str())
            .ok_or_else(|| ConfigError::UnknownEnvironmentCache {
                name: binding.name.clone(),
                cache_id: binding.cache_id.clone(),
            })?;
        runtime_internal.push(NormalizedRuntimeInternalEnvironment {
            name: binding.name,
            cache_id: binding.cache_id,
            container_target: format!("/workspace/{mount_path}"),
        });
    }
    for name in &remote_secret_only {
        if !names.insert(name.clone()) {
            return Err(ConfigError::DuplicateValue("environment"));
        }
    }
    fixed.sort_by(|left, right| left.name.cmp(&right.name));
    runtime_internal.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(NormalizedEnvironment {
        inherit,
        fixed,
        runtime_internal,
        remote_secret_only,
    })
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
    UnknownEnvironmentCache {
        name: String,
        cache_id: String,
    },
    MissingStoragePolicy,
    MissingRuntimeCapabilityPolicy,
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
            Self::UnknownEnvironmentCache { name, cache_id } => write!(
                formatter,
                "runtime-internal environment {name} references unknown cache {cache_id}"
            ),
            Self::MissingStoragePolicy => {
                write!(
                    formatter,
                    "schemas 1.2 and 1.3 require an explicit storage policy"
                )
            }
            Self::MissingRuntimeCapabilityPolicy => write!(
                formatter,
                "schema 1.3 requires pull_policy = never and swap_mode = disabled"
            ),
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
    fn v1_1_environment_classes_normalize_without_host_inheritance() {
        let input = r#"
schema_version = "1.1"
project = "owner/project"

[runtime]
kind = "docker_compatible"
image = "registry.example/ci@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
cpu_count = 2
memory_mib = 256
pids_limit = 64

[environment]
remote_secret_only = ["DEPLOY_TOKEN"]

[environment.fixed]
SOURCE_DATE_EPOCH = "0"

[[environment.runtime_internal]]
name = "CARGO_HOME"
cache_id = "cargo-home"

[[caches]]
id = "cargo-home"
mount_path = ".ccp-mounts/cargo-home"

[[checks]]
id = "format"
required = true
argv = ["cargo", "fmt", "--check"]
working_directory = "."
timeout_seconds = 60
"#;

        let plan = ConfigV1::parse(input)
            .and_then(ConfigV1::into_plan)
            .expect("v1.1 environment plan");

        assert!(plan.plan.environment.inherit.is_empty());
        assert_eq!(plan.plan.environment.fixed.len(), 1);
        assert_eq!(plan.plan.environment.runtime_internal.len(), 1);
        assert_eq!(
            plan.plan.environment.runtime_internal[0].container_target,
            "/workspace/.ccp-mounts/cargo-home"
        );
        assert_eq!(
            plan.plan.environment.remote_secret_only,
            vec!["DEPLOY_TOKEN".to_owned()]
        );
        assert!(
            !plan
                .canonical_bytes()
                .expect("public plan bytes")
                .windows(b"SOURCE_DATE_EPOCH=0".len())
                .any(|window| window == b"SOURCE_DATE_EPOCH=0")
        );
    }

    #[test]
    fn v1_1_runtime_internal_unknown_cache_fails_closed() {
        let input = valid_config(&check("format", &[]))
            .replace("schema_version = \"1.0\"", "schema_version = \"1.1\"")
            + "\n[[environment.runtime_internal]]\nname = \"CARGO_HOME\"\ncache_id = \"missing\"\n";
        assert!(matches!(
            ConfigV1::parse(&input).and_then(ConfigV1::into_plan),
            Err(ConfigError::UnknownEnvironmentCache { .. })
        ));
    }

    #[test]
    fn v1_2_storage_policy_is_explicit_and_normalized_into_the_plan() {
        let input = valid_config(&check("format", &[]))
            .replace("schema_version = \"1.0\"", "schema_version = \"1.2\"")
            + r#"

[storage]
min_free_bytes = 1073741824
receipt_journal_reserve_bytes = 1048576
max_cache_growth_bytes = 2147483648
"#;

        let plan = ConfigV1::parse(&input)
            .and_then(ConfigV1::into_plan)
            .expect("v1.2 storage plan");

        let storage = plan.plan.storage.expect("storage policy");
        assert_eq!(storage.min_free_bytes, 1_073_741_824);
        assert_eq!(storage.receipt_journal_reserve_bytes, 1_048_576);
        assert_eq!(storage.max_cache_growth_bytes, 2_147_483_648);
        assert_eq!(storage.max_artifact_bytes, 0);
    }

    #[test]
    fn v1_2_storage_policy_is_required_and_bounded() {
        let missing = valid_config(&check("format", &[]))
            .replace("schema_version = \"1.0\"", "schema_version = \"1.2\"");
        assert!(matches!(
            ConfigV1::parse(&missing).and_then(ConfigV1::into_plan),
            Err(ConfigError::MissingStoragePolicy)
        ));

        let invalid = valid_config(&check("format", &[]))
            .replace("schema_version = \"1.0\"", "schema_version = \"1.2\"")
            + r#"

[storage]
min_free_bytes = 0
receipt_journal_reserve_bytes = 1048576
max_cache_growth_bytes = 2147483648
"#;
        assert!(matches!(
            ConfigV1::parse(&invalid).and_then(ConfigV1::into_plan),
            Err(ConfigError::OutOfRange {
                field: "storage.min_free_bytes",
                ..
            })
        ));
    }

    #[test]
    fn v1_2_storage_policy_changes_the_plan_digest() {
        let base = valid_config(&check("format", &[]))
            .replace("schema_version = \"1.0\"", "schema_version = \"1.2\"")
            + r#"

[storage]
min_free_bytes = 1073741824
receipt_journal_reserve_bytes = 1048576
max_cache_growth_bytes = 2147483648
"#;
        let changed = base.replace(
            "max_cache_growth_bytes = 2147483648",
            "max_cache_growth_bytes = 3221225472",
        );

        let first = ConfigV1::parse(&base)
            .and_then(ConfigV1::into_plan)
            .expect("first plan");
        let second = ConfigV1::parse(&changed)
            .and_then(ConfigV1::into_plan)
            .expect("second plan");

        assert_ne!(first.plan_digest, second.plan_digest);
    }

    #[test]
    fn v1_3_requires_explicit_runtime_capability_policy() {
        let input = valid_config(&check("format", &[]))
            .replace("schema_version = \"1.0\"", "schema_version = \"1.3\"")
            + r#"

[storage]
min_free_bytes = 1073741824
receipt_journal_reserve_bytes = 1048576
max_cache_growth_bytes = 2147483648
"#;

        assert!(matches!(
            ConfigV1::parse(&input).and_then(ConfigV1::into_plan),
            Err(ConfigError::MissingRuntimeCapabilityPolicy)
        ));
    }

    #[test]
    fn v1_3_runtime_policy_changes_the_plan_digest() {
        let base = valid_config(&check("format", &[]))
            .replace("schema_version = \"1.0\"", "schema_version = \"1.3\"")
            .replace(
                "pids_limit = 512\n",
                "pids_limit = 512\npull_policy = \"never\"\nswap_mode = \"disabled\"\n",
            )
            + r#"

[storage]
min_free_bytes = 1073741824
receipt_journal_reserve_bytes = 1048576
max_cache_growth_bytes = 2147483648
"#;
        let first = ConfigV1::parse(&base)
            .and_then(ConfigV1::into_plan)
            .expect("first schema 1.3 plan");
        let mut changed = first.plan.clone();
        changed.runtime.swap_mode = None;

        assert_ne!(
            canonical_digest(&first.plan).expect("first digest"),
            canonical_digest(&changed).expect("changed digest")
        );
    }

    #[test]
    fn v1_3_requires_storage_and_historical_schemas_reject_runtime_policy() {
        let missing_storage = valid_config(&check("format", &[]))
            .replace("schema_version = \"1.0\"", "schema_version = \"1.3\"")
            .replace(
                "pids_limit = 512\n",
                "pids_limit = 512\npull_policy = \"never\"\nswap_mode = \"disabled\"\n",
            );
        assert!(matches!(
            ConfigV1::parse(&missing_storage).and_then(ConfigV1::into_plan),
            Err(ConfigError::MissingStoragePolicy)
        ));

        let historical = valid_config(&check("format", &[])).replace(
            "pids_limit = 512\n",
            "pids_limit = 512\npull_policy = \"never\"\nswap_mode = \"disabled\"\n",
        );
        assert!(matches!(
            ConfigV1::parse(&historical).and_then(ConfigV1::into_plan),
            Err(ConfigError::InvalidField("runtime.pull_policy"))
        ));
    }

    #[test]
    fn v1_2_storage_policy_derives_declared_artifact_allowance() {
        let check = check("report", &[]).replace(
            "depends_on = []",
            r#"depends_on = []
artifacts = ["results/report.json"]

[[checks.artifact_contracts]]
path = "results/report.json"
kind = "regular-file"
max_bytes = 4096
max_entries = 1"#,
        );
        let input = valid_config(&check)
            .replace("schema_version = \"1.0\"", "schema_version = \"1.2\"")
            + r#"

[storage]
min_free_bytes = 100
receipt_journal_reserve_bytes = 4096
max_cache_growth_bytes = 0
"#;

        let plan = ConfigV1::parse(&input)
            .and_then(ConfigV1::into_plan)
            .expect("v1.2 storage plan");
        assert_eq!(
            plan.plan
                .storage
                .expect("storage policy")
                .max_artifact_bytes,
            4096
        );
    }

    #[test]
    fn storage_policy_is_rejected_by_historical_schema_versions() {
        let input = valid_config(&check("format", &[]))
            .replace("schema_version = \"1.0\"", "schema_version = \"1.1\"")
            + r#"

[storage]
min_free_bytes = 1073741824
receipt_journal_reserve_bytes = 1048576
max_cache_growth_bytes = 0
"#;
        assert!(matches!(
            ConfigV1::parse(&input).and_then(ConfigV1::into_plan),
            Err(ConfigError::InvalidField("storage"))
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

        let nested_artifacts = valid_config(
            &(check("parent", &[]).replace(
                "depends_on = []",
                "depends_on = []\nartifacts = [\"build/reports\"]",
            ) + &check("child", &[]).replace(
                "depends_on = []",
                "depends_on = []\nartifacts = [\"build/reports/result.json\"]",
            )),
        );
        assert!(matches!(
            ConfigV1::parse(&nested_artifacts).and_then(ConfigV1::into_plan),
            Err(ConfigError::PathOverlap { .. })
        ));
    }

    #[test]
    fn artifact_contract_requires_a_bounded_regular_file_owned_by_its_check() {
        let input = valid_config(&check("test", &[])).replace(
            "depends_on = []",
            "depends_on = []\nartifacts = [\"results/report.json\"]\n\n[[checks.artifact_contracts]]\npath = \"results/report.json\"\nkind = \"regular-file\"\nmax_bytes = 1048576\nmax_entries = 1",
        );
        let plan = ConfigV1::parse(&input)
            .and_then(ConfigV1::into_plan)
            .expect("artifact contract plan");
        assert_eq!(plan.plan.checks[0].artifact_contracts.len(), 1);
        assert_eq!(
            plan.plan.checks[0].artifact_contracts[0].producer_check,
            "test"
        );
    }

    #[test]
    fn artifact_contract_rejects_undeclared_paths_and_unbounded_directory_shape() {
        let base = valid_config(&check("test", &[])).replace(
            "depends_on = []",
            "depends_on = []\nartifacts = [\"results\"]\n\n[[checks.artifact_contracts]]\npath = \"other\"\nkind = \"regular-file\"\nmax_bytes = 1\nmax_entries = 1",
        );
        assert!(matches!(
            ConfigV1::parse(&base).and_then(ConfigV1::into_plan),
            Err(ConfigError::InvalidField("check.artifact_contracts.path"))
        ));

        let directory = base
            .replace("path = \"other\"", "path = \"results\"")
            .replace("kind = \"regular-file\"", "kind = \"directory\"")
            .replace("max_entries = 1", "max_entries = 10001");
        assert!(matches!(
            ConfigV1::parse(&directory).and_then(ConfigV1::into_plan),
            Err(ConfigError::OutOfRange {
                field: "check.artifact_contracts.max_entries",
                ..
            })
        ));
    }
}
