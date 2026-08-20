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

use commit_ci_preflight::receipt::{EvidenceStatus, ReceiptEnvelopeV1, ReceiptEnvelopeV2};
use commit_ci_preflight::verify::{
    PolicyError, VerificationDecision, VerificationError, VerificationPolicyV1, VerificationStatus,
    verification_policy_schema_json, verification_report_schema_json, verify_receipt_document,
};

const RECEIPT: &[u8] = include_bytes!("fixtures/receipt-v1-pass.json");
const RECEIPT_V2: &[u8] = include_bytes!("fixtures/receipt-v2-pass.json");
const POLICY: &[u8] = include_bytes!("fixtures/policy-v1.toml");
const EXAMPLE_POLICY: &[u8] = include_bytes!("../examples/policy/example-project.toml");
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const EVALUATED_AT: &str = "2026-08-08T12:30:00Z";
const POLICY_SCHEMA: &str = include_str!("../schema/policy-v1.schema.json");
const REPORT_SCHEMA: &str = include_str!("../schema/verification-report-v1.schema.json");
const VERIFY_SOURCE: &str = include_str!("../src/verify.rs");

fn policy() -> VerificationPolicyV1 {
    VerificationPolicyV1::parse(POLICY).expect("policy")
}

fn receipt() -> ReceiptEnvelopeV1 {
    serde_json::from_slice(RECEIPT).expect("receipt")
}

fn report_for(envelope: &ReceiptEnvelopeV1) -> commit_ci_preflight::verify::VerificationReportV1 {
    verify_receipt_document(
        &envelope.canonical_bytes().expect("canonical receipt"),
        &policy(),
        COMMIT,
        EVALUATED_AT,
    )
    .expect("report")
}

fn finding_codes(report: &commit_ci_preflight::verify::VerificationReportV1) -> Vec<&str> {
    report
        .findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect()
}

fn collect_leaf_pointers(value: &serde_json::Value, path: &str, pointers: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, child) in object {
                collect_leaf_pointers(child, &format!("{path}/{key}"), pointers);
            }
        }
        serde_json::Value::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                collect_leaf_pointers(child, &format!("{path}/{index}"), pointers);
            }
        }
        _ => pointers.push(path.to_owned()),
    }
}

fn mutate_leaf(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Null => *value = serde_json::Value::Bool(true),
        serde_json::Value::Bool(current) => *current = !*current,
        serde_json::Value::Number(current) => {
            let changed = current.as_u64().expect("v1 numbers are unsigned") + 1;
            *value = serde_json::Value::Number(changed.into());
        }
        serde_json::Value::String(current) => current.push('x'),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            unreachable!("only JSON leaves are mutated")
        }
    }
}

#[test]
fn valid_receipt_passes_integrity_and_repository_policy() {
    let first = verify_receipt_document(RECEIPT, &policy(), COMMIT, EVALUATED_AT).expect("verify");
    let second = verify_receipt_document(RECEIPT, &policy(), COMMIT, EVALUATED_AT).expect("replay");

    assert_eq!(first, second);
    assert_eq!(first.integrity_status, VerificationStatus::Pass);
    assert_eq!(first.policy_status, VerificationStatus::Pass);
    assert_eq!(first.decision, VerificationDecision::Pass);
    assert_eq!(first.exit_code(), 0);
    assert!(first.findings.is_empty());
    assert!(first.receipt_id.is_some());
    assert_eq!(
        first.canonical_bytes().expect("report bytes"),
        second.canonical_bytes().expect("replay bytes")
    );
}

#[test]
fn valid_v2_receipt_passes_and_snapshot_tampering_fails_integrity() {
    let envelope: ReceiptEnvelopeV2 = serde_json::from_slice(RECEIPT_V2).expect("v2 receipt");
    let mut v2_policy = policy();
    v2_policy.configuration_digest = envelope.receipt.configuration_digest.clone();
    let report =
        verify_receipt_document(RECEIPT_V2, &v2_policy, COMMIT, EVALUATED_AT).expect("verify v2");
    assert_eq!(report.integrity_status, VerificationStatus::Pass);
    assert_eq!(report.policy_status, VerificationStatus::Pass);
    assert_eq!(report.decision, VerificationDecision::Pass);

    let mut envelope = envelope;
    envelope.receipt.source_snapshot.manifest_digest = format!("sha256:{}", "f".repeat(64));
    let tampered = serde_json::to_vec(&envelope).expect("tampered v2");
    let report = verify_receipt_document(&tampered, &v2_policy, COMMIT, EVALUATED_AT)
        .expect("tamper report");
    assert_eq!(report.integrity_status, VerificationStatus::Fail);
    assert_eq!(report.policy_status, VerificationStatus::NotRun);
}

