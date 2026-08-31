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
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::{
    CacheConfig, CheckConfig, ConfigError, ConfigV1, EnvironmentConfig, NormalizedCache,
    NormalizedCheck, NormalizedEnvironment, NormalizedRuntime, NormalizedStorage, ReceiptConfig,
    RuntimeConfig, RuntimeKind, StorageConfig,
};
use crate::receipt::{ReceiptError, canonical_digest, canonical_json};

pub const CAPABILITY_PACK_SCHEMA_VERSION: &str = "1.0";
pub const MAX_CAPABILITY_PACK_BYTES: usize = 1_048_576;
const MAX_PACK_PROFILES: usize = 32;
const MAX_PACK_SOURCES: usize = 16;
const MAX_PROFILE_TOOLS: usize = 32;
const MAX_PROFILE_INPUTS: usize = 64;
const MAX_PROFILE_HOSTS: usize = 8;
const MAX_PROFILE_TARGETS: usize = 8;
const MAX_PROFILE_FEATURES: usize = 16;
const MAX_BLIND_SPOTS: usize = 32;
const MAX_LICENSE_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityPackManifestV1 {
    pub schema_version: String,
    pub pack_id: String,
    pub pack_version: String,
    pub license: String,
    pub description: String,
    pub upstream_sources: Vec<CapabilitySourceV1>,
    pub profiles: Vec<CapabilityProfileConfigV1>,
}

impl CapabilityPackManifestV1 {
    pub fn parse(input: &str) -> Result<Self, CapabilityPackError> {
        if input.len() > MAX_CAPABILITY_PACK_BYTES {
            return Err(CapabilityPackError::ManifestTooLarge {
                actual: input.len(),
                maximum: MAX_CAPABILITY_PACK_BYTES,
            });
        }
        toml::from_str(input).map_err(CapabilityPackError::Parse)
    }

