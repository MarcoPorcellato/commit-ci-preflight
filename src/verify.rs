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

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};

use crate::config::{ConfigError, ConfigV1, ExecutionPlanEnvelopeV1, ExecutionPlanV1};
use crate::receipt::{
    CheckEvidence, EvidenceStatus, PlatformEvidence, ReceiptEnvelopeV1, ReceiptEnvelopeV2,
    ReceiptError, RepositoryEvidence, RunEvidence, SourceSnapshotStrategy, canonical_json,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationPolicyDocument {
    V1(VerificationPolicyV1),
    V1_1(VerificationPolicyV1_1),
    V2(crate::matrix::MatrixVerificationPolicyV2),
}

#[derive(Debug)]
pub enum VerificationPolicyDocumentError {
    Io(io::Error),
    TooLarge,
    InvalidUtf8,
    Parse(toml::de::Error),
    UnsupportedSchemaVersion,
    V1(PolicyError),
    V1_1(PolicyError),
    V2(crate::matrix::MatrixError),
}

impl fmt::Display for VerificationPolicyDocumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(_) => formatter.write_str("cannot read verification policy"),
            Self::TooLarge => formatter.write_str("verification policy exceeds size limit"),
            Self::InvalidUtf8 => formatter.write_str("verification policy is not UTF-8"),
            Self::Parse(_) => formatter.write_str("verification policy is not valid TOML"),
            Self::UnsupportedSchemaVersion => {
                formatter.write_str("verification policy schema version is unsupported")
            }
            Self::V1(error) => write!(formatter, "{error}"),
            Self::V1_1(error) => write!(formatter, "{error}"),
            Self::V2(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for VerificationPolicyDocumentError {}

pub fn load_verification_policy_document(
    path: &Path,
) -> Result<VerificationPolicyDocument, VerificationPolicyDocumentError> {
    let metadata = fs::metadata(path).map_err(VerificationPolicyDocumentError::Io)?;
    if metadata.len() > MAX_POLICY_BYTES as u64 {
        return Err(VerificationPolicyDocumentError::TooLarge);
    }
    let bytes = fs::read(path).map_err(VerificationPolicyDocumentError::Io)?;
    let source =
        std::str::from_utf8(&bytes).map_err(|_| VerificationPolicyDocumentError::InvalidUtf8)?;
    let value: toml::Value =
        toml::from_str(source).map_err(VerificationPolicyDocumentError::Parse)?;
    let version = value.get("schema_version").and_then(toml::Value::as_str);
    match version {
        Some(POLICY_SCHEMA_VERSION) => VerificationPolicyV1::parse(&bytes)
            .map(VerificationPolicyDocument::V1)
            .map_err(VerificationPolicyDocumentError::V1),
        Some(TRUSTED_PLAN_POLICY_SCHEMA_VERSION) => VerificationPolicyV1_1::parse(&bytes)
            .map(VerificationPolicyDocument::V1_1)
            .map_err(VerificationPolicyDocumentError::V1_1),
        Some(crate::matrix::MATRIX_POLICY_SCHEMA_VERSION) => {
            crate::matrix::MatrixVerificationPolicyV2::parse(source)
                .map(VerificationPolicyDocument::V2)
                .map_err(VerificationPolicyDocumentError::V2)
        }
        _ => Err(VerificationPolicyDocumentError::UnsupportedSchemaVersion),
    }
}

pub const POLICY_SCHEMA_VERSION: &str = "1.0";
pub const TRUSTED_PLAN_POLICY_SCHEMA_VERSION: &str = "1.1";
pub const VERIFICATION_REPORT_SCHEMA_VERSION: &str = "1.0";
const MAX_POLICY_BYTES: usize = 1024 * 1024;
const MAX_RECEIPT_BYTES: usize = 4 * 1024 * 1024;
const MAX_REQUIRED_CHECKS: usize = 128;
const MAX_PLATFORMS: usize = 32;
const MAX_FRESHNESS_SECONDS: u64 = 31_536_000;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerificationPolicyV1 {
    pub schema_version: String,
    pub project: String,
    pub configuration_digest: String,
    pub required_checks: Vec<String>,
    pub image_reference: String,
    pub max_age_seconds: u64,
    pub platforms: Vec<AcceptedPlatformV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptedPlatformV1 {
    pub host_os: String,
    pub host_arch: String,
    pub runtime_kind: String,
}

impl VerificationPolicyV1 {
    pub fn load(path: &Path) -> Result<Self, PolicyError> {
        let metadata = fs::metadata(path).map_err(PolicyError::Io)?;
        if metadata.len() > MAX_POLICY_BYTES as u64 {
            return Err(PolicyError::TooLarge);
        }
        let bytes = fs::read(path).map_err(PolicyError::Io)?;
        Self::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, PolicyError> {
        if bytes.len() > MAX_POLICY_BYTES {
            return Err(PolicyError::TooLarge);
        }
        let source = std::str::from_utf8(bytes).map_err(|_| PolicyError::InvalidUtf8)?;
        let policy: Self = toml::from_str(source).map_err(PolicyError::Parse)?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), PolicyError> {
        if self.schema_version != POLICY_SCHEMA_VERSION {
            return Err(PolicyError::UnsupportedSchemaVersion);
        }
        validate_project(&self.project)?;
        validate_digest(&self.configuration_digest)?;
        validate_image_reference(&self.image_reference)?;
        if !(1..=MAX_FRESHNESS_SECONDS).contains(&self.max_age_seconds) {
            return Err(PolicyError::InvalidField("max_age_seconds"));
        }
        if self.required_checks.is_empty() || self.required_checks.len() > MAX_REQUIRED_CHECKS {
            return Err(PolicyError::InvalidField("required_checks"));
        }
        let mut checks = BTreeSet::new();
        for check in &self.required_checks {
            validate_name("required_checks", check)?;
            if !checks.insert(check) {
                return Err(PolicyError::DuplicateValue("required_checks"));
            }
        }
        if self.platforms.is_empty() || self.platforms.len() > MAX_PLATFORMS {
            return Err(PolicyError::InvalidField("platforms"));
        }
        let mut platforms = BTreeSet::new();
        for platform in &self.platforms {
            validate_name("platforms.host_os", &platform.host_os)?;
            validate_name("platforms.host_arch", &platform.host_arch)?;
            validate_name("platforms.runtime_kind", &platform.runtime_kind)?;
            if !platforms.insert((
                &platform.host_os,
                &platform.host_arch,
                &platform.runtime_kind,
            )) {
                return Err(PolicyError::DuplicateValue("platforms"));
            }
        }
        Ok(())
    }
}

/// Strict policy version that reconstructs the normalized execution plan from
/// the trusted checkout before accepting a v2 receipt.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerificationPolicyV1_1 {
    pub schema_version: String,
    pub project: String,
    pub configuration_digest: String,
    pub required_checks: Vec<String>,
    pub image_reference: String,
    pub max_age_seconds: u64,
    pub platforms: Vec<AcceptedPlatformV1>,
    pub trusted_config: String,
    pub source_snapshot_strategy: SourceSnapshotStrategy,
    pub supported_producers: Vec<ProducerContractV1_1>,
    #[serde(default)]
    pub revoked_producers: Vec<ProducerContractV1_1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProducerContractV1_1 {
    pub name: String,
    pub version: String,
}

impl VerificationPolicyV1_1 {
    pub fn parse(bytes: &[u8]) -> Result<Self, PolicyError> {
        if bytes.len() > MAX_POLICY_BYTES {
            return Err(PolicyError::TooLarge);
        }
        let source = std::str::from_utf8(bytes).map_err(|_| PolicyError::InvalidUtf8)?;
        let policy: Self = toml::from_str(source).map_err(PolicyError::Parse)?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), PolicyError> {
        if self.schema_version != TRUSTED_PLAN_POLICY_SCHEMA_VERSION {
            return Err(PolicyError::UnsupportedSchemaVersion);
        }
        self.baseline().validate()?;
        validate_trusted_config_path(&self.trusted_config)?;
        if self.supported_producers.is_empty() {
            return Err(PolicyError::InvalidField("supported_producers"));
        }
        let mut supported = BTreeSet::new();
        for producer in &self.supported_producers {
            validate_producer_contract(producer)?;
            if !supported.insert((&producer.name, &producer.version)) {
                return Err(PolicyError::DuplicateValue("supported_producers"));
            }
        }
        let mut revoked = BTreeSet::new();
        for producer in &self.revoked_producers {
            validate_producer_contract(producer)?;
            let key = (&producer.name, &producer.version);
            if !revoked.insert(key) {
                return Err(PolicyError::DuplicateValue("revoked_producers"));
            }
            if supported.contains(&key) {
                return Err(PolicyError::DuplicateValue("producer_contracts"));
            }
        }
        Ok(())
    }

    fn baseline(&self) -> VerificationPolicyV1 {
        VerificationPolicyV1 {
            schema_version: POLICY_SCHEMA_VERSION.to_owned(),
            project: self.project.clone(),
            configuration_digest: self.configuration_digest.clone(),
            required_checks: self.required_checks.clone(),
            image_reference: self.image_reference.clone(),
            max_age_seconds: self.max_age_seconds,
            platforms: self.platforms.clone(),
        }
    }

    fn load_trusted_plan(
        &self,
        policy_path: &Path,
    ) -> Result<ExecutionPlanEnvelopeV1, TrustedPlanError> {
        let parent = policy_path.parent().ok_or(TrustedPlanError::PolicyPath)?;
        let config_path = parent.join(&self.trusted_config);
        let metadata = fs::symlink_metadata(&config_path).map_err(TrustedPlanError::Io)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(TrustedPlanError::UnsafeConfigurationPath);
        }
        ConfigV1::load(&config_path)
            .and_then(ConfigV1::into_plan)
            .map_err(TrustedPlanError::Config)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VerificationStatus {
    Pass,
    Fail,
    NotRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VerificationDecision {
    Pass,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerificationFindingV1 {
    pub code: String,
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerificationReportV1 {
    pub schema_version: String,
    pub assurance_scope: String,
    pub evaluated_at_utc: String,
    pub expected_commit: String,
    pub receipt_id: Option<String>,
    pub integrity_status: VerificationStatus,
    pub policy_status: VerificationStatus,
    pub decision: VerificationDecision,
    pub findings: Vec<VerificationFindingV1>,
}

impl VerificationReportV1 {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, VerificationError> {
        canonical_json(self).map_err(VerificationError::Receipt)
    }

    pub fn exit_code(&self) -> i32 {
        match self.decision {
            VerificationDecision::Pass => 0,
            VerificationDecision::Fail => 3,
        }
    }
}

pub fn verify_receipt_document(
    bytes: &[u8],
    policy: &VerificationPolicyV1,
    expected_commit: &str,
    evaluated_at_utc: &str,
) -> Result<VerificationReportV1, VerificationError> {
    policy.validate().map_err(VerificationError::Policy)?;
    validate_commit(expected_commit)?;
    let evaluated_at =
        parse_utc_seconds(evaluated_at_utc).ok_or(VerificationError::InvalidEvaluationTime)?;
    let mut report = VerificationReportV1 {
        schema_version: VERIFICATION_REPORT_SCHEMA_VERSION.to_owned(),
        assurance_scope: "integrity_and_repository_policy_only".to_owned(),
        evaluated_at_utc: evaluated_at_utc.to_owned(),
        expected_commit: expected_commit.to_owned(),
        receipt_id: None,
        integrity_status: VerificationStatus::Fail,
        policy_status: VerificationStatus::NotRun,
        decision: VerificationDecision::Fail,
        findings: Vec::new(),
    };

    if bytes.len() > MAX_RECEIPT_BYTES {
        report.findings.push(finding(
            "receipt.too_large",
            "receipt",
            "receipt exceeds the bounded verification input size",
        ));
        return Ok(report);
    }
    let document = match serde_json::from_slice::<serde_json::Value>(bytes) {
        Ok(value) => value,
        Err(_) => {
            report.findings.push(finding(
                "receipt.parse_or_shape",
                "receipt",
                "receipt is not valid strict schema v1 or v2 JSON",
            ));
            return Ok(report);
        }
    };
    let Some(schema_version) = document
        .pointer("/receipt/schema_version")
        .and_then(serde_json::Value::as_str)
    else {
        report.findings.push(finding(
            "receipt.parse_or_shape",
            "receipt.schema_version",
            "receipt schema version is missing or invalid",
        ));
        return Ok(report);
    };
    let parsed = match schema_version {
        "1.0" => serde_json::from_slice::<ReceiptEnvelopeV1>(bytes).map(VerifiedReceipt::V1),
        "2.0" => serde_json::from_slice::<ReceiptEnvelopeV2>(bytes).map(VerifiedReceipt::V2),
        _ => {
            report.findings.push(finding(
                "receipt.unsupported_schema",
                "receipt.schema_version",
                "receipt schema version is unsupported",
            ));
            return Ok(report);
        }
    };
    let envelope = match parsed {
        Ok(envelope) => envelope,
        Err(_) => {
            report.findings.push(finding(
                "receipt.parse_or_shape",
                "receipt",
                "receipt is not valid strict schema v1 or v2 JSON",
            ));
            return Ok(report);
        }
    };
    if let Err(error) = envelope.verify() {
        let (code, message) = match error {
            ReceiptError::UnsupportedSchemaVersion(_) => (
                "receipt.unsupported_schema",
                "receipt schema version is unsupported",
            ),
            ReceiptError::DigestMismatch { .. } => (
                "receipt.digest_mismatch",
                "receipt payload does not match its integrity identifier",
            ),
            _ => (
                "receipt.semantic_invalid",
                "receipt violates schema semantic invariants",
            ),
        };
        report.findings.push(finding(code, "receipt", message));
        return Ok(report);
    }

    report.receipt_id = Some(envelope.receipt_id().to_owned());
    report.integrity_status = VerificationStatus::Pass;
    report.policy_status = VerificationStatus::Pass;
    evaluate_policy(
        envelope.view(),
        policy,
        expected_commit,
        evaluated_at,
        &mut report.findings,
    );
    if !report.findings.is_empty() {
        report.policy_status = VerificationStatus::Fail;
    }
    report.decision = if report.integrity_status == VerificationStatus::Pass
        && report.policy_status == VerificationStatus::Pass
    {
        VerificationDecision::Pass
    } else {
        VerificationDecision::Fail
    };
    Ok(report)
}

/// Verify a receipt against the strict policy version selected from trusted
/// policy bytes. V1 keeps its original parser and behaviour; V2 is an
/// explicitly opt-in multi-runtime contract.
pub fn verify_receipt_document_for_policy(
    bytes: &[u8],
    policy: &VerificationPolicyDocument,
    expected_commit: &str,
    evaluated_at_utc: &str,
) -> Result<VerificationReportV1, VerificationError> {
    match policy {
        VerificationPolicyDocument::V1(policy) => {
            verify_receipt_document(bytes, policy, expected_commit, evaluated_at_utc)
        }
        VerificationPolicyDocument::V1_1(_) => Err(VerificationError::TrustedPolicyPathRequired),
        VerificationPolicyDocument::V2(policy) => crate::matrix::verify_matrix_receipt_document(
            bytes,
            policy,
            expected_commit,
            evaluated_at_utc,
        )
        .map_err(|error| VerificationError::Matrix(error.to_string())),
    }
}

/// Verify a receipt against a policy loaded from a trusted checkout. Policy
/// version 1.1 resolves its configuration only relative to that policy file;
/// callers cannot substitute an arbitrary configuration path.
pub fn verify_receipt_document_for_policy_path(
    bytes: &[u8],
    policy_path: &Path,
    expected_commit: &str,
    evaluated_at_utc: &str,
) -> Result<VerificationReportV1, VerificationError> {
    let policy = load_verification_policy_document(policy_path)
        .map_err(|error| VerificationError::PolicyDocument(error.to_string()))?;
    match &policy {
        VerificationPolicyDocument::V1_1(policy) => {
            let trusted_plan = policy
                .load_trusted_plan(policy_path)
                .map_err(VerificationError::TrustedPlan)?;
            verify_trusted_plan_receipt_document(
                bytes,
                policy,
                &trusted_plan,
                expected_commit,
                evaluated_at_utc,
            )
        }
        _ => verify_receipt_document_for_policy(bytes, &policy, expected_commit, evaluated_at_utc),
    }
}

/// Validate every trusted input selected by a policy path before receipt I/O.
/// This prevents a missing or malformed receipt from bypassing policy v1.1's
/// trusted-configuration requirement.
pub fn validate_verification_policy_path(policy_path: &Path) -> Result<(), VerificationError> {
    let policy = load_verification_policy_document(policy_path)
        .map_err(|error| VerificationError::PolicyDocument(error.to_string()))?;
    if let VerificationPolicyDocument::V1_1(policy) = policy {
        policy
            .load_trusted_plan(policy_path)
            .map_err(VerificationError::TrustedPlan)?;
    }
    Ok(())
}

fn verify_trusted_plan_receipt_document(
    bytes: &[u8],
    policy: &VerificationPolicyV1_1,
    trusted_plan: &ExecutionPlanEnvelopeV1,
    expected_commit: &str,
    evaluated_at_utc: &str,
) -> Result<VerificationReportV1, VerificationError> {
    let mut report =
        verify_receipt_document(bytes, &policy.baseline(), expected_commit, evaluated_at_utc)?;
    report.assurance_scope = "integrity_and_trusted_plan_policy".to_owned();
    if report.integrity_status != VerificationStatus::Pass {
        return Ok(report);
    }
    if trusted_plan.plan_digest != policy.configuration_digest {
        report.findings.push(finding(
            "policy.trusted_config_digest",
            "trusted_config",
            "trusted configuration does not reconstruct the policy execution-plan digest",
        ));
    }
    let receipt = match serde_json::from_slice::<ReceiptEnvelopeV2>(bytes) {
        Ok(receipt) => receipt,
        Err(_) => {
            report.findings.push(finding(
                "policy.receipt_schema",
                "receipt.schema_version",
                "trusted-plan policy requires a strict receipt v2",
            ));
            finalize_trusted_plan_report(&mut report);
            return Ok(report);
        }
    };
    compare_execution_plan(
        &trusted_plan.plan,
        &receipt.receipt.execution_plan,
        &mut report.findings,
    )?;
    if receipt.receipt.source_snapshot.strategy != policy.source_snapshot_strategy {
        report.findings.push(finding(
            "policy.source_snapshot_strategy",
            "source_snapshot.strategy",
            "receipt source snapshot strategy is not accepted by trusted policy",
        ));
    }
    let producer = (
        &receipt.receipt.producer.name,
        &receipt.receipt.producer.version,
    );
    if policy
        .revoked_producers
        .iter()
        .any(|candidate| (&candidate.name, &candidate.version) == producer)
    {
        report.findings.push(finding(
            "policy.producer_revoked",
            "producer",
            "receipt producer is explicitly revoked by trusted policy",
        ));
    } else if !policy
        .supported_producers
        .iter()
        .any(|candidate| (&candidate.name, &candidate.version) == producer)
    {
        report.findings.push(finding(
            "policy.producer_unsupported",
            "producer",
            "receipt producer is not supported by trusted policy",
        ));
    }
    finalize_trusted_plan_report(&mut report);
    Ok(report)
}

fn finalize_trusted_plan_report(report: &mut VerificationReportV1) {
    if !report.findings.is_empty() {
        report.policy_status = VerificationStatus::Fail;
    }
    report.decision = if report.integrity_status == VerificationStatus::Pass
        && report.policy_status == VerificationStatus::Pass
    {
        VerificationDecision::Pass
    } else {
        VerificationDecision::Fail
    };
}

pub fn receipt_input_failure_report(
    expected_commit: &str,
    evaluated_at_utc: &str,
) -> Result<VerificationReportV1, VerificationError> {
    validate_commit(expected_commit)?;
    parse_utc_seconds(evaluated_at_utc).ok_or(VerificationError::InvalidEvaluationTime)?;
    Ok(VerificationReportV1 {
        schema_version: VERIFICATION_REPORT_SCHEMA_VERSION.to_owned(),
        assurance_scope: "integrity_and_repository_policy_only".to_owned(),
        evaluated_at_utc: evaluated_at_utc.to_owned(),
        expected_commit: expected_commit.to_owned(),
        receipt_id: None,
        integrity_status: VerificationStatus::Fail,
        policy_status: VerificationStatus::NotRun,
        decision: VerificationDecision::Fail,
        findings: vec![finding(
            "receipt.read_failed",
            "receipt",
            "receipt could not be read from the caller-supplied path",
        )],
    })
}

struct ReceiptPolicyView<'a> {
    repository: &'a RepositoryEvidence,
    run: &'a RunEvidence,
    platform: &'a PlatformEvidence,
    configuration_digest: &'a str,
    checks: &'a [CheckEvidence],
    overall_status: EvidenceStatus,
}

// Direct ownership keeps both versioned receipt validation paths uniform.
#[allow(clippy::large_enum_variant)]
enum VerifiedReceipt {
    V1(ReceiptEnvelopeV1),
    V2(ReceiptEnvelopeV2),
}

impl VerifiedReceipt {
    fn verify(&self) -> Result<(), ReceiptError> {
        match self {
            Self::V1(value) => value.verify(),
            Self::V2(value) => value.verify(),
        }
    }

    fn receipt_id(&self) -> &str {
        match self {
            Self::V1(value) => &value.receipt_id,
            Self::V2(value) => &value.receipt_id,
        }
    }

    fn view(&self) -> ReceiptPolicyView<'_> {
        match self {
            Self::V1(value) => ReceiptPolicyView {
                repository: &value.receipt.repository,
                run: &value.receipt.run,
                platform: &value.receipt.platform,
                configuration_digest: &value.receipt.configuration_digest,
                checks: &value.receipt.checks,
                overall_status: value.receipt.overall_status,
            },
            Self::V2(value) => ReceiptPolicyView {
                repository: &value.receipt.repository,
                run: &value.receipt.run,
                platform: &value.receipt.platform,
                configuration_digest: &value.receipt.configuration_digest,
                checks: &value.receipt.checks,
                overall_status: value.receipt.overall_status,
            },
        }
    }
}

fn evaluate_policy(
    receipt: ReceiptPolicyView<'_>,
    policy: &VerificationPolicyV1,
    expected_commit: &str,
    evaluated_at: i64,
    findings: &mut Vec<VerificationFindingV1>,
) {
    check_equal(
        &receipt.repository.repository,
        &policy.project,
        "policy.repository",
        "repository.repository",
        "receipt project does not match repository policy",
        findings,
    );
    check_equal(
        &receipt.repository.commit_sha,
        expected_commit,
        "policy.commit",
        "repository.commit_sha",
        "receipt commit does not match the externally supplied commit",
        findings,
    );
    if receipt.repository.dirty {
        findings.push(finding(
            "policy.dirty",
            "repository.dirty",
            "repository policy requires a clean checkout",
        ));
    }
    check_equal(
        receipt.configuration_digest,
        &policy.configuration_digest,
        "policy.configuration",
        "configuration_digest",
        "receipt configuration digest does not match repository policy",
        findings,
    );
    check_equal(
        &receipt.platform.image_reference,
        &policy.image_reference,
        "policy.image",
        "platform.image_reference",
        "receipt image does not match repository policy",
        findings,
    );
    if !policy.platforms.iter().any(|accepted| {
        accepted.host_os == receipt.platform.host_os
            && accepted.host_arch == receipt.platform.host_arch
            && accepted.runtime_kind == receipt.platform.runtime_kind
    }) {
        findings.push(finding(
            "policy.platform",
            "platform",
            "receipt platform tuple is not accepted by repository policy",
        ));
    }
    if receipt.overall_status != EvidenceStatus::Pass {
        findings.push(finding(
            "policy.overall_status",
            "overall_status",
            "repository policy requires an overall PASS receipt",
        ));
    }

    let actual_required: BTreeSet<_> = receipt
        .checks
        .iter()
        .filter(|check| check.required)
        .map(|check| check.id.as_str())
        .collect();
    let expected_required: BTreeSet<_> =
        policy.required_checks.iter().map(String::as_str).collect();
    if actual_required != expected_required {
        findings.push(finding(
            "policy.required_check_set",
            "checks",
            "required check set does not exactly match repository policy",
        ));
    }
    if receipt.checks.iter().any(|check| {
        expected_required.contains(check.id.as_str()) && check.status != EvidenceStatus::Pass
    }) {
        findings.push(finding(
            "policy.required_check_result",
            "checks.status",
            "one or more policy-required checks did not PASS",
        ));
    }

    if let Some(finished_at) = parse_utc_seconds(&receipt.run.finished_at_utc) {
        if finished_at > evaluated_at {
            findings.push(finding(
                "policy.future_receipt",
                "run.finished_at_utc",
                "receipt completion time is later than verification time",
            ));
        } else if evaluated_at - finished_at > policy.max_age_seconds as i64 {
            findings.push(finding(
                "policy.stale_receipt",
                "run.finished_at_utc",
                "receipt exceeds repository freshness policy",
            ));
        }
    } else {
        findings.push(finding(
            "policy.invalid_time",
            "run.finished_at_utc",
            "receipt completion time cannot be evaluated",
        ));
    }
}

fn check_equal(
    actual: &str,
    expected: &str,
    code: &str,
    field: &str,
    message: &str,
    findings: &mut Vec<VerificationFindingV1>,
) {
    if actual != expected {
        findings.push(finding(code, field, message));
    }
}

const MAX_PLAN_FINDINGS: usize = 128;

fn compare_execution_plan(
    trusted: &ExecutionPlanV1,
    receipt: &ExecutionPlanV1,
    findings: &mut Vec<VerificationFindingV1>,
) -> Result<(), VerificationError> {
    let trusted = serde_json::to_value(trusted)
        .map_err(ReceiptError::Serialization)
        .map_err(VerificationError::Receipt)?;
    let receipt = serde_json::to_value(receipt)
        .map_err(ReceiptError::Serialization)
        .map_err(VerificationError::Receipt)?;
    compare_plan_value(&trusted, &receipt, "", findings);
    Ok(())
}

fn compare_plan_value(
    trusted: &serde_json::Value,
    receipt: &serde_json::Value,
    path: &str,
    findings: &mut Vec<VerificationFindingV1>,
) {
    if findings.len() >= MAX_PLAN_FINDINGS || trusted == receipt {
        return;
    }
    match (trusted, receipt) {
        (serde_json::Value::Object(trusted), serde_json::Value::Object(receipt)) => {
            let keys: BTreeSet<_> = trusted.keys().chain(receipt.keys()).collect();
            for key in keys {
                let child_path = format!("{path}/{}", escape_json_pointer(key));
                match (trusted.get(key), receipt.get(key)) {
                    (Some(trusted), Some(receipt)) => {
                        compare_plan_value(trusted, receipt, &child_path, findings)
                    }
                    _ => push_plan_finding(&child_path, findings),
                }
                if findings.len() >= MAX_PLAN_FINDINGS {
                    break;
                }
            }
        }
        (serde_json::Value::Array(trusted), serde_json::Value::Array(receipt)) => {
            let longest = trusted.len().max(receipt.len());
            for index in 0..longest {
                let child_path = format!("{path}/{index}");
                match (trusted.get(index), receipt.get(index)) {
                    (Some(trusted), Some(receipt)) => {
                        compare_plan_value(trusted, receipt, &child_path, findings)
                    }
                    _ => push_plan_finding(&child_path, findings),
                }
                if findings.len() >= MAX_PLAN_FINDINGS {
                    break;
                }
            }
        }
        _ => push_plan_finding(path, findings),
    }
}

fn escape_json_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn push_plan_finding(path: &str, findings: &mut Vec<VerificationFindingV1>) {
    if findings.len() < MAX_PLAN_FINDINGS {
        findings.push(finding(
            "policy.execution_plan.field_mismatch",
            &format!("execution_plan{path}"),
            "receipt execution-plan field does not match trusted configuration",
        ));
    }
}

pub(crate) fn finding(code: &str, field: &str, message: &str) -> VerificationFindingV1 {
    VerificationFindingV1 {
        code: code.to_owned(),
        field: field.to_owned(),
        message: message.to_owned(),
    }
}

pub fn verification_policy_schema_json() -> Result<String, VerificationError> {
    serde_json::to_string_pretty(&schema_for!(VerificationPolicyV1))
        .map_err(ReceiptError::Serialization)
        .map_err(VerificationError::Receipt)
}

pub fn trusted_plan_policy_schema_json() -> Result<String, VerificationError> {
    serde_json::to_string_pretty(&schema_for!(VerificationPolicyV1_1))
        .map_err(ReceiptError::Serialization)
        .map_err(VerificationError::Receipt)
}

pub fn verification_report_schema_json() -> Result<String, VerificationError> {
    serde_json::to_string_pretty(&schema_for!(VerificationReportV1))
        .map_err(ReceiptError::Serialization)
        .map_err(VerificationError::Receipt)
}

pub fn system_evaluated_at_utc() -> Result<String, VerificationError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| VerificationError::InvalidEvaluationTime)?
        .as_secs();
    format_unix_utc(seconds).ok_or(VerificationError::InvalidEvaluationTime)
}

fn validate_project(value: &str) -> Result<(), PolicyError> {
    if value.len() > 255 || value.chars().any(char::is_control) {
        return Err(PolicyError::InvalidField("project"));
    }
    let mut parts = value.split('/');
    let owner = parts.next().unwrap_or_default();
    let repository = parts.next().unwrap_or_default();
    if owner.is_empty()
        || repository.is_empty()
        || parts.next().is_some()
        || !valid_name(owner)
        || !valid_name(repository)
    {
        return Err(PolicyError::InvalidField("project"));
    }
    Ok(())
}

fn validate_name(field: &'static str, value: &str) -> Result<(), PolicyError> {
    if value.len() > 128 || !valid_name(value) {
        return Err(PolicyError::InvalidField(field));
    }
    Ok(())
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn validate_digest(value: &str) -> Result<(), PolicyError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(PolicyError::InvalidField("configuration_digest"));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(PolicyError::InvalidField("configuration_digest"));
    }
    Ok(())
}

fn validate_image_reference(value: &str) -> Result<(), PolicyError> {
    let Some((name, digest)) = value.rsplit_once('@') else {
        return Err(PolicyError::InvalidField("image_reference"));
    };
    if name.is_empty() || name.contains('@') || name.chars().any(char::is_control) {
        return Err(PolicyError::InvalidField("image_reference"));
    }
    validate_digest(digest).map_err(|_| PolicyError::InvalidField("image_reference"))
}

fn validate_trusted_config_path(value: &str) -> Result<(), PolicyError> {
    if value.is_empty()
        || value.len() > 255
        || value.starts_with('/')
        || value.starts_with('~')
        || value.contains('\\')
        || value.contains(':')
        || value
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        Err(PolicyError::InvalidField("trusted_config"))
    } else {
        Ok(())
    }
}

fn validate_producer_contract(value: &ProducerContractV1_1) -> Result<(), PolicyError> {
    validate_name("producers.name", &value.name)?;
    if value.version.is_empty()
        || value.version.len() > 128
        || value.version.chars().any(char::is_control)
    {
        Err(PolicyError::InvalidField("producers.version"))
    } else {
        Ok(())
    }
}

pub(crate) fn validate_commit(value: &str) -> Result<(), VerificationError> {
    if !matches!(value.len(), 40 | 64)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(VerificationError::InvalidExpectedCommit);
    }
    Ok(())
}

pub(crate) fn parse_utc_seconds(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return None;
    }
    let year = decimal(&bytes[0..4])? as i64;
    let month = decimal(&bytes[5..7])? as i64;
    let day = decimal(&bytes[8..10])? as i64;
    let hour = decimal(&bytes[11..13])? as i64;
    let minute = decimal(&bytes[14..16])? as i64;
    let second = decimal(&bytes[17..19])? as i64;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    if year == 0 || !(1..=days_in_month).contains(&day) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    days.checked_mul(86_400)?
        .checked_add(hour * 3_600 + minute * 60 + second)
}