#[test]
fn editing_every_receipt_leaf_breaks_integrity() {
    let original: serde_json::Value = serde_json::from_slice(RECEIPT).expect("JSON");
    let mut pointers = Vec::new();
    collect_leaf_pointers(&original, "", &mut pointers);
    assert!(
        pointers.len() > 30,
        "fixture should cover the complete v1 surface"
    );

    for pointer in pointers {
        let mut value = original.clone();
        mutate_leaf(value.pointer_mut(&pointer).expect("covered pointer"));
        let bytes = serde_json::to_vec(&value).expect("tampered bytes");
        let report = verify_receipt_document(&bytes, &policy(), COMMIT, EVALUATED_AT)
            .expect("tamper report");
        assert_eq!(
            report.integrity_status,
            VerificationStatus::Fail,
            "{pointer}"
        );
        assert_eq!(
            report.policy_status,
            VerificationStatus::NotRun,
            "{pointer}"
        );
        assert_eq!(report.exit_code(), 3, "{pointer}");
    }
}

#[test]
fn digest_valid_policy_mismatches_fail_each_covered_policy_dimension() {
    let mut cases: Vec<(&str, ReceiptEnvelopeV1, &str)> = Vec::new();

    let mut project = receipt().receipt;
    project.repository.repository = "other/project".to_owned();
    cases.push((
        "project",
        ReceiptEnvelopeV1::seal(project).expect("seal"),
        "policy.repository",
    ));

    let mut commit = receipt().receipt;
    commit.repository.commit_sha = "b".repeat(40);
    cases.push((
        "commit",
        ReceiptEnvelopeV1::seal(commit).expect("seal"),
        "policy.commit",
    ));

    let mut dirty = receipt().receipt;
    dirty.repository.dirty = true;
    cases.push((
        "dirty",
        ReceiptEnvelopeV1::seal(dirty).expect("seal"),
        "policy.dirty",
    ));

    let mut configuration = receipt().receipt;
    configuration.configuration_digest = format!("sha256:{}", "d".repeat(64));
    cases.push((
        "configuration",
        ReceiptEnvelopeV1::seal(configuration).expect("seal"),
        "policy.configuration",
    ));

    let mut image = receipt().receipt;
    image.platform.image_digest = format!("sha256:{}", "d".repeat(64));
    image.platform.image_reference =
        format!("example.invalid/other@{}", image.platform.image_digest);
    cases.push((
        "image",
        ReceiptEnvelopeV1::seal(image).expect("seal"),
        "policy.image",
    ));

    let mut platform = receipt().receipt;
    platform.platform.host_os = "linux".to_owned();
    cases.push((
        "platform",
        ReceiptEnvelopeV1::seal(platform).expect("seal"),
        "policy.platform",
    ));

    let mut stale = receipt().receipt;
    stale.run.started_at_utc = "2026-08-08T10:00:00Z".to_owned();
    stale.run.finished_at_utc = "2026-08-08T10:00:01Z".to_owned();
    cases.push((
        "freshness",
        ReceiptEnvelopeV1::seal(stale).expect("seal"),
        "policy.stale_receipt",
    ));

    let mut checks = receipt().receipt;
    checks.checks[0].id = "other-test".to_owned();
    cases.push((
        "required checks",
        ReceiptEnvelopeV1::seal(checks).expect("seal"),
        "policy.required_check_set",
    ));

    let mut result = receipt().receipt;
    result.checks[0].status = EvidenceStatus::Fail;
    result.checks[0].exit_code = Some(1);
    result.overall_status = EvidenceStatus::Fail;
    cases.push((
        "check result",
        ReceiptEnvelopeV1::seal(result).expect("seal"),
        "policy.required_check_result",
    ));

    for (label, envelope, expected_code) in cases {
        let report = report_for(&envelope);
        assert_eq!(report.integrity_status, VerificationStatus::Pass, "{label}");
        assert_eq!(report.policy_status, VerificationStatus::Fail, "{label}");
        assert_eq!(report.exit_code(), 3, "{label}");
        assert!(finding_codes(&report).contains(&expected_code), "{label}");
    }
}

#[test]
fn characterizes_declared_digest_not_binding_check_argv_before_t7() {
    let original = receipt();
    let mut altered = original.receipt.clone();
    altered.checks[0].argv = vec![
        "cargo".to_owned(),
        "test".to_owned(),
        "--release".to_owned(),
    ];
    assert_eq!(
        altered.configuration_digest,
        original.receipt.configuration_digest
    );
    let altered = ReceiptEnvelopeV1::seal(altered).expect("seal altered receipt");

    let report = report_for(&altered);

    assert_eq!(report.integrity_status, VerificationStatus::Pass);
    assert_eq!(report.policy_status, VerificationStatus::Pass);
    assert_eq!(report.decision, VerificationDecision::Pass);
    assert!(report.findings.is_empty());
}

