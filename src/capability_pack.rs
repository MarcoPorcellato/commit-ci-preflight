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

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::{
    CacheConfig, CheckConfig, ConfigError, EnvironmentConfig, RuntimeConfig, StorageConfig,
};
use crate::receipt::ReceiptError;

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
        Err(CapabilityPackError::InvalidField("profiles"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityPackEnvelopeV1 {
    pub pack_version: String,
    pub manifest: CapabilityPackManifestV1,
    #[allow(dead_code)]
    pub manifest_id: String,
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
