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
use std::path::PathBuf;

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

fn assert_type_contracts() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ReceiptError>();
    assert_send_sync::<ConfigError>();
    assert_send_sync::<PolicyError>();
    assert_send_sync::<commit_ci_preflight::verify::TrustedPlanError>();
    assert_send_sync::<VerificationError>();
    assert_send_sync::<MatrixError>();
}

fn assert_all_error_families_are_constructible_and_stable() {
    let json = serde_json::from_str::<serde_json::Value>("[").unwrap_err();
    let toml = toml::from_str::<toml::Value>("[").unwrap_err();
    let io = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
    let receipt_cases = [
        (
            ReceiptError::Serialization(json),
            "receipt serialization failed: EOF while parsing a list at line 1 column 1",
        ),
        (
            ReceiptError::UnsupportedSchemaVersion("9".into()),
            "unsupported receipt schema version: 9",
        ),
        (
            ReceiptError::UnsupportedSourceSnapshotSchemaVersion("9".into()),
            "unsupported source snapshot schema version: 9",
        ),
        (ReceiptError::EmptyField("x"), "receipt field is empty: x"),
        (
            ReceiptError::ControlCharacter("x"),
            "receipt field contains a control character: x",
        ),
        (
            ReceiptError::InvalidCommitSha("x".into()),
            "invalid commit SHA: x",
        ),
        (
            ReceiptError::InvalidRepositoryIdentity,
            "invalid repository identity",
        ),
        (ReceiptError::InvalidSha256("x"), "invalid SHA-256 value: x"),
        (
            ReceiptError::InvalidTimestamp("x"),
            "invalid UTC timestamp: x",
        ),
        (
            ReceiptError::InvalidRunWindow,
            "receipt run finishes before it starts",
        ),
        (
            ReceiptError::ImageDigestMismatch,
            "image reference is not pinned to image digest",
        ),
        (ReceiptError::UnsafePath("x"), "unsafe receipt path: x"),
        (ReceiptError::NoChecks, "receipt contains no checks"),
        (
            ReceiptError::NoRequiredChecks,
            "receipt contains no required checks",
        ),
        (
            ReceiptError::DuplicateCheckId("x".into()),
            "duplicate check ID: x",
        ),
        (
            ReceiptError::InvalidCommand("x".into()),
            "invalid command for check: x",
        ),
        (
            ReceiptError::InvalidCheckResult("x".into()),
            "inconsistent result for check: x",
        ),
        (
            ReceiptError::MissingIncompleteReason("x"),
            "missing incomplete reason: x",
        ),
        (
            ReceiptError::UnexpectedIncompleteReason("x"),
            "unexpected incomplete reason: x",
        ),
        (
            ReceiptError::InvalidSourceSnapshotEntryCount(2),
            "invalid source snapshot entry count: 2",
        ),
        (
            ReceiptError::OverallStatusMismatch {
                expected: commit_ci_preflight::receipt::EvidenceStatus::Pass,
                actual: commit_ci_preflight::receipt::EvidenceStatus::Fail,
            },
            "overall status mismatch: expected Pass, found Fail",
        ),
        (
            ReceiptError::DigestMismatch {
                expected: "a".into(),
                actual: "b".into(),
            },
            "receipt digest mismatch: expected a, found b",
        ),
        (
            ReceiptError::ExecutionPlanDigestMismatch {
                expected: "a".into(),
                actual: "b".into(),
            },
            "receipt execution plan digest mismatch: expected a, found b",
        ),
        (
            ReceiptError::ExecutionPlanCheckMismatch("x".into()),
            "receipt evidence does not match execution plan check: x",
        ),
        (
            ReceiptError::DuplicateArtifactEvidence("x".into()),
            "duplicate artifact evidence path: x",
        ),
        (
            ReceiptError::ArtifactManifestMismatch("x".into()),
            "artifact evidence does not match the execution plan: x",
        ),
        (
            ReceiptError::MissingRuntimeCapabilityEvidence,
            "schema 1.3 receipt lacks runtime capability evidence",
        ),
        (
            ReceiptError::UnexpectedRuntimeCapabilityEvidence,
            "historical receipt unexpectedly contains runtime capability evidence",
        ),
        (
            ReceiptError::InvalidRuntimeCapabilityEvidence("x"),
            "runtime capability evidence is invalid: x",
        ),
        (
            ReceiptError::RuntimeCapabilityEvidenceMismatch,
            "runtime capability evidence does not match the execution plan",
        ),
    ];
    for (error, expected) in receipt_cases {
        let source = matches!(&error, ReceiptError::Serialization(_));
        assert_error(error, expected, source);
    }
    let config_cases = vec![
        (
            ConfigError::Io {
                path: PathBuf::from("x"),
                source: std::io::Error::new(std::io::ErrorKind::Other, "e"),
            },
            "cannot read configuration x: e",
            true,
        ),
        (
            ConfigError::Parse(toml::from_str::<toml::Value>("[").unwrap_err()),
            "invalid TOML configuration: TOML parse error at line 1, column 2\n  |\n1 | [\n  |  ^\nunquoted keys cannot be empty, expected letters, numbers, `-`, `_`\n",
            true,
        ),
        (
            ConfigError::Receipt(ReceiptError::NoChecks),
            "cannot canonicalize execution plan: receipt contains no checks",
            true,
        ),
        (
            ConfigError::UnsupportedSchemaVersion("x".into()),
            "unsupported configuration schema version: x",
            false,
        ),
        (
            ConfigError::ConfigTooLarge {
                actual: 2,
                maximum: 1,
            },
            "configuration is 2 bytes; maximum is 1",
            false,
        ),
        (
            ConfigError::InvalidField("x"),
            "invalid configuration field: x",
            false,
        ),
        (
            ConfigError::OutOfRange {
                field: "x",
                minimum: 1,
                maximum: 2,
                actual: 3,
            },
            "configuration field x is 3; expected 1..=2",
            false,
        ),
        (
            ConfigError::TooManyItems {
                field: "x",
                actual: 3,
                maximum: 2,
            },
            "configuration has 3 x; maximum is 2",
            false,
        ),
        (
            ConfigError::NoChecks,
            "configuration contains no checks",
            false,
        ),
        (
            ConfigError::NoRequiredChecks,
            "configuration contains no required checks",
            false,
        ),
        (
            ConfigError::DuplicateId {
                field: "x",
                id: "y".into(),
            },
            "duplicate x: y",
            false,
        ),
        (
            ConfigError::DuplicateValue("x"),
            "duplicate value in x",
            false,
        ),
        (
            ConfigError::DuplicateArtifact("x".into()),
            "duplicate artifact path: x",
            false,
        ),
        (
            ConfigError::PathOverlap {
                first: "a".into(),
                second: "b".into(),
            },
            "configuration paths overlap: a and b",
            false,
        ),
        (
            ConfigError::SelfDependency("x".into()),
            "check depends on itself: x",
            false,
        ),
        (
            ConfigError::UnknownDependency {
                check: "a".into(),
                dependency: "b".into(),
            },
            "check a depends on unknown check b",
            false,
        ),
        (
            ConfigError::UnknownEnvironmentCache {
                name: "a".into(),
                cache_id: "b".into(),
            },
            "runtime-internal environment a references unknown cache b",
            false,
        ),
        (
            ConfigError::MissingStoragePolicy,
            "schemas 1.2 and 1.3 require an explicit storage policy",
            false,
        ),
        (
            ConfigError::MissingRuntimeCapabilityPolicy,
            "schema 1.3 requires pull_policy = never and swap_mode = disabled",
            false,
        ),
        (
            ConfigError::DependencyCycle(vec!["a".into(), "b".into()]),
            "check dependency cycle involves: a, b",
            false,
        ),
        (
            ConfigError::PlanDigestMismatch,
            "execution plan digest mismatch",
            false,
        ),
    ];
    for (error, expected, source) in config_cases {
        assert_error(error, expected, source);
    }
    let policy_cases = [
        (PolicyError::Io(io), "cannot read verification policy", true),
        (
            PolicyError::TooLarge,
            "verification policy exceeds size limit",
            false,
        ),
        (
            PolicyError::InvalidUtf8,
            "verification policy is not UTF-8",
            false,
        ),
        (
            PolicyError::Parse(toml),
            "verification policy is not valid strict TOML",
            true,
        ),
        (
            PolicyError::UnsupportedSchemaVersion,
            "verification policy schema version is unsupported",
            false,
        ),
        (
            PolicyError::InvalidField("x"),
            "invalid policy field: x",
            false,
        ),
        (
            PolicyError::DuplicateValue("x"),
            "duplicate policy value: x",
            false,
        ),
    ];
    for (error, expected, source) in policy_cases {
        assert_error(error, expected, source);
    }
    let verification_cases = [
        VerificationError::Policy(PolicyError::TooLarge),
        VerificationError::PolicyDocument("x".into()),
        VerificationError::TrustedPlan(commit_ci_preflight::verify::TrustedPlanError::PolicyPath),
        VerificationError::TrustedPolicyPathRequired,
        VerificationError::InvalidExpectedCommit,
        VerificationError::InvalidEvaluationTime,
        VerificationError::Receipt(ReceiptError::NoChecks),
        VerificationError::Matrix("x".into()),
    ];
    let expected = [
        "verification policy exceeds size limit",
        "x",
        "trusted policy path has no parent directory",
        "trusted-plan policy verification requires the policy file path",
        "expected commit must be lowercase Git SHA-1 or SHA-256",
        "verification time is not representable as strict UTC",
        "verification report serialization failed",
        "matrix verification failed: x",
    ];
    for (error, expected) in verification_cases.into_iter().zip(expected) {
        assert_error(
            error,
            expected,
            matches!(
                expected,
                "verification policy exceeds size limit"
                    | "trusted policy path has no parent directory"
                    | "verification report serialization failed"
            ),
        );
    }
    // MatrixError has no source() implementation; every variant is publicly constructible.
    let matrix_cases = vec![
        MatrixError::Io(std::io::Error::new(std::io::ErrorKind::Other, "x")),
        MatrixError::Parse(toml::from_str::<toml::Value>("[").unwrap_err()),
        MatrixError::Json(serde_json::from_str::<serde_json::Value>("[").unwrap_err()),
        MatrixError::Config(ConfigError::NoChecks),
        MatrixError::Receipt(ReceiptError::NoChecks),
        MatrixError::Policy("x".into()),
        MatrixError::Verification(VerificationError::InvalidExpectedCommit),
        MatrixError::Runtime(commit_ci_preflight::runtime::RuntimeError::Unavailable),
        MatrixError::Run(commit_ci_preflight::run::RunError::InvalidCommit),
        MatrixError::UnsupportedSchemaVersion("x".into()),
        MatrixError::ConfigTooLarge,
        MatrixError::InvalidField("x"),
        MatrixError::DuplicateValue("x"),
        MatrixError::UnknownRuntime("x".into()),
        MatrixError::RuntimeWithoutRequiredCheck("x".into()),
        MatrixError::CrossRuntimeDependency {
            check: "a".into(),
            dependency: "b".into(),
        },
        MatrixError::LegacyPlanNotRepresentable("x"),
        MatrixError::PlanDigestMismatch,
        MatrixError::ReceiptIdMismatch,
        MatrixError::InvalidReceipt,
        MatrixError::InvalidEvaluationTime,
    ];
    let expected = [
        "matrix I/O failed: x",
        "matrix configuration parse failed: TOML parse error",
        "matrix JSON serialization failed: EOF while parsing a list at line 1 column 1",
        "matrix configuration invalid: configuration contains no checks",
        "matrix receipt invalid: receipt contains no checks",
        "matrix policy invalid: x",
        "matrix verification invalid: expected commit must be lowercase Git SHA-1 or SHA-256",
        "matrix runtime invalid: Docker-compatible runtime is unavailable",
        "matrix run failed: Git returned an invalid commit identifier",
        "unsupported matrix schema version: x",
        "matrix configuration exceeds the bounded input size",
        "invalid matrix field: x",
        "duplicate matrix value: x",
        "matrix check references unknown runtime: x",
        "matrix runtime has no required check: x",
        "matrix cross-runtime dependency is unsupported: a -> b",
        "matrix legacy plan cannot represent: x",
        "matrix plan digest mismatch",
        "matrix receipt identifier mismatch",
        "matrix receipt violates semantic invariants",
        "matrix evaluation time is invalid",
    ];
    for (error, prefix) in matrix_cases.into_iter().zip(expected) {
        assert!(error.source().is_none());
        assert!(
            error.to_string().starts_with(prefix),
            "{} != {prefix}",
            error
        );
    }
    let _ = PathBuf::new();
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
    assert_type_contracts();
    receipt_witness(ReceiptError::NoChecks);
    config_witness(ConfigError::NoChecks);
    policy_witness(PolicyError::TooLarge);
    trusted_witness(commit_ci_preflight::verify::TrustedPlanError::PolicyPath);
    verification_witness(VerificationError::InvalidExpectedCommit);
    matrix_witness(MatrixError::PlanDigestMismatch);
    assert_all_error_families_are_constructible_and_stable();
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