#[test]
fn unsupported_schema_unknown_fields_and_oversize_input_fail_closed() {
    let mut unsupported: serde_json::Value = serde_json::from_slice(RECEIPT).expect("JSON");
    unsupported["receipt"]["schema_version"] = serde_json::Value::String("9.0".to_owned());
    let unsupported = verify_receipt_document(
        &serde_json::to_vec(&unsupported).expect("bytes"),
        &policy(),
        COMMIT,
        EVALUATED_AT,
    )
    .expect("unsupported report");
    assert!(finding_codes(&unsupported).contains(&"receipt.unsupported_schema"));

    let malformed =
        verify_receipt_document(b"{", &policy(), COMMIT, EVALUATED_AT).expect("malformed report");
    assert!(finding_codes(&malformed).contains(&"receipt.parse_or_shape"));

    let mut missing: serde_json::Value = serde_json::from_slice(RECEIPT).expect("JSON");
    missing["receipt"]
        .as_object_mut()
        .expect("receipt object")
        .remove("schema_version");
    let missing = verify_receipt_document(
        &serde_json::to_vec(&missing).expect("bytes"),
        &policy(),
        COMMIT,
        EVALUATED_AT,
    )
    .expect("missing schema report");
    assert!(finding_codes(&missing).contains(&"receipt.parse_or_shape"));

    let mut unknown: serde_json::Value = serde_json::from_slice(RECEIPT).expect("JSON");
    unknown["receipt"]["unexpected"] = serde_json::Value::Bool(true);
    let unknown = verify_receipt_document(
        &serde_json::to_vec(&unknown).expect("bytes"),
        &policy(),
        COMMIT,
        EVALUATED_AT,
    )
    .expect("unknown report");
    assert!(finding_codes(&unknown).contains(&"receipt.parse_or_shape"));

    let oversized = vec![b' '; 4 * 1024 * 1024 + 1];
    let oversized = verify_receipt_document(&oversized, &policy(), COMMIT, EVALUATED_AT)
        .expect("oversized report");
    assert!(finding_codes(&oversized).contains(&"receipt.too_large"));
}

#[test]
fn policy_parser_and_external_inputs_are_strict() {
    let unknown = std::str::from_utf8(POLICY).expect("UTF-8").replace(
        "schema_version = \"1.0\"",
        "schema_version = \"1.0\"\nunknown = true",
    );
    assert!(matches!(
        VerificationPolicyV1::parse(unknown.as_bytes()),
        Err(PolicyError::Parse(_))
    ));

    let duplicate = std::str::from_utf8(POLICY).expect("UTF-8").replace(
        "required_checks = [\"rust-test\"]",
        "required_checks = [\"rust-test\", \"rust-test\"]",
    );
    assert!(matches!(
        VerificationPolicyV1::parse(duplicate.as_bytes()),
        Err(PolicyError::DuplicateValue("required_checks"))
    ));

    assert!(matches!(
        verify_receipt_document(RECEIPT, &policy(), "HEAD", EVALUATED_AT),
        Err(VerificationError::InvalidExpectedCommit)
    ));
    assert!(matches!(
        verify_receipt_document(RECEIPT, &policy(), COMMIT, "now"),
        Err(VerificationError::InvalidEvaluationTime)
    ));
}

#[test]
fn future_receipt_is_not_freshness_evidence() {
    let report = verify_receipt_document(RECEIPT, &policy(), COMMIT, "2026-08-08T11:59:59Z")
        .expect("future report");
    assert_eq!(report.integrity_status, VerificationStatus::Pass);
    assert_eq!(report.policy_status, VerificationStatus::Fail);
    assert!(finding_codes(&report).contains(&"policy.future_receipt"));
}

#[test]
fn generated_policy_and_report_schemas_match_pinned_bytes() {
    assert_eq!(
        verification_policy_schema_json().expect("policy schema"),
        POLICY_SCHEMA
    );
    assert_eq!(
        verification_report_schema_json().expect("report schema"),
        REPORT_SCHEMA
    );
    VerificationPolicyV1::parse(EXAMPLE_POLICY).expect("example policy");
}

#[test]
fn verifier_has_no_execution_runtime_cache_or_workspace_dependency() {
    for forbidden in [
        "crate::run",
        "crate::runtime",
        "crate::cache",
        "crate::workspace",
        "ProcessSupervisor",
    ] {
        assert!(
            !VERIFY_SOURCE.contains(forbidden),
            "independent verifier imported execution concern: {forbidden}"
        );
    }
}
