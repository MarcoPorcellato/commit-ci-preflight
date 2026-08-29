use commit_ci_preflight::{
    config::{ConfigError, ExecutionPlanV1, NormalizedRuntime},
    matrix::{MatrixError, MatrixPlanV2},
    receipt::{ReceiptEnvelopeV1, ReceiptError},
    verify::{
        PolicyError, VerificationError, VerificationPolicyDocument, VerificationPolicyV1,
        VerificationPolicyV1_1, VerificationReportV1,
    },
};
use std::error::Error;

fn assert_error<E: Error + Send + Sync + 'static>(error: E, expected: &str, has_source: bool) {
    assert_eq!(error.to_string(), expected);
    assert_eq!(error.source().is_some(), has_source);
}

// Exhaustive witnesses intentionally keep future enum additions compile-visible.
fn receipt_witness(e: ReceiptError) {
    match e {
        ReceiptError::Serialization(_)
        | ReceiptError::UnsupportedSchemaVersion(_)
        | ReceiptError::UnsupportedSourceSnapshotSchemaVersion(_)
        | ReceiptError::EmptyField(_)
        | ReceiptError::ControlCharacter(_)
        | ReceiptError::InvalidCommitSha(_)
        | ReceiptError::InvalidRepositoryIdentity
        | ReceiptError::InvalidSha256(_)
        | ReceiptError::InvalidTimestamp(_)
        | ReceiptError::InvalidRunWindow
        | ReceiptError::ImageDigestMismatch
        | ReceiptError::UnsafePath(_)
        | ReceiptError::NoChecks
        | ReceiptError::NoRequiredChecks
        | ReceiptError::DuplicateCheckId(_)
        | ReceiptError::InvalidCommand(_)
        | ReceiptError::InvalidCheckResult(_)
        | ReceiptError::MissingIncompleteReason(_)
        | ReceiptError::UnexpectedIncompleteReason(_)
        | ReceiptError::InvalidSourceSnapshotEntryCount(_)
        | ReceiptError::OverallStatusMismatch { .. }
        | ReceiptError::DigestMismatch { .. }
        | ReceiptError::ExecutionPlanDigestMismatch { .. }
        | ReceiptError::ExecutionPlanCheckMismatch(_)
        | ReceiptError::DuplicateArtifactEvidence(_)
        | ReceiptError::ArtifactManifestMismatch(_)
        | ReceiptError::MissingRuntimeCapabilityEvidence
        | ReceiptError::UnexpectedRuntimeCapabilityEvidence
        | ReceiptError::InvalidRuntimeCapabilityEvidence(_)
        | ReceiptError::RuntimeCapabilityEvidenceMismatch => {}
    }
}
fn policy_witness(e: PolicyError) {
    match e {
        PolicyError::Io(_)
        | PolicyError::TooLarge
        | PolicyError::InvalidUtf8
        | PolicyError::Parse(_)
        | PolicyError::UnsupportedSchemaVersion
        | PolicyError::InvalidField(_)
        | PolicyError::DuplicateValue(_) => {}
    }
}
fn verification_witness(e: VerificationError) {
    match e {
        VerificationError::Policy(_)
        | VerificationError::PolicyDocument(_)
        | VerificationError::TrustedPlan(_)
        | VerificationError::TrustedPolicyPathRequired
        | VerificationError::InvalidExpectedCommit
        | VerificationError::InvalidEvaluationTime
        | VerificationError::Receipt(_)
        | VerificationError::Matrix(_) => {}
    }
}
fn config_witness(e: ConfigError) {
    match e {
        ConfigError::Io { .. }
        | ConfigError::Parse(_)
        | ConfigError::Receipt(_)
        | ConfigError::UnsupportedSchemaVersion(_)
        | ConfigError::ConfigTooLarge { .. }
        | ConfigError::InvalidField(_)
        | ConfigError::OutOfRange { .. }
        | ConfigError::TooManyItems { .. }
        | ConfigError::NoChecks
        | ConfigError::NoRequiredChecks
        | ConfigError::DuplicateId { .. }
        | ConfigError::DuplicateValue(_)
        | ConfigError::DuplicateArtifact(_)
        | ConfigError::PathOverlap { .. }
        | ConfigError::SelfDependency(_)
        | ConfigError::UnknownDependency { .. }
        | ConfigError::UnknownEnvironmentCache { .. }
        | ConfigError::MissingStoragePolicy
        | ConfigError::MissingRuntimeCapabilityPolicy
        | ConfigError::DependencyCycle(_)
        | ConfigError::PlanDigestMismatch => {}
    }
}
fn matrix_witness(e: MatrixError) {
    match e {
        MatrixError::Io(_)
        | MatrixError::Parse(_)
        | MatrixError::Json(_)
        | MatrixError::Config(_)
        | MatrixError::Receipt(_)
        | MatrixError::Policy(_)
        | MatrixError::Verification(_)
        | MatrixError::Runtime(_)
        | MatrixError::Run(_)
        | MatrixError::UnsupportedSchemaVersion(_)
        | MatrixError::ConfigTooLarge
        | MatrixError::InvalidField(_)
        | MatrixError::DuplicateValue(_)
        | MatrixError::UnknownRuntime(_)
        | MatrixError::RuntimeWithoutRequiredCheck(_)
        | MatrixError::CrossRuntimeDependency { .. }
        | MatrixError::LegacyPlanNotRepresentable(_)
        | MatrixError::PlanDigestMismatch
        | MatrixError::ReceiptIdMismatch
        | MatrixError::InvalidReceipt
        | MatrixError::InvalidEvaluationTime => {}
    }
}
fn trusted_witness(e: commit_ci_preflight::verify::TrustedPlanError) {
    match e {
        commit_ci_preflight::verify::TrustedPlanError::PolicyPath
        | commit_ci_preflight::verify::TrustedPlanError::Io(_)
        | commit_ci_preflight::verify::TrustedPlanError::UnsafeConfigurationPath
        | commit_ci_preflight::verify::TrustedPlanError::Config(_) => {}
    }
}

