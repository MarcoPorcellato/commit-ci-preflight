// Copyright 2026 Marco Porcellato
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.

use commit_ci_preflight::matrix::{
    MATRIX_CONFIG_SCHEMA_VERSION, MATRIX_POLICY_SCHEMA_VERSION, MATRIX_RECEIPT_SCHEMA_VERSION,
    MatrixConfigV2, MatrixReceiptEnvelopeV2, MatrixReceiptV2, MatrixRequiredCheckV2,
    MatrixRuntimePolicyV2, MatrixRuntimeReceiptV2, MatrixVerificationPolicyV2,
    matrix_config_schema_json, matrix_policy_schema_json, matrix_receipt_schema_json,
    verify_matrix_receipt_document,
};
use commit_ci_preflight::receipt::{
    CheckEvidence, EvidenceStatus, PlatformEvidence, ProducerEvidence, ReceiptEnvelopeV1,
    ReceiptV1, RepositoryEvidence, RunEvidence,
};
use serde_json::Value;
use commit_ci_preflight::verify::{
    AcceptedPlatformV1, VerificationDecision, VerificationPolicyDocument, VerificationStatus,
    verify_receipt_document_for_policy,
};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const IMAGE_311: &str = "example.invalid/python311@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const IMAGE_312: &str = "example.invalid/python312@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const CONFIG_SCHEMA: &str = include_str!("../schema/config-v2.schema.json");
const RECEIPT_SCHEMA: &str = include_str!("../schema/receipt-v2.schema.json");
const POLICY_SCHEMA: &str = include_str!("../schema/policy-v2.schema.json");

fn runtime_receipt(id: &str, image: &str, check_id: &str) -> ReceiptEnvelopeV1 {
    let digest = image.rsplit_once('@').expect("pinned image").1.to_owned();
    ReceiptEnvelopeV1::seal(ReceiptV1 {
        schema_version: "1.0".to_owned(),
        producer: ProducerEvidence {
            name: "commit-ci-preflight".to_owned(),
            version: "0.1.0".to_owned(),
        },
        repository: RepositoryEvidence {
            repository: "example/project".to_owned(),
            commit_sha: COMMIT.to_owned(),
            dirty: false,
        },
        run: RunEvidence {
            run_id: format!("run-{id}"),
            generation: 1,
            started_at_utc: "2026-08-16T10:00:00Z".to_owned(),
            finished_at_utc: "2026-08-16T10:00:01Z".to_owned(),
        },
        platform: PlatformEvidence {
            host_os: "macos".to_owned(),
            host_arch: "aarch64".to_owned(),
            runtime_kind: "docker_compatible".to_owned(),
            runtime_version: "test".to_owned(),
            image_reference: image.to_owned(),
            image_digest: digest,
        },
        configuration_digest: DIGEST.to_owned(),
        checks: vec![CheckEvidence {
            id: check_id.to_owned(),
            required: true,
            argv: vec!["python".to_owned(), "-V".to_owned()],
            working_directory: ".".to_owned(),
            status: EvidenceStatus::Pass,
            exit_code: Some(0),
            duration_ms: 1,
            timed_out: false,
            cancelled: false,
            output_digest: Some(DIGEST.to_owned()),
            incomplete_reason: None,
        }],
        overall_status: EvidenceStatus::Pass,
        incomplete_reason: None,
        redaction_policy_version: "1.0".to_owned(),
    })
    .expect("seal v1 receipt")
}

