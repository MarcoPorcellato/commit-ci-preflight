use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
