//! Pure Matrix V2 configuration and plan contracts.
//!
//! This module intentionally contains no execution, runtime, cache, or Docker
//! dependencies.  It is the protocol nucleus consumed by independent tools.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::canonical::{canonical_digest, canonical_json};
use crate::config::{
    CacheConfig, CheckConfig, ConfigError, ConfigV1, EnvironmentConfig, ExecutionPlanEnvelopeV1,
    ExecutionPlanV1, NormalizedCache, NormalizedCheck, NormalizedEnvironment, NormalizedReceipt,
    NormalizedRuntime, ReceiptConfig, RuntimeConfig, RuntimeKind, validate_identifier,
};
use crate::errors::{EvidenceStatus, ReceiptError};
use crate::verification_model::{
    AcceptedPlatformV1, VerificationDecision, VerificationReportV1, VerificationStatus, finding,
    parse_utc_seconds, validate_commit,
};

pub const MATRIX_RECEIPT_SCHEMA_VERSION: &str = "2.0";
pub const MATRIX_POLICY_SCHEMA_VERSION: &str = "2.0";

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
    #[serde(skip)]
    profile: MatrixPlanProfile,
    #[serde(skip)]
    legacy_basis: Option<crate::matrix_legacy::LegacyMatrixDigestBasisV1>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MatrixPlanProfile {
    #[default]
    CurrentV2,
    LegacyV1,
}

impl MatrixPlanProfile {
    pub const fn producer_version(self) -> &'static str {
        match self {
            Self::CurrentV2 => env!("CARGO_PKG_VERSION"),
            Self::LegacyV1 => concat!(env!("CARGO_PKG_VERSION"), "+matrix-v2-legacy-v1"),
        }
    }
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
    build_matrix_plan_with_profile(config, MatrixPlanProfile::CurrentV2)
}

pub fn build_matrix_plan_with_profile(
    config: MatrixConfigV2,
    profile: MatrixPlanProfile,
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
    let legacy_basis = if profile == MatrixPlanProfile::LegacyV1 {
        let basis = crate::matrix_legacy::project_legacy_basis(&plan)?;
        let mut plan = plan;
        for runtime in &mut plan.runtimes {
            runtime.configuration_digest = basis.runtime_digest(&runtime.id)?.to_owned();
        }
        let digest = basis.outer_digest()?;
        return Ok(MatrixPlanEnvelopeV2 {
            plan_digest: digest,
            plan,
            profile,
            legacy_basis: Some(basis),
        });
    } else {
        None
    };
    let digest = canonical_digest(&plan).map_err(MatrixContractError::Receipt)?;
    Ok(MatrixPlanEnvelopeV2 {
        plan_digest: digest,
        plan,
        profile,
        legacy_basis,
    })
}
impl MatrixPlanEnvelopeV2 {
    pub fn profile(&self) -> MatrixPlanProfile {
        self.profile
    }
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

    /// Project each runtime into the v1 execution envelope consumed by the
    /// runner.  This is intentionally a pure operation: the independent
    /// verifier can expose the same plan contract without linking execution.
    pub fn runtime_envelopes(
        &self,
    ) -> Result<Vec<(String, ExecutionPlanEnvelopeV1)>, MatrixContractError> {
        self.validate_profile_binding()?;
        self.plan
            .runtimes
            .iter()
            .map(|runtime| {
                let plan = ExecutionPlanV1 {
                    schema_version: "1.0".to_owned(),
                    project: self.plan.project.clone(),
                    runtime: runtime.runtime.clone(),
                    receipt: self.plan.receipt.clone(),
                    environment: self.plan.environment.clone(),
                    caches: self.plan.caches.clone(),
                    storage: None,
                    checks: runtime.checks.clone(),
                };
                let plan_digest = match self.profile {
                    MatrixPlanProfile::CurrentV2 => {
                        canonical_digest(&plan).map_err(MatrixContractError::Receipt)?
                    }
                    MatrixPlanProfile::LegacyV1 => {
                        self.runtime_configuration_digest(&runtime.id)?.to_owned()
                    }
                };
                if self.profile == MatrixPlanProfile::CurrentV2
                    && plan_digest != runtime.configuration_digest
                {
                    return Err(MatrixContractError::PlanDigestMismatch);
                }
                Ok((
                    runtime.id.clone(),
                    ExecutionPlanEnvelopeV1 {
                        plan_digest,
                        plan,
                        fixed_environment: BTreeMap::new(),
                    },
                ))
            })
            .collect()
    }

