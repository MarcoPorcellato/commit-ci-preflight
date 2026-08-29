//! Pure Matrix V2 configuration and plan contracts.
//!
//! This module intentionally contains no execution, runtime, cache, or Docker
//! dependencies.  It is the protocol nucleus consumed by independent tools.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::canonical::{canonical_digest, canonical_json};
use crate::config::{
    CacheConfig, CheckConfig, ConfigError, ConfigV1, EnvironmentConfig, NormalizedCache,
    NormalizedCheck, NormalizedEnvironment, NormalizedReceipt, NormalizedRuntime, ReceiptConfig,
    RuntimeConfig, RuntimeKind, validate_identifier,
};
use crate::errors::ReceiptError;

pub const MATRIX_CONFIG_SCHEMA_VERSION: &str = "2.0";
const MAX_MATRIX_RUNTIMES: usize = 32;
const MAX_MATRIX_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatrixConfigV2 {
    pub schema_version: String,
    pub project: String,
    pub runtimes: Vec<MatrixRuntimeConfigV2>,
    #[serde(default)]
    pub receipt: ReceiptConfig,
    #[serde(default)]
    pub environment: MatrixEnvironmentConfigV2,
    #[serde(default)]
    pub caches: Vec<CacheConfig>,
    pub checks: Vec<MatrixCheckConfigV2>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct MatrixEnvironmentConfigV2 {
    pub allow: Vec<String>,
}

impl MatrixEnvironmentConfigV2 {
    fn as_v1(&self) -> EnvironmentConfig {
        EnvironmentConfig {
            allow: self.allow.clone(),
            ..EnvironmentConfig::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatrixRuntimeConfigV2 {
    pub id: String,
    pub kind: RuntimeKind,
    pub image: String,
    pub cpu_count: u16,
    pub memory_mib: u64,
    pub pids_limit: u32,
    #[serde(default)]
    pub network: bool,
}
impl MatrixRuntimeConfigV2 {
    fn as_runtime(&self) -> RuntimeConfig {
        RuntimeConfig {
            kind: self.kind,
            image: self.image.clone(),
            cpu_count: self.cpu_count,
            memory_mib: self.memory_mib,
            pids_limit: self.pids_limit,
            network: self.network,
            pull_policy: None,
            swap_mode: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatrixCheckConfigV2 {
    pub id: String,
    pub runtime_id: String,
    pub required: bool,
    pub argv: Vec<String>,
    pub working_directory: String,
    pub timeout_seconds: u64,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
}
impl MatrixCheckConfigV2 {
    fn as_v1(&self) -> CheckConfig {
        CheckConfig {
            id: self.id.clone(),
            required: self.required,
            argv: self.argv.clone(),
            working_directory: self.working_directory.clone(),
            timeout_seconds: self.timeout_seconds,
            depends_on: self.depends_on.clone(),
            artifacts: self.artifacts.clone(),
            artifact_contracts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MatrixPlanEnvelopeV2 {
    pub plan_digest: String,
    pub plan: MatrixPlanV2,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MatrixPlanV2 {
    pub schema_version: String,
    pub project: String,
    pub receipt: NormalizedReceipt,
    pub environment: NormalizedEnvironment,
    pub caches: Vec<NormalizedCache>,
    pub runtimes: Vec<MatrixRuntimePlanV2>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MatrixRuntimePlanV2 {
    pub id: String,
    pub configuration_digest: String,
    pub runtime: NormalizedRuntime,
    pub checks: Vec<NormalizedCheck>,
}

impl MatrixConfigV2 {
    pub fn parse(input: &str) -> Result<Self, MatrixContractError> {
        if input.len() > MAX_MATRIX_BYTES {
            return Err(MatrixContractError::ConfigTooLarge);
        };
        toml::from_str(input).map_err(MatrixContractError::Parse)
    }
    pub fn load(path: &Path) -> Result<Self, MatrixContractError> {
        let metadata = fs::metadata(path).map_err(MatrixContractError::Io)?;
        if metadata.len() > MAX_MATRIX_BYTES as u64 {
            return Err(MatrixContractError::ConfigTooLarge);
        };
        Self::parse(&fs::read_to_string(path).map_err(MatrixContractError::Io)?)
    }
    pub fn into_plan(self) -> Result<MatrixPlanEnvelopeV2, MatrixContractError> {
        build_matrix_plan(self)
    }
}
pub fn build_matrix_plan(
    config: MatrixConfigV2,
) -> Result<MatrixPlanEnvelopeV2, MatrixContractError> {
    if config.schema_version != MATRIX_CONFIG_SCHEMA_VERSION {
        return Err(MatrixContractError::UnsupportedSchemaVersion(
            config.schema_version,
        ));
    }
    if !(2..=MAX_MATRIX_RUNTIMES).contains(&config.runtimes.len()) {
        return Err(MatrixContractError::InvalidField("runtimes"));
    }
    let mut runtimes = BTreeMap::new();
    for runtime in config.runtimes {
        validate_identifier("runtimes.id", &runtime.id).map_err(MatrixContractError::Config)?;
        if runtimes.insert(runtime.id.clone(), runtime).is_some() {
            return Err(MatrixContractError::DuplicateValue("runtimes.id"));
        }
    }
    let mut ids = BTreeMap::new();
    let mut grouped: BTreeMap<String, Vec<CheckConfig>> = BTreeMap::new();
    for check in &config.checks {
        validate_identifier("checks.runtime_id", &check.runtime_id)
            .map_err(MatrixContractError::Config)?;
        if !runtimes.contains_key(&check.runtime_id) {
            return Err(MatrixContractError::UnknownRuntime(
                check.runtime_id.clone(),
            ));
        }
        if ids
            .insert(check.id.clone(), check.runtime_id.clone())
            .is_some()
        {
            return Err(MatrixContractError::DuplicateValue("checks.id"));
        }
        grouped
            .entry(check.runtime_id.clone())
            .or_default()
            .push(check.as_v1());
    }
    for check in &config.checks {
        for dependency in &check.depends_on {
            if let Some(runtime) = ids.get(dependency)
                && runtime != &check.runtime_id
            {
                return Err(MatrixContractError::CrossRuntimeDependency {
                    check: check.id.clone(),
                    dependency: dependency.clone(),
                });
            }
        }
    }
    let mut plans = Vec::new();
    let mut shared = None;
    for (id, runtime) in runtimes {
        let checks = grouped.remove(&id).unwrap_or_default();
        if !checks.iter().any(|c| c.required) {
            return Err(MatrixContractError::RuntimeWithoutRequiredCheck(id));
        }
        let group = ConfigV1 {
            schema_version: "1.0".into(),
            project: config.project.clone(),
            runtime: runtime.as_runtime(),
            receipt: config.receipt.clone(),
            environment: config.environment.as_v1(),
            caches: config.caches.clone(),
            storage: None,
            checks,
        }
        .into_plan()
        .map_err(MatrixContractError::Config)?;
        if shared.is_none() {
            shared = Some((
                group.plan.receipt.clone(),
                group.plan.environment.clone(),
                group.plan.caches.clone(),
            ));
        }
        plans.push(MatrixRuntimePlanV2 {
            id,
            configuration_digest: group.plan_digest,
            runtime: group.plan.runtime,
            checks: group.plan.checks,
        });
    }
    let (receipt, environment, caches) = shared.ok_or(MatrixContractError::InvalidReceipt)?;
    let plan = MatrixPlanV2 {
        schema_version: MATRIX_CONFIG_SCHEMA_VERSION.into(),
        project: config.project,
        receipt,
        environment,
        caches,
        runtimes: plans,
    };
    let digest = canonical_digest(&plan).map_err(MatrixContractError::Receipt)?;
    Ok(MatrixPlanEnvelopeV2 {
        plan_digest: digest,
        plan,
    })
}
impl MatrixPlanEnvelopeV2 {
    pub fn plan_digest(&self) -> Result<&str, MatrixContractError> {
        self.validate()?;
        Ok(&self.plan_digest)
    }
    pub fn runtime_configuration_digest(&self, id: &str) -> Result<&str, MatrixContractError> {
        self.validate()?;
        self.plan
            .runtimes
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.configuration_digest.as_str())
            .ok_or_else(|| MatrixContractError::UnknownRuntime(id.into()))
    }
    pub fn validate(&self) -> Result<(), MatrixContractError> {
        if canonical_digest(&self.plan).map_err(MatrixContractError::Receipt)? != self.plan_digest {
            return Err(MatrixContractError::PlanDigestMismatch);
        }
        Ok(())
    }
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, MatrixContractError> {
        self.validate()?;
        canonical_json(self).map_err(MatrixContractError::Receipt)
    }
}

#[derive(Debug)]
pub enum MatrixContractError {
    Io(std::io::Error),
    Parse(toml::de::Error),
    Config(ConfigError),
    Receipt(ReceiptError),
    UnsupportedSchemaVersion(String),
    ConfigTooLarge,
    InvalidField(&'static str),
    DuplicateValue(&'static str),
    UnknownRuntime(String),
    RuntimeWithoutRequiredCheck(String),
    CrossRuntimeDependency { check: String, dependency: String },
    PlanDigestMismatch,
    InvalidReceipt,
}
impl fmt::Display for MatrixContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "matrix I/O failed: {e}"),
            Self::Parse(e) => write!(f, "matrix configuration parse failed: {e}"),
            Self::Config(e) => write!(f, "matrix configuration invalid: {e}"),
            Self::Receipt(e) => write!(f, "matrix receipt invalid: {e}"),
            Self::UnsupportedSchemaVersion(v) => {
                write!(f, "unsupported matrix schema version: {v}")
            }
            Self::ConfigTooLarge => {
                f.write_str("matrix configuration exceeds the bounded input size")
            }
            Self::InvalidField(v) => write!(f, "invalid matrix field: {v}"),
            Self::DuplicateValue(v) => write!(f, "duplicate matrix value: {v}"),
            Self::UnknownRuntime(v) => write!(f, "matrix check references unknown runtime: {v}"),
            Self::RuntimeWithoutRequiredCheck(v) => {
                write!(f, "matrix runtime has no required check: {v}")
            }
            Self::CrossRuntimeDependency { check, dependency } => write!(
                f,
                "matrix cross-runtime dependency is unsupported: {check} -> {dependency}"
            ),
            Self::PlanDigestMismatch => f.write_str("matrix plan digest mismatch"),
            Self::InvalidReceipt => f.write_str("matrix receipt violates semantic invariants"),
        }
    }
}
impl std::error::Error for MatrixContractError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Parse(e) => Some(e),
            Self::Config(e) => Some(e),
            Self::Receipt(e) => Some(e),
            _ => None,
        }
    }
}