#[test]
fn root_public_api_and_error_contract_is_compile_checked() {
    let _: Option<(
        ReceiptEnvelopeV1,
        ExecutionPlanV1,
        NormalizedRuntime,
        VerificationPolicyDocument,
        VerificationPolicyV1,
        VerificationPolicyV1_1,
        MatrixPlanV2,
        VerificationReportV1,
        ReceiptError,
        ConfigError,
        PolicyError,
        VerificationError,
        MatrixError,
    )> = None;
    assert_error(ReceiptError::NoChecks, "receipt contains no checks", false);
    assert_error(
        PolicyError::TooLarge,
        "verification policy exceeds size limit",
        false,
    );
    assert_error(
        VerificationError::TrustedPolicyPathRequired,
        "trusted-plan policy verification requires the policy file path",
        false,
    );
    assert_error(
        ConfigError::NoChecks,
        "configuration contains no checks",
        false,
    );
    assert_error(
        MatrixError::PlanDigestMismatch,
        "matrix plan digest mismatch",
        false,
    );
    let io = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
    assert_error(PolicyError::Io(io), "cannot read verification policy", true);
    let parse = toml::from_str::<toml::Value>("[").unwrap_err();
    assert_error(
        PolicyError::Parse(parse),
        "verification policy is not valid strict TOML",
        true,
    );
    assert_error(
        VerificationError::Policy(PolicyError::TooLarge),
        "verification policy exceeds size limit",
        true,
    );
    assert_error(
        VerificationError::TrustedPlan(commit_ci_preflight::verify::TrustedPlanError::PolicyPath),
        "trusted policy path has no parent directory",
        true,
    );
    assert_error(
        VerificationError::Receipt(ReceiptError::NoChecks),
        "verification report serialization failed",
        true,
    );
}