fn receipt() -> MatrixReceiptEnvelopeV2 {
    MatrixReceiptEnvelopeV2::seal(MatrixReceiptV2 {
        schema_version: MATRIX_RECEIPT_SCHEMA_VERSION.to_owned(),
        producer: ProducerEvidence {
            name: "commit-ci-preflight".to_owned(),
            version: "0.1.0".to_owned(),
        },
        repository: RepositoryEvidence {
            repository: "example/project".to_owned(),
            commit_sha: COMMIT.to_owned(),
            dirty: false,
        },
        run: RunEvidence {
            run_id: "matrix-run".to_owned(),
            generation: 1,
            started_at_utc: "2026-08-16T10:00:00Z".to_owned(),
            finished_at_utc: "2026-08-16T10:00:02Z".to_owned(),
        },
        configuration_digest: DIGEST.to_owned(),
        runtime_receipts: vec![
            MatrixRuntimeReceiptV2 {
                runtime_id: "python311".to_owned(),
                receipt: runtime_receipt("python311", IMAGE_311, "compat-py311"),
            },
            MatrixRuntimeReceiptV2 {
                runtime_id: "python312".to_owned(),
                receipt: runtime_receipt("python312", IMAGE_312, "repository-check"),
            },
        ],
        overall_status: EvidenceStatus::Pass,
        incomplete_reason: None,
        redaction_policy_version: "1.0".to_owned(),
    })
    .expect("seal matrix receipt")
}

fn policy() -> MatrixVerificationPolicyV2 {
    MatrixVerificationPolicyV2 {
        schema_version: MATRIX_POLICY_SCHEMA_VERSION.to_owned(),
        project: "example/project".to_owned(),
        configuration_digest: DIGEST.to_owned(),
        required_checks: vec![
            MatrixRequiredCheckV2 {
                id: "compat-py311".to_owned(),
                runtime_id: "python311".to_owned(),
            },
            MatrixRequiredCheckV2 {
                id: "repository-check".to_owned(),
                runtime_id: "python312".to_owned(),
            },
        ],
        max_age_seconds: 300,
        runtimes: vec![
            MatrixRuntimePolicyV2 {
                id: "python311".to_owned(),
                configuration_digest: DIGEST.to_owned(),
                image_reference: IMAGE_311.to_owned(),
                platforms: platforms(),
            },
            MatrixRuntimePolicyV2 {
                id: "python312".to_owned(),
                configuration_digest: DIGEST.to_owned(),
                image_reference: IMAGE_312.to_owned(),
                platforms: platforms(),
            },
        ],
    }
}

fn platforms() -> Vec<AcceptedPlatformV1> {
    vec![AcceptedPlatformV1 {
        host_os: "macos".to_owned(),
        host_arch: "aarch64".to_owned(),
        runtime_kind: "docker_compatible".to_owned(),
    }]
}

#[test]
fn v2_policy_binds_each_required_check_to_its_named_runtime() {
    let envelope = receipt();
    let report = verify_matrix_receipt_document(
        &envelope.canonical_bytes().expect("bytes"),
        &policy(),
        COMMIT,
        "2026-08-16T10:01:00Z",
    )
    .expect("report");
    assert_eq!(report.integrity_status, VerificationStatus::Pass);
    assert_eq!(report.decision, VerificationDecision::Pass);

    let dispatched = verify_receipt_document_for_policy(
        &envelope.canonical_bytes().expect("bytes"),
        &VerificationPolicyDocument::V2(policy()),
        COMMIT,
        "2026-08-16T10:01:00Z",
    )
    .expect("dispatched report");
    assert_eq!(dispatched.decision, VerificationDecision::Pass);

    let mut wrong = policy();
    wrong.required_checks[0].runtime_id = "python312".to_owned();
    wrong.required_checks[1].runtime_id = "python311".to_owned();
    let report = verify_matrix_receipt_document(
        &envelope.canonical_bytes().expect("bytes"),
        &wrong,
        COMMIT,
        "2026-08-16T10:01:00Z",
    )
    .expect("report");
    assert_eq!(report.decision, VerificationDecision::Fail);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == "policy.check_runtime")
    );

    let mut changed = receipt();
    changed.receipt.runtime_receipts[0]
        .receipt
        .receipt
        .configuration_digest =
        "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_owned();
    let changed_inner = changed.receipt.runtime_receipts[0].receipt.receipt.clone();
    changed.receipt.runtime_receipts[0].receipt =
        ReceiptEnvelopeV1::seal(changed_inner).expect("reseal inner");
    changed = MatrixReceiptEnvelopeV2::seal(changed.receipt).expect("reseal outer");
    let report = verify_matrix_receipt_document(
        &changed.canonical_bytes().expect("bytes"),
        &policy(),
        COMMIT,
        "2026-08-16T10:01:00Z",
    )
    .expect("report");
    assert_eq!(report.integrity_status, VerificationStatus::Pass);
    assert_eq!(report.decision, VerificationDecision::Fail);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == "policy.runtime_configuration")
    );
}