    pub fn load(path: &Path) -> Result<Self, CapabilityPackError> {
        let metadata = fs::metadata(path).map_err(|source| CapabilityPackError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let size = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
        if size > MAX_CAPABILITY_PACK_BYTES {
            return Err(CapabilityPackError::ManifestTooLarge {
                actual: size,
                maximum: MAX_CAPABILITY_PACK_BYTES,
            });
        }
        let input = fs::read_to_string(path).map_err(|source| CapabilityPackError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&input)
    }

    pub fn validate(self) -> Result<CapabilityPackEnvelopeV1, CapabilityPackError> {
        if self.schema_version != CAPABILITY_PACK_SCHEMA_VERSION {
            return Err(CapabilityPackError::UnsupportedSchemaVersion(
                self.schema_version,
            ));
        }
        validate_identifier("pack_id", &self.pack_id)?;
        validate_text("description", &self.description)?;
        validate_license("license", &self.license)?;
        validate_semver(&self.pack_version)?;
        let upstream_sources = validate_sources(&self.upstream_sources)?;
        if self.profiles.is_empty() {
            return Err(CapabilityPackError::InvalidField("profiles"));
        }
        if self.profiles.len() > MAX_PACK_PROFILES {
            return Err(CapabilityPackError::TooManyItems {
                field: "profiles",
                actual: self.profiles.len(),
                maximum: MAX_PACK_PROFILES,
            });
        }
        let mut profiles = Vec::new();
        let mut ids = BTreeSet::new();
        let mut configs = BTreeMap::new();
        for profile in self.profiles {
            if !ids.insert(profile.id.clone()) {
                return Err(CapabilityPackError::DuplicateId {
                    field: "profiles.id",
                    id: profile.id,
                });
            }
            let (normalized, raw) = validate_profile(profile)?;
            configs.insert(normalized.id.clone(), raw);
            profiles.push(normalized);
        }
        profiles.sort_by(|left, right| left.id.cmp(&right.id));
        let pack = NormalizedCapabilityPackV1 {
            schema_version: self.schema_version,
            pack_id: self.pack_id,
            pack_version: self.pack_version,
            license: self.license,
            description: self.description,
            upstream_sources,
            profiles,
        };
        let pack_digest = canonical_digest(&pack)?;
        Ok(CapabilityPackEnvelopeV1 {
            pack_digest,
            pack,
            profile_configs: configs,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapabilityPackEnvelopeV1 {
    pub pack_digest: String,
    pub pack: NormalizedCapabilityPackV1,
    #[allow(dead_code)]
    #[serde(skip)]
    profile_configs: BTreeMap<String, ValidatedProfileConfigV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NormalizedCapabilityPackV1 {
    pub schema_version: String,
    pub pack_id: String,
    pub pack_version: String,
    pub license: String,
    pub description: String,
    pub upstream_sources: Vec<CapabilitySourceV1>,
    pub profiles: Vec<NormalizedCapabilityProfileV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NormalizedCapabilityProfileV1 {
    pub id: String,
    pub description: String,
    pub evidence_class: CapabilityEvidenceClassV1,
    pub pass_semantics: String,
    pub known_blind_spots: Vec<String>,
    pub supported_hosts: Vec<CapabilityHostPlatformV1>,
    pub target_platforms: Vec<CapabilityTargetPlatformV1>,
    pub required_runtime_features: Vec<CapabilityRuntimeFeatureV1>,
    pub offline_preparation: OfflinePreparationV1,
    pub tools: Vec<CapabilityToolV1>,
    pub inputs: Vec<CapabilityInputProvenanceV1>,
    pub runtime: NormalizedRuntime,
    pub environment: NormalizedEnvironment,
    pub caches: Vec<NormalizedCache>,
    pub storage: NormalizedStorage,
    pub checks: Vec<NormalizedCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ValidatedProfileConfigV1 {
    evidence_class: CapabilityEvidenceClassV1,
    runtime: RuntimeConfig,
    environment: EnvironmentConfig,
    caches: Vec<CacheConfig>,
    storage: StorageConfig,
    checks: Vec<CheckConfig>,
}

impl CapabilityPackEnvelopeV1 {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CapabilityPackError> {
        if canonical_digest(&self.pack)? != self.pack_digest {
            return Err(CapabilityPackError::PackDigestMismatch);
        }
        Ok(canonical_json(self)?)
    }
    pub fn inspection(&self) -> &NormalizedCapabilityPackV1 {
        &self.pack
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilitySourceV1 {
    pub id: String,
    pub url: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityProfileConfigV1 {
    pub id: String,
    pub description: String,
    pub evidence_class: CapabilityEvidenceClassV1,
    pub pass_semantics: String,
    pub known_blind_spots: Vec<String>,
    pub supported_hosts: Vec<CapabilityHostPlatformV1>,
    pub target_platforms: Vec<CapabilityTargetPlatformV1>,
    pub required_runtime_features: Vec<CapabilityRuntimeFeatureV1>,
    pub offline_preparation: OfflinePreparationV1,
    pub tools: Vec<CapabilityToolV1>,
    #[serde(default)]
    pub inputs: Vec<CapabilityInputProvenanceV1>,
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub environment: EnvironmentConfig,
    #[serde(default)]
    pub caches: Vec<CacheConfig>,
    pub storage: StorageConfig,
    pub checks: Vec<CheckConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityToolV1 {
    pub id: String,
    pub version: String,
    pub license: String,
    pub url: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityInputProvenanceV1 {
    pub id: String,
    pub kind: CapabilityInputKindV1,
    pub url: String,
    pub digest: String,
    #[serde(default)]
    pub snapshot_created_at_utc: Option<String>,
    #[serde(default)]
    pub max_age_seconds: Option<u64>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityEvidenceClassV1 {
    Deterministic,
    ScheduleSensitive,
    BoundedNondeterministic,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityInputKindV1 {
    Rules,
    TypeStubs,
    Corpus,
    AdvisoryDatabase,
    VulnerabilityDatabase,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityHostPlatformV1 {
    MacosAarch64,
    LinuxArm64,
    LinuxAmd64,
    WindowsAmd64,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityTargetPlatformV1 {
    LinuxArm64,
    LinuxAmd64,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityRuntimeFeatureV1 {
    LinuxUserland,
    NoNetwork,
    ReadOnlySource,
    WritableCaches,
    BoundedArtifacts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OfflinePreparationV1 {
    None,
    RequiredExternal,
}

#[derive(Debug)]
pub enum CapabilityPackError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse(toml::de::Error),
    Json(serde_json::Error),
    Receipt(ReceiptError),
    Config(ConfigError),
    UnsupportedSchemaVersion(String),
    ManifestTooLarge {
        actual: usize,
        maximum: usize,
    },
    InvalidField(&'static str),
    TooManyItems {
        field: &'static str,
        actual: usize,
        maximum: usize,
    },
    DuplicateId {
        field: &'static str,
        id: String,
    },
    DuplicateValue(&'static str),
    ShellEntrypoint(String),
    UnknownProfile(String),
    PackDigestMismatch,
}

impl fmt::Display for CapabilityPackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "cannot read capability pack {path:?}: {source}")
            }
            Self::Parse(error) => write!(formatter, "invalid capability pack TOML: {error}"),
            Self::Json(error) => write!(
                formatter,
                "capability pack JSON schema generation failed: {error}"
            ),
            Self::Receipt(error) => {
                write!(formatter, "cannot build capability pack identity: {error}")
            }
            Self::Config(error) => {
                write!(
                    formatter,
                    "invalid embedded config in capability pack: {error}"
                )
            }
            Self::UnsupportedSchemaVersion(version) => {
                write!(
                    formatter,
                    "unsupported capability pack schema version: {version}"
                )
            }
            Self::ManifestTooLarge { actual, maximum } => {
                write!(
                    formatter,
                    "capability pack is {actual} bytes; maximum is {maximum}"
                )
            }
            Self::InvalidField(field) => {
                write!(formatter, "invalid capability pack field: {field}")
            }
            Self::TooManyItems {
                field,
                actual,
                maximum,
            } => {
                write!(
                    formatter,
                    "capability pack has {actual} {field}; maximum is {maximum}"
                )
            }
            Self::DuplicateId { field, id } => write!(formatter, "duplicate {field}: {id}"),
            Self::DuplicateValue(field) => write!(formatter, "duplicate value in {field}"),
            Self::ShellEntrypoint(name) => write!(formatter, "invalid shell entrypoint: {name}"),
            Self::UnknownProfile(name) => write!(formatter, "unknown profile referenced: {name}"),
            Self::PackDigestMismatch => write!(formatter, "pack digest mismatch"),
        }
    }
}

impl std::error::Error for CapabilityPackError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Receipt(error) => Some(error),
            Self::Config(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ReceiptError> for CapabilityPackError {
    fn from(error: ReceiptError) -> Self {
        Self::Receipt(error)
    }
}

impl From<ConfigError> for CapabilityPackError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

fn validate_identifier(field: &'static str, value: &str) -> Result<(), CapabilityPackError> {
    crate::config::validate_identifier(field, value)
        .map_err(|_| CapabilityPackError::InvalidField(field))
}

fn validate_text(field: &'static str, value: &str) -> Result<(), CapabilityPackError> {
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        Err(CapabilityPackError::InvalidField(field))
    } else {
        Ok(())
    }
}

fn validate_license(field: &'static str, value: &str) -> Result<(), CapabilityPackError> {
    if value.is_empty()
        || value.len() > MAX_LICENSE_BYTES
        || matches!(value, "NOASSERTION" | "NONE")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
    {
        Err(CapabilityPackError::InvalidField(field))
    } else {
        Ok(())
    }
}

fn validate_semver(value: &str) -> Result<(), CapabilityPackError> {
    let components: Vec<_> = value.split('.').collect();
    if components.len() != 3
        || components.iter().any(|component| {
            component.is_empty()
                || !component.bytes().all(|byte| byte.is_ascii_digit())
                || (component.len() > 1 && component.starts_with('0'))
                || component.parse::<u64>().is_err()
        })
    {
        Err(CapabilityPackError::InvalidField("pack_version"))
    } else {
        Ok(())
    }
}

fn validate_digest(field: &'static str, value: &str) -> Result<(), CapabilityPackError> {
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Err(CapabilityPackError::InvalidField(field))
    } else {
        Ok(())
    }
}

fn validate_url(field: &'static str, value: &str) -> Result<(), CapabilityPackError> {
    let authority = value
        .strip_prefix("https://")
        .and_then(|suffix| suffix.split('/').next());
    if value.len() > 4096
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
        || value.contains('#')
        || authority.is_none_or(|authority| authority.is_empty() || authority.contains('@'))
    {
        Err(CapabilityPackError::InvalidField(field))
    } else {
        Ok(())
    }
}

fn validate_sources(
    sources: &[CapabilitySourceV1],
) -> Result<Vec<CapabilitySourceV1>, CapabilityPackError> {
    if sources.is_empty() {
        return Err(CapabilityPackError::InvalidField("upstream_sources"));
    }
    if sources.len() > MAX_PACK_SOURCES {
        return Err(CapabilityPackError::TooManyItems {
            field: "upstream_sources",
            actual: sources.len(),
            maximum: MAX_PACK_SOURCES,
        });
    }
    let mut by_id = BTreeMap::new();
    for source in sources {
        validate_identifier("upstream_sources.id", &source.id)?;
        validate_url("upstream_sources.url", &source.url)?;
        validate_digest("upstream_sources.digest", &source.digest)?;
        if by_id.insert(source.id.clone(), source.clone()).is_some() {
            return Err(CapabilityPackError::DuplicateId {
                field: "upstream_sources.id",
                id: source.id.clone(),
            });
        }
    }
    Ok(by_id.into_values().collect())
}

fn unique_sorted<T: Ord + Clone>(
    field: &'static str,
    values: &[T],
) -> Result<Vec<T>, CapabilityPackError> {
    let mut unique = BTreeSet::new();
    for value in values {
        if !unique.insert(value.clone()) {
            return Err(CapabilityPackError::DuplicateValue(field));
        }
    }
    Ok(unique.into_iter().collect())
}

fn validate_tools(
    tools: &[CapabilityToolV1],
) -> Result<Vec<CapabilityToolV1>, CapabilityPackError> {
    if tools.is_empty() {
        return Err(CapabilityPackError::InvalidField("profiles.tools"));
    }
    if tools.len() > MAX_PROFILE_TOOLS {
        return Err(CapabilityPackError::TooManyItems {
            field: "profiles.tools",
            actual: tools.len(),
            maximum: MAX_PROFILE_TOOLS,
        });
    }
    let mut by_id = BTreeMap::new();
    for tool in tools {
        validate_identifier("profiles.tools.id", &tool.id)?;
        validate_text("profiles.tools.version", &tool.version)?;
        validate_license("profiles.tools.license", &tool.license)?;
        validate_url("profiles.tools.url", &tool.url)?;
        validate_digest("profiles.tools.digest", &tool.digest)?;
        if by_id.insert(tool.id.clone(), tool.clone()).is_some() {
            return Err(CapabilityPackError::DuplicateId {
                field: "profiles.tools.id",
                id: tool.id.clone(),
            });
        }
    }
    Ok(by_id.into_values().collect())
}

fn validate_inputs(
    inputs: &[CapabilityInputProvenanceV1],
) -> Result<Vec<CapabilityInputProvenanceV1>, CapabilityPackError> {
    if inputs.len() > MAX_PROFILE_INPUTS {
        return Err(CapabilityPackError::TooManyItems {
            field: "profiles.inputs",
            actual: inputs.len(),
            maximum: MAX_PROFILE_INPUTS,
        });
    }
    let mut by_id = BTreeMap::new();
    for input in inputs {
        validate_identifier("profiles.inputs.id", &input.id)?;
        validate_url("profiles.inputs.url", &input.url)?;
        validate_digest("profiles.inputs.digest", &input.digest)?;
        let database = matches!(
            input.kind,
            CapabilityInputKindV1::AdvisoryDatabase | CapabilityInputKindV1::VulnerabilityDatabase
        );
        if database {
            let (created, max_age_seconds) =
                match (&input.snapshot_created_at_utc, input.max_age_seconds) {
                    (Some(created), Some(max_age_seconds)) => (created, max_age_seconds),
                    _ => {
                        return Err(CapabilityPackError::InvalidField(
                            "profiles.inputs.freshness",
                        ));
                    }
                };
            if crate::verify::parse_utc_seconds(created).is_none() {
                return Err(CapabilityPackError::InvalidField(
                    "profiles.inputs.snapshot_created_at_utc",
                ));
            }
            if !(1..=31_536_000).contains(&max_age_seconds) {
                return Err(CapabilityPackError::InvalidField(
                    "profiles.inputs.max_age_seconds",
                ));
            }
        } else if input.snapshot_created_at_utc.is_some() || input.max_age_seconds.is_some() {
            return Err(CapabilityPackError::InvalidField(
                "profiles.inputs.freshness",
            ));
        }
        if by_id.insert(input.id.clone(), input.clone()).is_some() {
            return Err(CapabilityPackError::DuplicateId {
                field: "profiles.inputs.id",
                id: input.id.clone(),
            });
        }
    }
    Ok(by_id.into_values().collect())
}

fn shell_entrypoint(argv: &[String]) -> bool {
    const SHELLS: &[&str] = &[
        "sh",
        "bash",
        "dash",
        "zsh",
        "ksh",
        "fish",
        "csh",
        "tcsh",
        "cmd",
        "cmd.exe",
        "powershell",
        "powershell.exe",
        "pwsh",
        "pwsh.exe",
    ];
    let Some(entrypoint) = argv.first() else {
        return false;
    };
    let basename = entrypoint
        .rsplit('/')
        .next()
        .unwrap_or(entrypoint)
        .to_ascii_lowercase();
    if SHELLS.contains(&basename.as_str()) {
        return true;
    }
    if !matches!(basename.as_str(), "env") {
        return false;
    }
    argv.iter()
        .skip(1)
        .find(|argument| !argument.starts_with('-'))
        .is_some_and(|argument| {
            let name = argument
                .rsplit('/')
                .next()
                .unwrap_or(argument)
                .to_ascii_lowercase();
            SHELLS.contains(&name.as_str())
        })
}

fn validate_profile(
    p: CapabilityProfileConfigV1,
) -> Result<(NormalizedCapabilityProfileV1, ValidatedProfileConfigV1), CapabilityPackError> {
    validate_identifier("profiles.id", &p.id)?;
    validate_text("profiles.description", &p.description)?;
    validate_text("profiles.pass_semantics", &p.pass_semantics)?;
    if p.supported_hosts.is_empty() {
        return Err(CapabilityPackError::InvalidField(
            "profiles.supported_hosts",
        ));
    }
    if p.supported_hosts.len() > MAX_PROFILE_HOSTS {
        return Err(CapabilityPackError::TooManyItems {
            field: "profiles.supported_hosts",
            actual: p.supported_hosts.len(),
            maximum: MAX_PROFILE_HOSTS,
        });
    }
    if p.target_platforms.is_empty() {
        return Err(CapabilityPackError::InvalidField(
            "profiles.target_platforms",
        ));
    }
    if p.target_platforms.len() > MAX_PROFILE_TARGETS {
        return Err(CapabilityPackError::TooManyItems {
            field: "profiles.target_platforms",
            actual: p.target_platforms.len(),
            maximum: MAX_PROFILE_TARGETS,
        });
    }
    if p.required_runtime_features.is_empty() {
        return Err(CapabilityPackError::InvalidField(
            "profiles.required_runtime_features",
        ));
    }
    if p.required_runtime_features.len() > MAX_PROFILE_FEATURES {
        return Err(CapabilityPackError::TooManyItems {
            field: "profiles.required_runtime_features",
            actual: p.required_runtime_features.len(),
            maximum: MAX_PROFILE_FEATURES,
        });
    }
    if p.known_blind_spots.is_empty() {
        return Err(CapabilityPackError::InvalidField(
            "profiles.known_blind_spots",
        ));
    }
    if p.known_blind_spots.len() > MAX_BLIND_SPOTS {
        return Err(CapabilityPackError::TooManyItems {
            field: "profiles.known_blind_spots",
            actual: p.known_blind_spots.len(),
            maximum: MAX_BLIND_SPOTS,
        });
    }

    let supported_hosts = unique_sorted("profiles.supported_hosts", &p.supported_hosts)?;
    let target_platforms = unique_sorted("profiles.target_platforms", &p.target_platforms)?;
    let required_runtime_features = unique_sorted(
        "profiles.required_runtime_features",
        &p.required_runtime_features,
    )?;
    let known_blind_spots = {
        for blind_spot in &p.known_blind_spots {
            validate_text("profiles.known_blind_spots", blind_spot)?;
        }
        unique_sorted("profiles.known_blind_spots", &p.known_blind_spots)?
    };
    for required in [
        CapabilityRuntimeFeatureV1::NoNetwork,
        CapabilityRuntimeFeatureV1::ReadOnlySource,
        CapabilityRuntimeFeatureV1::LinuxUserland,
    ] {
        if !required_runtime_features.contains(&required) {
            return Err(CapabilityPackError::InvalidField(
                "profiles.required_runtime_features",
            ));
        }
    }
    let artifacts_declared = p.checks.iter().any(|check| !check.artifacts.is_empty());
    if required_runtime_features.contains(&CapabilityRuntimeFeatureV1::WritableCaches)
        == p.caches.is_empty()
        || required_runtime_features.contains(&CapabilityRuntimeFeatureV1::BoundedArtifacts)
            != artifacts_declared
    {
        return Err(CapabilityPackError::InvalidField(
            "profiles.required_runtime_features",
        ));
    }
    if p.runtime.kind != RuntimeKind::DockerCompatible {
        return Err(CapabilityPackError::InvalidField("profiles.runtime.kind"));
    }
    if p.runtime.network {
        return Err(CapabilityPackError::InvalidField(
            "profiles.runtime.network",
        ));
    }
    for check in &p.checks {
        if shell_entrypoint(&check.argv) {
            return Err(CapabilityPackError::ShellEntrypoint(check.id.clone()));
        }
    }

    let tools = validate_tools(&p.tools)?;
    let inputs = validate_inputs(&p.inputs)?;
    let raw = ValidatedProfileConfigV1 {
        evidence_class: p.evidence_class,
        runtime: p.runtime.clone(),
        environment: p.environment.clone(),
        caches: p.caches.clone(),
        storage: p.storage.clone(),
        checks: p.checks.clone(),
    };
    let plan = ConfigV1 {
        schema_version: "1.3".to_owned(),
        project: "capability-pack/validation".to_owned(),
        runtime: raw.runtime.clone(),
        receipt: ReceiptConfig {
            output: ".ccp/capability-pack-validation.json".to_owned(),
            freshness_seconds: 86_400,
        },
        environment: raw.environment.clone(),
        caches: raw.caches.clone(),
        storage: Some(raw.storage.clone()),
        checks: raw.checks.clone(),
    }
    .into_plan()?;
    let storage = plan
        .plan
        .storage
        .clone()
        .ok_or(CapabilityPackError::InvalidField("profiles.storage"))?;
    Ok((
        NormalizedCapabilityProfileV1 {
            id: p.id,
            description: p.description,
            evidence_class: p.evidence_class,
            pass_semantics: p.pass_semantics,
            known_blind_spots,
            supported_hosts,
            target_platforms,
            required_runtime_features,
            offline_preparation: p.offline_preparation,
            tools,
            inputs,
            runtime: plan.plan.runtime,
            environment: plan.plan.environment,
            caches: plan.plan.caches,
            storage,
            checks: plan.plan.checks,
        },
        raw,
    ))
}

const _: () = {
    let _ = MAX_PACK_PROFILES;
    let _ = MAX_PACK_SOURCES;
    let _ = MAX_PROFILE_TOOLS;
    let _ = MAX_PROFILE_INPUTS;
    let _ = MAX_PROFILE_HOSTS;
    let _ = MAX_PROFILE_TARGETS;
    let _ = MAX_PROFILE_FEATURES;
    let _ = MAX_BLIND_SPOTS;
    let _ = MAX_LICENSE_BYTES;
};
