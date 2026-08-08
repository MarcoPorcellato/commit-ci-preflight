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

use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const RECEIPT_SCHEMA_VERSION: &str = "1.0";
pub const RECEIPT_ID_PREFIX: &str = "sha256:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReceiptEnvelopeV1 {
    pub receipt_id: String,
    pub receipt: ReceiptV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReceiptV1 {
    pub schema_version: String,
    pub producer: ProducerEvidence,
    pub repository: RepositoryEvidence,
    pub run: RunEvidence,
    pub platform: PlatformEvidence,
    pub configuration_digest: String,
    pub checks: Vec<CheckEvidence>,
    pub overall_status: EvidenceStatus,
    pub incomplete_reason: Option<String>,
    pub redaction_policy_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProducerEvidence {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepositoryEvidence {
    pub repository: String,
    pub commit_sha: String,
    pub dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunEvidence {
    pub run_id: String,
    pub generation: u64,
    pub started_at_utc: String,
    pub finished_at_utc: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlatformEvidence {
    pub host_os: String,
    pub host_arch: String,
    pub runtime_kind: String,
    pub runtime_version: String,
    pub image_reference: String,
    pub image_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckEvidence {
    pub id: String,
    pub required: bool,
    pub argv: Vec<String>,
    pub working_directory: String,
    pub status: EvidenceStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub timed_out: bool,
    pub cancelled: bool,
    pub output_digest: Option<String>,
    pub incomplete_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvidenceStatus {
    Pass,
    Fail,
    Pending,
    NotRun,
}

impl ReceiptEnvelopeV1 {
    pub fn seal(receipt: ReceiptV1) -> Result<Self, ReceiptError> {
        receipt.validate()?;
        let receipt_id = digest_canonical(&receipt)?;
        Ok(Self {
            receipt_id,
            receipt,
        })
    }

    pub fn verify(&self) -> Result<(), ReceiptError> {
        self.receipt.validate()?;
        let expected = digest_canonical(&self.receipt)?;
        if self.receipt_id != expected {
            return Err(ReceiptError::DigestMismatch {
                expected,
                actual: self.receipt_id.clone(),
            });
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ReceiptError> {
        self.verify()?;
        canonical_json(self)
    }
}

impl ReceiptV1 {
    pub fn validate(&self) -> Result<(), ReceiptError> {
        if self.schema_version != RECEIPT_SCHEMA_VERSION {
            return Err(ReceiptError::UnsupportedSchemaVersion(
                self.schema_version.clone(),
            ));
        }

        require_text("producer.name", &self.producer.name)?;
        require_text("producer.version", &self.producer.version)?;
        validate_repository_identity(&self.repository.repository)?;
        validate_commit_sha(&self.repository.commit_sha)?;
        require_text("run.run_id", &self.run.run_id)?;
        require_timestamp("run.started_at_utc", &self.run.started_at_utc)?;
        require_timestamp("run.finished_at_utc", &self.run.finished_at_utc)?;
        if self.run.finished_at_utc < self.run.started_at_utc {
            return Err(ReceiptError::InvalidRunWindow);
        }
        require_text("platform.host_os", &self.platform.host_os)?;
        require_text("platform.host_arch", &self.platform.host_arch)?;
        require_text("platform.runtime_kind", &self.platform.runtime_kind)?;
        require_text("platform.runtime_version", &self.platform.runtime_version)?;
        require_text("platform.image_reference", &self.platform.image_reference)?;
        validate_sha256("platform.image_digest", &self.platform.image_digest)?;
        let image_reference_digest = self
            .platform
            .image_reference
            .rsplit_once('@')
            .map(|(_, digest)| digest);
        if image_reference_digest != Some(self.platform.image_digest.as_str()) {
            return Err(ReceiptError::ImageDigestMismatch);
        }
        validate_sha256("configuration_digest", &self.configuration_digest)?;
        require_text("redaction_policy_version", &self.redaction_policy_version)?;

        if self.checks.is_empty() {
            return Err(ReceiptError::NoChecks);
        }

        let mut check_ids = BTreeSet::new();
        let mut required_statuses = Vec::new();
        for check in &self.checks {
            check.validate()?;
            if !check_ids.insert(check.id.as_str()) {
                return Err(ReceiptError::DuplicateCheckId(check.id.clone()));
            }
            if check.required {
                required_statuses.push(check.status);
            }
        }

        if required_statuses.is_empty() {
            return Err(ReceiptError::NoRequiredChecks);
        }

        let expected_status = derive_overall_status(&required_statuses);
        if self.overall_status != expected_status {
            return Err(ReceiptError::OverallStatusMismatch {
                expected: expected_status,
                actual: self.overall_status,
            });
        }

        validate_incomplete_reason(
            "receipt.incomplete_reason",
            self.overall_status,
            self.incomplete_reason.as_deref(),
        )?;
        Ok(())
    }
}

impl CheckEvidence {
    fn validate(&self) -> Result<(), ReceiptError> {
        require_text("check.id", &self.id)?;
        if self.argv.is_empty() || self.argv.iter().any(|part| part.is_empty()) {
            return Err(ReceiptError::InvalidCommand(self.id.clone()));
        }
        validate_relative_path("check.working_directory", &self.working_directory)?;
        validate_incomplete_reason(
            "check.incomplete_reason",
            self.status,
            self.incomplete_reason.as_deref(),
        )?;

        match self.status {
            EvidenceStatus::Pass => {
                if self.exit_code != Some(0) || self.timed_out || self.cancelled {
                    return Err(ReceiptError::InvalidCheckResult(self.id.clone()));
                }
            }
            EvidenceStatus::Fail => {
                if matches!(self.exit_code, None | Some(0)) && !self.timed_out && !self.cancelled {
                    return Err(ReceiptError::InvalidCheckResult(self.id.clone()));
                }
            }
            EvidenceStatus::Pending | EvidenceStatus::NotRun => {
                if self.exit_code.is_some()
                    || self.duration_ms != 0
                    || self.timed_out
                    || self.cancelled
                    || self.output_digest.is_some()
                {
                    return Err(ReceiptError::InvalidCheckResult(self.id.clone()));
                }
            }
        }

        if let Some(digest) = &self.output_digest {
            validate_sha256("check.output_digest", digest)?;
        }
        Ok(())
    }
}

pub fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, ReceiptError> {
    let value = serde_json::to_value(value).map_err(ReceiptError::Serialization)?;
    let normalized = normalize_json(value);
    serde_json::to_vec(&normalized).map_err(ReceiptError::Serialization)
}

pub fn receipt_schema_json() -> Result<String, ReceiptError> {
    let schema = schema_for!(ReceiptEnvelopeV1);
    serde_json::to_string_pretty(&schema).map_err(ReceiptError::Serialization)
}

fn digest_canonical<T: Serialize>(value: &T) -> Result<String, ReceiptError> {
    let bytes = canonical_json(value)?;
    let digest = Sha256::digest(bytes);
    Ok(format!("{RECEIPT_ID_PREFIX}{}", encode_hex(&digest)))
}

fn normalize_json(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(normalize_json).collect()),
        Value::Object(items) => {
            let sorted: BTreeMap<_, _> = items
                .into_iter()
                .map(|(key, value)| (key, normalize_json(value)))
                .collect();
            Value::Object(sorted.into_iter().collect())
        }
        scalar => scalar,
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn derive_overall_status(statuses: &[EvidenceStatus]) -> EvidenceStatus {
    if statuses.contains(&EvidenceStatus::Fail) {
        EvidenceStatus::Fail
    } else if statuses
        .iter()
        .any(|status| matches!(status, EvidenceStatus::Pending | EvidenceStatus::NotRun))
    {
        EvidenceStatus::Pending
    } else {
        EvidenceStatus::Pass
    }
}

fn validate_incomplete_reason(
    field: &'static str,
    status: EvidenceStatus,
    reason: Option<&str>,
) -> Result<(), ReceiptError> {
    let requires_reason = matches!(status, EvidenceStatus::Pending | EvidenceStatus::NotRun);
    match (requires_reason, reason.map(str::trim)) {
        (true, None | Some("")) => Err(ReceiptError::MissingIncompleteReason(field)),
        (false, Some(reason)) if !reason.is_empty() => {
            Err(ReceiptError::UnexpectedIncompleteReason(field))
        }
        _ => Ok(()),
    }
}

fn require_text(field: &'static str, value: &str) -> Result<(), ReceiptError> {
    if value.trim().is_empty() {
        Err(ReceiptError::EmptyField(field))
    } else if value.chars().any(char::is_control) {
        Err(ReceiptError::ControlCharacter(field))
    } else {
        Ok(())
    }
}

fn validate_commit_sha(value: &str) -> Result<(), ReceiptError> {
    if matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(ReceiptError::InvalidCommitSha(value.to_owned()))
    }
}

fn validate_repository_identity(value: &str) -> Result<(), ReceiptError> {
    require_text("repository.repository", value)?;
    let mut segments = value.split('/');
    let owner = segments.next().unwrap_or_default();
    let repository = segments.next().unwrap_or_default();
    let allowed =
        |character: char| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.');
    if !owner.is_empty()
        && !repository.is_empty()
        && segments.next().is_none()
        && owner.chars().all(allowed)
        && repository.chars().all(allowed)
    {
        Ok(())
    } else {
        Err(ReceiptError::InvalidRepositoryIdentity)
    }
}

fn validate_sha256(field: &'static str, value: &str) -> Result<(), ReceiptError> {
    let Some(hex) = value.strip_prefix(RECEIPT_ID_PREFIX) else {
        return Err(ReceiptError::InvalidSha256(field));
    };
    if hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(ReceiptError::InvalidSha256(field))
    }
}

fn require_timestamp(field: &'static str, value: &str) -> Result<(), ReceiptError> {
    require_text(field, value)?;
    let bytes = value.as_bytes();
    let separators_are_valid = bytes.len() == 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z';
    if !separators_are_valid {
        return Err(ReceiptError::InvalidTimestamp(field));
    }

    let year = parse_decimal(&bytes[0..4]).ok_or(ReceiptError::InvalidTimestamp(field))?;
    let month = parse_decimal(&bytes[5..7]).ok_or(ReceiptError::InvalidTimestamp(field))?;
    let day = parse_decimal(&bytes[8..10]).ok_or(ReceiptError::InvalidTimestamp(field))?;
    let hour = parse_decimal(&bytes[11..13]).ok_or(ReceiptError::InvalidTimestamp(field))?;
    let minute = parse_decimal(&bytes[14..16]).ok_or(ReceiptError::InvalidTimestamp(field))?;
    let second = parse_decimal(&bytes[17..19]).ok_or(ReceiptError::InvalidTimestamp(field))?;

    let leap_year = divisible_by(year, 4) && (!divisible_by(year, 100) || divisible_by(year, 400));
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year => 29,
        2 => 28,
        _ => 0,
    };
    if year > 0 && (1..=days_in_month).contains(&day) && hour <= 23 && minute <= 59 && second <= 59
    {
        Ok(())
    } else {
        Err(ReceiptError::InvalidTimestamp(field))
    }
}

fn parse_decimal(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0_u32, |value, byte| {
        byte.is_ascii_digit()
            .then(|| value * 10 + u32::from(byte - b'0'))
    })
}

fn divisible_by(value: u32, divisor: u32) -> bool {
    value / divisor * divisor == value
}

fn validate_relative_path(field: &'static str, value: &str) -> Result<(), ReceiptError> {
    require_text(field, value)?;
    let is_safe = value == "."
        || (!value.starts_with('/')
            && !value.starts_with('~')
            && !value.contains('\\')
            && !value.contains(':')
            && value
                .split('/')
                .all(|segment| !segment.is_empty() && segment != "." && segment != ".."));
    if is_safe {
        Ok(())
    } else {
        Err(ReceiptError::UnsafePath(field))
    }
}

#[derive(Debug)]
pub enum ReceiptError {
    Serialization(serde_json::Error),
    UnsupportedSchemaVersion(String),
    EmptyField(&'static str),
    ControlCharacter(&'static str),
    InvalidCommitSha(String),
    InvalidRepositoryIdentity,
    InvalidSha256(&'static str),
    InvalidTimestamp(&'static str),
    InvalidRunWindow,
    ImageDigestMismatch,
    UnsafePath(&'static str),
    NoChecks,
    NoRequiredChecks,
    DuplicateCheckId(String),
    InvalidCommand(String),
    InvalidCheckResult(String),
    MissingIncompleteReason(&'static str),
    UnexpectedIncompleteReason(&'static str),
    OverallStatusMismatch {
        expected: EvidenceStatus,
        actual: EvidenceStatus,
    },
    DigestMismatch {
        expected: String,
        actual: String,
    },
}

impl fmt::Display for ReceiptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialization(error) => {
                write!(formatter, "receipt serialization failed: {error}")
            }
            Self::UnsupportedSchemaVersion(version) => {
                write!(formatter, "unsupported receipt schema version: {version}")
            }
            Self::EmptyField(field) => write!(formatter, "receipt field is empty: {field}"),
            Self::ControlCharacter(field) => {
                write!(
                    formatter,
                    "receipt field contains a control character: {field}"
                )
            }
            Self::InvalidCommitSha(value) => write!(formatter, "invalid commit SHA: {value}"),
            Self::InvalidRepositoryIdentity => write!(formatter, "invalid repository identity"),
            Self::InvalidSha256(field) => write!(formatter, "invalid SHA-256 value: {field}"),
            Self::InvalidTimestamp(field) => write!(formatter, "invalid UTC timestamp: {field}"),
            Self::InvalidRunWindow => write!(formatter, "receipt run finishes before it starts"),
            Self::ImageDigestMismatch => {
                write!(formatter, "image reference is not pinned to image digest")
            }
            Self::UnsafePath(field) => write!(formatter, "unsafe receipt path: {field}"),
            Self::NoChecks => write!(formatter, "receipt contains no checks"),
            Self::NoRequiredChecks => write!(formatter, "receipt contains no required checks"),
            Self::DuplicateCheckId(id) => write!(formatter, "duplicate check ID: {id}"),
            Self::InvalidCommand(id) => write!(formatter, "invalid command for check: {id}"),
            Self::InvalidCheckResult(id) => {
                write!(formatter, "inconsistent result for check: {id}")
            }
            Self::MissingIncompleteReason(field) => {
                write!(formatter, "missing incomplete reason: {field}")
            }
            Self::UnexpectedIncompleteReason(field) => {
                write!(formatter, "unexpected incomplete reason: {field}")
            }
            Self::OverallStatusMismatch { expected, actual } => write!(
                formatter,
                "overall status mismatch: expected {expected:?}, found {actual:?}"
            ),
            Self::DigestMismatch { expected, actual } => {
                write!(
                    formatter,
                    "receipt digest mismatch: expected {expected}, found {actual}"
                )
            }
        }
    }
}

impl std::error::Error for ReceiptError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Serialization(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(fill: char) -> String {
        format!("sha256:{}", fill.to_string().repeat(64))
    }

    fn passing_receipt() -> ReceiptV1 {
        ReceiptV1 {
            schema_version: RECEIPT_SCHEMA_VERSION.to_owned(),
            producer: ProducerEvidence {
                name: "commit-ci-preflight".to_owned(),
                version: "0.1.0".to_owned(),
            },
            repository: RepositoryEvidence {
                repository: "example/project".to_owned(),
                commit_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
                dirty: false,
            },
            run: RunEvidence {
                run_id: "fixture-run-0001".to_owned(),
                generation: 1,
                started_at_utc: "2026-08-08T12:00:00Z".to_owned(),
                finished_at_utc: "2026-08-08T12:00:01Z".to_owned(),
            },
            platform: PlatformEvidence {
                host_os: "macos".to_owned(),
                host_arch: "aarch64".to_owned(),
                runtime_kind: "orbstack".to_owned(),
                runtime_version: "fixture-1".to_owned(),
                image_reference: format!("example.invalid/ci@{}", digest('a')),
                image_digest: digest('a'),
            },
            configuration_digest: digest('b'),
            checks: vec![CheckEvidence {
                id: "rust-test".to_owned(),
                required: true,
                argv: vec!["cargo".to_owned(), "test".to_owned()],
                working_directory: ".".to_owned(),
                status: EvidenceStatus::Pass,
                exit_code: Some(0),
                duration_ms: 1000,
                timed_out: false,
                cancelled: false,
                output_digest: Some(digest('c')),
                incomplete_reason: None,
            }],
            overall_status: EvidenceStatus::Pass,
            incomplete_reason: None,
            redaction_policy_version: "1".to_owned(),
        }
    }

    #[test]
    fn deterministic_replay_produces_identical_bytes_and_id() {
        let first = ReceiptEnvelopeV1::seal(passing_receipt()).expect("first receipt");
        let second = ReceiptEnvelopeV1::seal(passing_receipt()).expect("second receipt");

        assert_eq!(first, second);
        assert_eq!(
            first.canonical_bytes().expect("first bytes"),
            second.canonical_bytes().expect("second bytes")
        );
    }

    #[test]
    fn tampering_is_rejected() {
        let mut envelope = ReceiptEnvelopeV1::seal(passing_receipt()).expect("receipt");
        envelope.receipt.run.generation += 1;

        assert!(matches!(
            envelope.verify(),
            Err(ReceiptError::DigestMismatch { .. })
        ));
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let envelope = ReceiptEnvelopeV1::seal(passing_receipt()).expect("receipt");
        let mut value = serde_json::to_value(envelope).expect("JSON value");
        value
            .as_object_mut()
            .expect("root object")
            .insert("unexpected".to_owned(), Value::Bool(true));

        let result = serde_json::from_value::<ReceiptEnvelopeV1>(value);
        assert!(result.is_err());
    }

    #[test]
    fn duplicate_check_ids_are_rejected() {
        let mut receipt = passing_receipt();
        receipt.checks.push(receipt.checks[0].clone());

        assert!(matches!(
            receipt.validate(),
            Err(ReceiptError::DuplicateCheckId(_))
        ));
    }

    #[test]
    fn pending_required_check_requires_truthful_reason() {
        let mut receipt = passing_receipt();
        let check = &mut receipt.checks[0];
        check.status = EvidenceStatus::Pending;
        check.exit_code = None;
        check.duration_ms = 0;
        check.output_digest = None;
        receipt.overall_status = EvidenceStatus::Pending;
        receipt.incomplete_reason = Some("Windows-native execution is pending".to_owned());

        assert!(matches!(
            receipt.validate(),
            Err(ReceiptError::MissingIncompleteReason(
                "check.incomplete_reason"
            ))
        ));

        receipt.checks[0].incomplete_reason =
            Some("Windows-native execution is pending".to_owned());
        receipt.validate().expect("truthful pending receipt");
    }

    #[test]
    fn absolute_and_parent_paths_are_rejected() {
        for path in [
            "/Users/example/project",
            "../project",
            "nested/../../escape",
            r"C:\Users\example\project",
            "C:/Users/example/project",
            "~/project",
            "nested//project",
            "nested/./project",
        ] {
            let mut receipt = passing_receipt();
            receipt.checks[0].working_directory = path.to_owned();
            assert!(matches!(
                receipt.validate(),
                Err(ReceiptError::UnsafePath("check.working_directory"))
            ));
        }
    }

    #[test]
    fn failed_check_requires_failure_evidence() {
        let mut receipt = passing_receipt();
        receipt.checks[0].status = EvidenceStatus::Fail;
        receipt.checks[0].exit_code = None;
        receipt.checks[0].output_digest = None;
        receipt.overall_status = EvidenceStatus::Fail;

        assert!(matches!(
            receipt.validate(),
            Err(ReceiptError::InvalidCheckResult(id)) if id == "rust-test"
        ));

        receipt.checks[0].timed_out = true;
        receipt.validate().expect("timeout is failure evidence");
    }

    #[test]
    fn uppercase_or_short_digests_are_rejected() {
        for value in [digest('A'), "sha256:abcd".to_owned(), "abcd".to_owned()] {
            let mut receipt = passing_receipt();
            receipt.configuration_digest = value;
            assert!(matches!(
                receipt.validate(),
                Err(ReceiptError::InvalidSha256("configuration_digest"))
            ));
        }
    }

    #[test]
    fn overall_status_is_derived_from_required_checks() {
        let mut receipt = passing_receipt();
        receipt.overall_status = EvidenceStatus::Pending;
        receipt.incomplete_reason = Some("incorrectly marked pending".to_owned());

        assert!(matches!(
            receipt.validate(),
            Err(ReceiptError::OverallStatusMismatch {
                expected: EvidenceStatus::Pass,
                actual: EvidenceStatus::Pending
            })
        ));
    }

    #[test]
    fn timestamps_are_strict_utc_and_ordered() {
        for timestamp in [
            "2026-02-29T12:00:00Z",
            "2026-13-01T12:00:00Z",
            "2026-08-08 12:00:00Z",
            "2026-08-08T24:00:00Z",
            "2026-08-08T12:00:00+00:00",
        ] {
            let mut receipt = passing_receipt();
            receipt.run.started_at_utc = timestamp.to_owned();
            assert!(matches!(
                receipt.validate(),
                Err(ReceiptError::InvalidTimestamp("run.started_at_utc"))
            ));
        }

        let mut receipt = passing_receipt();
        receipt.run.started_at_utc = "2026-08-08T12:00:02Z".to_owned();
        assert!(matches!(
            receipt.validate(),
            Err(ReceiptError::InvalidRunWindow)
        ));
    }

    #[test]
    fn repository_identity_cannot_embed_credentials_or_urls() {
        for repository in [
            "https://github.com/example/project",
            "user:token@example/project",
            "example/project/extra",
            "/example/project",
        ] {
            let mut receipt = passing_receipt();
            receipt.repository.repository = repository.to_owned();
            assert!(matches!(
                receipt.validate(),
                Err(ReceiptError::InvalidRepositoryIdentity)
            ));
        }
    }

    #[test]
    fn image_reference_must_match_pinned_digest() {
        let mut receipt = passing_receipt();
        receipt.platform.image_reference = "example.invalid/ci:latest".to_owned();
        assert!(matches!(
            receipt.validate(),
            Err(ReceiptError::ImageDigestMismatch)
        ));
    }

    #[test]
    fn schema_is_generated_and_versioned() {
        let schema = receipt_schema_json().expect("schema JSON");
        assert!(schema.contains("ReceiptEnvelopeV1"));
        assert!(schema.contains("receipt_id"));
        assert!(!schema.contains("SCREAMING_SNAKE_CASE"));
    }
}
