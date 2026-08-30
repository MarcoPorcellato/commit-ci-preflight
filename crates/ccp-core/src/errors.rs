use std::fmt;
use std::io;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::ConfigError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvidenceStatus {
    Pass,
    Fail,
    Pending,
    NotRun,
}

#[derive(Debug)]
pub enum ReceiptError {
    Serialization(serde_json::Error),
    UnsupportedSchemaVersion(String),
    UnsupportedSourceSnapshotSchemaVersion(String),
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
    InvalidSourceSnapshotEntryCount(u64),
    OverallStatusMismatch {
        expected: EvidenceStatus,
        actual: EvidenceStatus,
    },
    DigestMismatch {
        expected: String,
        actual: String,
    },
    ExecutionPlanDigestMismatch {
        expected: String,
        actual: String,
    },
    ExecutionPlanCheckMismatch(String),
    DuplicateArtifactEvidence(String),
    ArtifactManifestMismatch(String),
    MissingRuntimeCapabilityEvidence,
    UnexpectedRuntimeCapabilityEvidence,
    InvalidRuntimeCapabilityEvidence(&'static str),
    RuntimeCapabilityEvidenceMismatch,
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
            Self::UnsupportedSourceSnapshotSchemaVersion(version) => {
                write!(
                    formatter,
                    "unsupported source snapshot schema version: {version}"
                )
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
            Self::InvalidSourceSnapshotEntryCount(count) => {
                write!(formatter, "invalid source snapshot entry count: {count}")
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
            Self::ExecutionPlanDigestMismatch { expected, actual } => write!(
                formatter,
                "receipt execution plan digest mismatch: expected {expected}, found {actual}"
            ),
            Self::ExecutionPlanCheckMismatch(id) => {
                write!(
                    formatter,
                    "receipt evidence does not match execution plan check: {id}"
                )
            }
            Self::DuplicateArtifactEvidence(path) => {
                write!(formatter, "duplicate artifact evidence path: {path}")
            }
            Self::ArtifactManifestMismatch(path) => {
                write!(
                    formatter,
                    "artifact evidence does not match the execution plan: {path}"
                )
            }
            Self::MissingRuntimeCapabilityEvidence => {
                write!(
                    formatter,
                    "schema 1.3 receipt lacks runtime capability evidence"
                )
            }
            Self::UnexpectedRuntimeCapabilityEvidence => write!(
                formatter,
                "historical receipt unexpectedly contains runtime capability evidence"
            ),
            Self::InvalidRuntimeCapabilityEvidence(field) => {
                write!(formatter, "runtime capability evidence is invalid: {field}")
            }
            Self::RuntimeCapabilityEvidenceMismatch => write!(
                formatter,
                "runtime capability evidence does not match the execution plan"
            ),
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

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(_) => f.write_str("cannot read verification policy"),
            Self::TooLarge => f.write_str("verification policy exceeds size limit"),
            Self::InvalidUtf8 => f.write_str("verification policy is not UTF-8"),
            Self::Parse(_) => f.write_str("verification policy is not valid strict TOML"),
            Self::UnsupportedSchemaVersion => {
                f.write_str("verification policy schema version is unsupported")
            }
            Self::InvalidField(field) => write!(f, "invalid policy field: {field}"),
            Self::DuplicateValue(field) => write!(f, "duplicate policy value: {field}"),
        }
    }
}
impl std::error::Error for PolicyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Parse(e) => Some(e),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum TrustedPlanError {
    PolicyPath,
    Io(io::Error),
    UnsafeConfigurationPath,
    Config(ConfigError),
}
impl fmt::Display for TrustedPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PolicyPath => f.write_str("trusted policy path has no parent directory"),
            Self::Io(_) => f.write_str("cannot read trusted configuration"),
            Self::UnsafeConfigurationPath => {
                f.write_str("trusted configuration path is not a regular local file")
            }
            Self::Config(e) => write!(f, "trusted configuration is invalid: {e}"),
        }
    }
}
impl std::error::Error for TrustedPlanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Config(e) => Some(e),
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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(e) => write!(f, "{e}"),
            Self::PolicyDocument(e) => write!(f, "{e}"),
            Self::TrustedPlan(e) => write!(f, "{e}"),
            Self::TrustedPolicyPathRequired => {
                f.write_str("trusted-plan policy verification requires the policy file path")
            }
            Self::InvalidExpectedCommit => {
                f.write_str("expected commit must be lowercase Git SHA-1 or SHA-256")
            }
            Self::InvalidEvaluationTime => {
                f.write_str("verification time is not representable as strict UTC")
            }
            Self::Receipt(_) => f.write_str("verification report serialization failed"),
            Self::Matrix(e) => write!(f, "matrix verification failed: {e}"),
        }
    }
}
impl std::error::Error for VerificationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Policy(e) => Some(e),
            Self::TrustedPlan(e) => Some(e),
            Self::Receipt(e) => Some(e),
            _ => None,
        }
    }
}