#[test]
fn v2_config_is_canonical_across_runtime_declaration_order() {
    let common = r#"
schema_version = "2.0"
project = "example/project"

[receipt]
output = ".ccp/receipt.json"
freshness_seconds = 300

[[checks]]
id = "repository-check"
runtime_id = "python312"
required = true
argv = ["python", "-V"]
working_directory = "."
timeout_seconds = 30

[[checks]]
id = "compat-py311"
runtime_id = "python311"
required = true
argv = ["python", "-V"]
working_directory = "."
timeout_seconds = 30
"#;
    let first = format!(
        "{common}\n{}\n{}",
        runtime("python312", IMAGE_312),
        runtime("python311", IMAGE_311)
    );
    let second = format!(
        "{common}\n{}\n{}",
        runtime("python311", IMAGE_311),
        runtime("python312", IMAGE_312)
    );
    let first = MatrixConfigV2::parse(&first)
        .expect("parse")
        .into_plan()
        .expect("plan");
    let second = MatrixConfigV2::parse(&second)
        .expect("parse")
        .into_plan()
        .expect("plan");
    assert_eq!(first.plan.schema_version, MATRIX_CONFIG_SCHEMA_VERSION);
    assert_eq!(
        first.canonical_bytes().expect("bytes"),
        second.canonical_bytes().expect("bytes")
    );
}

#[test]
fn v2_matrix_configuration_rejects_single_runtime_environment_classes() {
    let input = format!(
        r#"
schema_version = "2.0"
project = "example/project"

[environment.fixed]
SOURCE_DATE_EPOCH = "0"

[[checks]]
id = "repository-check"
runtime_id = "python312"
required = true
argv = ["python", "-V"]
working_directory = "."
timeout_seconds = 30

{}
{}
"#,
        runtime("python312", IMAGE_312),
        runtime("python311", IMAGE_311),
    );

    assert!(MatrixConfigV2::parse(&input).is_err());
}

#[test]
fn generated_v2_schemas_match_pinned_contracts() {
    assert_eq!(
        matrix_config_schema_json().expect("config schema"),
        CONFIG_SCHEMA
    );
    assert_eq!(
        matrix_receipt_schema_json().expect("receipt schema"),
        RECEIPT_SCHEMA
    );
    assert_eq!(
        matrix_policy_schema_json().expect("policy schema"),
        POLICY_SCHEMA
    );
}

fn runtime(id: &str, image: &str) -> String {
    format!(
        "[[runtimes]]\nid = \"{id}\"\nkind = \"docker_compatible\"\nimage = \"{image}\"\ncpu_count = 1\nmemory_mib = 256\npids_limit = 64\nnetwork = false\n"
    )
}

fn legacy_compatible_config() -> &'static str {
    include_str!("fixtures/config-v2-legacy-compatible.toml")
}

#[test]
fn historical_legacy_fixture_is_self_consistent() {
    let raw = include_str!("fixtures/matrix-v2-legacy-plan-044697.json");
    let provenance: Value = serde_json::from_str(include_str!(
        "fixtures/matrix-v2-legacy-plan-044697.provenance.json"
    ))
    .expect("provenance JSON");
    let document: Value = serde_json::from_str(raw).expect("plan JSON");
    let plan = document.get("plan").expect("plan");
    let plan_digest = document
        .get("plan_digest")
        .and_then(Value::as_str)
        .expect("plan_digest");
    assert_eq!(
        commit_ci_preflight::receipt::canonical_digest(plan).expect("canonical digest"),
        plan_digest
    );
    assert_eq!(provenance["output_sha256"], sha256_hex(raw.as_bytes()));
    assert_eq!(provenance["plan_digest"], plan_digest);
    assert_eq!(provenance["config_sha256"], sha256_hex(legacy_compatible_config().as_bytes()));
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