fn decimal(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0_u32, |value, byte| {
        byte.is_ascii_digit()
            .then(|| value * 10 + u32::from(byte - b'0'))
    })
}

fn format_unix_utc(seconds: u64) -> Option<String> {
    let days = i64::try_from(seconds / 86_400).ok()?;
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days)?;
    if !(1..=9999).contains(&year) {
        return None;
    }
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
    ))
}

fn civil_from_days(days_since_epoch: i64) -> Option<(i64, u64, u64)> {
    let shifted = days_since_epoch.checked_add(719_468)?;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted.checked_sub(146_096)?
    } / 146_097;
    let day_of_era = shifted.checked_sub(era.checked_mul(146_097)?)?;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era.checked_add(era.checked_mul(400)?)?;
    let day_of_year =
        day_of_era.checked_sub(365 * year_of_era + year_of_era / 4 - year_of_era / 100)?;
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    Some((year, u64::try_from(month).ok()?, u64::try_from(day).ok()?))
}

#[derive(Debug)]
pub enum PolicyError {
    Io(io::Error),
    TooLarge,
    InvalidUtf8,
    Parse(toml::de::Error),
    UnsupportedSchemaVersion,
    InvalidField(&'static str),
    DuplicateValue(&'static str),
}

#[derive(Debug)]
pub enum TrustedPlanError {
    PolicyPath,
    Io(io::Error),
    UnsafeConfigurationPath,
    Config(ConfigError),
}

impl fmt::Display for TrustedPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PolicyPath => formatter.write_str("trusted policy path has no parent directory"),
            Self::Io(_) => formatter.write_str("cannot read trusted configuration"),
            Self::UnsafeConfigurationPath => {
                formatter.write_str("trusted configuration path is not a regular local file")
            }
            Self::Config(error) => write!(formatter, "trusted configuration is invalid: {error}"),
        }
    }
}