    pub fn legacy_digest_basis_value(
        &self,
    ) -> Result<Option<serde_json::Value>, MatrixContractError> {
        self.legacy_basis
            .as_ref()
            .map(|basis| basis.report_value())
            .transpose()
    }

    pub fn validate_profile_binding(&self) -> Result<(), MatrixContractError> {
        match self.profile {
            MatrixPlanProfile::CurrentV2 => self.validate(),
            MatrixPlanProfile::LegacyV1 => {
                let basis = self
                    .legacy_basis
                    .as_ref()
                    .ok_or(MatrixContractError::PlanDigestMismatch)?;
                let projected = crate::matrix_legacy::project_legacy_basis(&self.plan)
                    .map_err(MatrixContractError::from)?;
                if &projected != basis || projected.outer_digest()? != self.plan_digest {
                    return Err(MatrixContractError::PlanDigestMismatch);
                }
                for runtime in &self.plan.runtimes {
                    if basis.runtime_digest(&runtime.id)? != runtime.configuration_digest {
                        return Err(MatrixContractError::PlanDigestMismatch);
                    }
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug)]
pub enum MatrixContractError {
    Io(std::io::Error),
    Json(serde_json::Error),
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
    ReceiptIdMismatch,
    Verification(crate::errors::VerificationError),
    InvalidEvaluationTime,
}
impl fmt::Display for MatrixContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "matrix I/O failed: {e}"),
            Self::Json(e) => write!(f, "matrix JSON failed: {e}"),
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
            Self::ReceiptIdMismatch => f.write_str("matrix receipt ID mismatch"),
            Self::Verification(e) => write!(f, "matrix verification failed: {e}"),
            Self::InvalidEvaluationTime => {
                f.write_str("verification time is not representable as strict UTC")
            }
        }
    }
}
impl std::error::Error for MatrixContractError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            Self::Parse(e) => Some(e),
            Self::Config(e) => Some(e),
            Self::Receipt(e) => Some(e),
            Self::Verification(e) => Some(e),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatrixReceiptEnvelopeV2 {
    pub receipt_id: String,
    pub receipt: MatrixReceiptV2,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatrixReceiptV2 {
    pub schema_version: String,
    pub producer: crate::receipt::ProducerEvidence,
    pub repository: crate::receipt::RepositoryEvidence,
    pub run: crate::receipt::RunEvidence,
    pub configuration_digest: String,
    pub runtime_receipts: Vec<MatrixRuntimeReceiptV2>,
    pub overall_status: EvidenceStatus,
    pub incomplete_reason: Option<String>,
    pub redaction_policy_version: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatrixRuntimeReceiptV2 {
    pub runtime_id: String,
    pub receipt: crate::receipt::ReceiptEnvelopeV1,
}

impl MatrixReceiptEnvelopeV2 {
    pub fn seal(receipt: MatrixReceiptV2) -> Result<Self, MatrixContractError> {
        receipt.validate()?;
        let receipt_id = canonical_digest(&receipt).map_err(MatrixContractError::Receipt)?;
        Ok(Self {
            receipt_id,
            receipt,
        })
    }
    pub fn verify(&self) -> Result<(), MatrixContractError> {
        self.receipt.validate()?;
        let expected = canonical_digest(&self.receipt).map_err(MatrixContractError::Receipt)?;
        if expected != self.receipt_id {
            return Err(MatrixContractError::ReceiptIdMismatch);
        }
        Ok(())
    }
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, MatrixContractError> {
        self.verify()?;
        canonical_json(self).map_err(MatrixContractError::Receipt)
    }
}
impl MatrixReceiptV2 {
    pub fn validate(&self) -> Result<(), MatrixContractError> {
        if self.schema_version != MATRIX_RECEIPT_SCHEMA_VERSION {
            return Err(MatrixContractError::UnsupportedSchemaVersion(
                self.schema_version.clone(),
            ));
        }
        if self.repository.dirty || self.runtime_receipts.len() < 2 {
            return Err(MatrixContractError::InvalidReceipt);
        }
        let mut runtime_ids = BTreeSet::new();
        let mut check_ids = BTreeSet::new();
        let mut checks = Vec::new();
        for group in &self.runtime_receipts {
            validate_identifier("runtime_receipts.runtime_id", &group.runtime_id)
                .map_err(MatrixContractError::Config)?;
            if !runtime_ids.insert(group.runtime_id.as_str()) {
                return Err(MatrixContractError::DuplicateValue(
                    "runtime_receipts.runtime_id",
                ));
            }
            group
                .receipt
                .verify()
                .map_err(MatrixContractError::Receipt)?;
            let receipt = &group.receipt.receipt;
            if receipt.repository != self.repository || receipt.producer != self.producer {
                return Err(MatrixContractError::InvalidReceipt);
            }
            for check in &receipt.checks {
                if !check_ids.insert(check.id.as_str()) {
                    return Err(MatrixContractError::DuplicateValue(
                        "runtime_receipts.checks.id",
                    ));
                }
                checks.push(check.clone());
            }
        }
        if checks.is_empty() {
            return Err(MatrixContractError::InvalidReceipt);
        }
        crate::receipt::ReceiptV1 {
            schema_version: crate::receipt::RECEIPT_SCHEMA_VERSION.to_owned(),
            producer: self.producer.clone(),
            repository: self.repository.clone(),
            run: self.run.clone(),
            platform: self.runtime_receipts[0].receipt.receipt.platform.clone(),
            configuration_digest: self.configuration_digest.clone(),
            checks,
            overall_status: self.overall_status,
            incomplete_reason: self.incomplete_reason.clone(),
            redaction_policy_version: self.redaction_policy_version.clone(),
        }
        .validate()
        .map_err(MatrixContractError::Receipt)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatrixVerificationPolicyV2 {
    pub schema_version: String,
    pub project: String,
    pub configuration_digest: String,
    pub required_checks: Vec<MatrixRequiredCheckV2>,
    pub max_age_seconds: u64,
    pub runtimes: Vec<MatrixRuntimePolicyV2>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatrixRequiredCheckV2 {
    pub id: String,
    pub runtime_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatrixRuntimePolicyV2 {
    pub id: String,
    pub configuration_digest: String,
    pub image_reference: String,
    pub platforms: Vec<AcceptedPlatformV1>,
}

impl MatrixVerificationPolicyV2 {
    pub fn parse(source: &str) -> Result<Self, MatrixContractError> {
        if source.len() > MAX_MATRIX_BYTES {
            return Err(MatrixContractError::ConfigTooLarge);
        }
        let value: Self = toml::from_str(source).map_err(MatrixContractError::Parse)?;
        value.validate()?;
        Ok(value)
    }
    pub fn load(path: &Path) -> Result<Self, MatrixContractError> {
        Self::parse(&fs::read_to_string(path).map_err(MatrixContractError::Io)?)
    }
    pub fn validate(&self) -> Result<(), MatrixContractError> {
        if self.schema_version != MATRIX_POLICY_SCHEMA_VERSION {
            return Err(MatrixContractError::UnsupportedSchemaVersion(
                self.schema_version.clone(),
            ));
        }
        if !(2..=MAX_MATRIX_RUNTIMES).contains(&self.runtimes.len())
            || self.required_checks.is_empty()
        {
            return Err(MatrixContractError::InvalidField(
                "runtimes_or_required_checks",
            ));
        }
        let mut ids = BTreeSet::new();
        for r in &self.runtimes {
            validate_identifier("runtimes.id", &r.id).map_err(MatrixContractError::Config)?;
            if !ids.insert(r.id.as_str()) {
                return Err(MatrixContractError::DuplicateValue("runtimes.id"));
            }
        }
        let mut checks = BTreeSet::new();
        for c in &self.required_checks {
            validate_identifier("required_checks.id", &c.id)
                .map_err(MatrixContractError::Config)?;
            validate_identifier("required_checks.runtime_id", &c.runtime_id)
                .map_err(MatrixContractError::Config)?;
            if !ids.contains(c.runtime_id.as_str()) {
                return Err(MatrixContractError::UnknownRuntime(c.runtime_id.clone()));
            }
            if !checks.insert(c.id.as_str()) {
                return Err(MatrixContractError::DuplicateValue("required_checks.id"));
            }
        }
        Ok(())
    }
}

pub fn verify_matrix_receipt_document(
    bytes: &[u8],
    policy: &MatrixVerificationPolicyV2,
    expected_commit: &str,
    evaluated_at_utc: &str,
) -> Result<VerificationReportV1, MatrixContractError> {
    policy.validate()?;
    validate_commit(expected_commit).map_err(MatrixContractError::Verification)?;
    if parse_utc_seconds(evaluated_at_utc).is_none() {
        return Err(MatrixContractError::InvalidEvaluationTime);
    }
    let mut report = VerificationReportV1 {
        schema_version: "1.0".into(),
        assurance_scope: "integrity_and_repository_policy_only".into(),
        evaluated_at_utc: evaluated_at_utc.into(),
        expected_commit: expected_commit.into(),
        receipt_id: None,
        integrity_status: VerificationStatus::Fail,
        policy_status: VerificationStatus::NotRun,
        decision: VerificationDecision::Fail,
        findings: Vec::new(),
    };
    let envelope: MatrixReceiptEnvelopeV2 = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(_) => {
            report.findings.push(finding(
                "receipt.parse_or_shape",
                "receipt",
                "receipt is not valid strict schema v2 JSON",
            ));
            return Ok(report);
        }
    };
    if envelope.verify().is_err() {
        report.findings.push(finding(
            "receipt.semantic_or_digest_invalid",
            "receipt",
            "receipt violates v2 integrity invariants",
        ));
        return Ok(report);
    }
    report.receipt_id = Some(envelope.receipt_id.clone());
    report.integrity_status = VerificationStatus::Pass;
    report.policy_status = VerificationStatus::Pass;
    let r = &envelope.receipt;
    if r.repository.repository != policy.project {
        report.findings.push(finding(
            "policy.repository",
            "repository.repository",
            "receipt project does not match repository policy",
        ));
    }
    if r.repository.commit_sha != expected_commit {
        report.findings.push(finding(
            "policy.commit",
            "repository.commit_sha",
            "receipt commit does not match externally supplied commit",
        ));
    }
    if r.configuration_digest != policy.configuration_digest {
        report.findings.push(finding(
            "policy.configuration",
            "configuration_digest",
            "receipt configuration digest does not match repository policy",
        ));
    }
    if r.overall_status != EvidenceStatus::Pass {
        report.findings.push(finding(
            "policy.overall_status",
            "overall_status",
            "repository policy requires an overall PASS receipt",
        ));
    }
    if report.findings.is_empty() {
        report.decision = VerificationDecision::Pass;
    } else {
        report.policy_status = VerificationStatus::Fail;
    }
    Ok(report)
}