impl std::error::Error for TrustedPlanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Config(error) => Some(error),
            _ => None,
        }
    }
}

impl fmt::Display for PolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(_) => formatter.write_str("cannot read verification policy"),
            Self::TooLarge => formatter.write_str("verification policy exceeds size limit"),
            Self::InvalidUtf8 => formatter.write_str("verification policy is not UTF-8"),
            Self::Parse(_) => formatter.write_str("verification policy is not valid strict TOML"),
            Self::UnsupportedSchemaVersion => {
                formatter.write_str("verification policy schema version is unsupported")
            }
            Self::InvalidField(field) => write!(formatter, "invalid policy field: {field}"),
            Self::DuplicateValue(field) => write!(formatter, "duplicate policy value: {field}"),
        }
    }
}

impl std::error::Error for PolicyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Parse(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum VerificationError {
    Policy(PolicyError),
    PolicyDocument(String),
    TrustedPlan(TrustedPlanError),
    TrustedPolicyPathRequired,
    InvalidExpectedCommit,
    InvalidEvaluationTime,
    Receipt(ReceiptError),
    Matrix(String),
}

impl fmt::Display for VerificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(error) => write!(formatter, "{error}"),
            Self::PolicyDocument(error) => write!(formatter, "{error}"),
            Self::TrustedPlan(error) => write!(formatter, "{error}"),
            Self::TrustedPolicyPathRequired => formatter
                .write_str("trusted-plan policy verification requires the policy file path"),
            Self::InvalidExpectedCommit => {
                formatter.write_str("expected commit must be lowercase Git SHA-1 or SHA-256")
            }
            Self::InvalidEvaluationTime => {
                formatter.write_str("verification time is not representable as strict UTC")
            }
            Self::Receipt(_) => formatter.write_str("verification report serialization failed"),
            Self::Matrix(error) => write!(formatter, "matrix verification failed: {error}"),
        }
    }
}

impl std::error::Error for VerificationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Policy(error) => Some(error),
            Self::TrustedPlan(error) => Some(error),
            Self::Receipt(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{format_unix_utc, parse_utc_seconds};

    #[test]
    fn strict_utc_conversion_round_trips_known_instants() {
        for (seconds, timestamp) in [
            (0, "1970-01-01T00:00:00Z"),
            (951_827_696, "2000-02-29T12:34:56Z"),
            (1_785_542_400, "2026-08-01T00:00:00Z"),
        ] {
            assert_eq!(format_unix_utc(seconds).as_deref(), Some(timestamp));
            assert_eq!(parse_utc_seconds(timestamp), Some(seconds as i64));
        }
        assert_eq!(parse_utc_seconds("2026-02-29T00:00:00Z"), None);
        assert_eq!(parse_utc_seconds("2026-08-01T00:00:00+00:00"), None);
    }
}
