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

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use commit_ci_preflight::matrix::{
    MatrixReceiptEnvelopeV2, MatrixReceiptV2, MatrixRuntimeReceiptV2, MatrixVerificationPolicyV2,
    verify_matrix_receipt_document,
};
use commit_ci_preflight::receipt::{
    CheckEvidence, EvidenceStatus, PlatformEvidence, ProducerEvidence, ReceiptEnvelopeV1,
    ReceiptEnvelopeV2, ReceiptV1, RepositoryEvidence, RunEvidence,
};
use commit_ci_preflight::verify::{
    PolicyError, VerificationDecision, VerificationError, VerificationPolicyDocument,
    VerificationPolicyV1, VerificationPolicyV1_1, VerificationStatus,
    load_verification_policy_document, trusted_plan_policy_schema_json,
    verification_policy_schema_json, verification_report_schema_json, verify_receipt_document,
    verify_receipt_document_for_policy_path,
};

const RECEIPT: &[u8] = include_bytes!("fixtures/receipt-v1-pass.json");
const RECEIPT_V2: &[u8] = include_bytes!("fixtures/receipt-v2-pass.json");
const POLICY: &[u8] = include_bytes!("fixtures/policy-v1.toml");
const EXAMPLE_POLICY: &[u8] = include_bytes!("../examples/policy/example-project.toml");
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const EVALUATED_AT: &str = "2026-08-08T12:30:00Z";
const POLICY_SCHEMA: &str = include_str!("../schema/policy-v1.schema.json");
const TRUSTED_PLAN_POLICY_SCHEMA: &str = include_str!("../schema/policy-v1_1.schema.json");
const REPORT_SCHEMA: &str = include_str!("../schema/verification-report-v1.schema.json");
const VERIFY_SOURCE: &str = include_str!("../src/verify.rs");
const TRUSTED_PLAN_POLICY: &str = "tests/fixtures/policy-v1_1-trusted-plan.toml";
const ALTERED_TRUSTED_PLAN_POLICY: &str = "tests/fixtures/policy-v1_1-trusted-plan-altered.toml";
const ROOT_POLICY: &str = ".commit-ci-policy.toml";
const LEGACY_MATRIX_POLICY: &str = include_str!("fixtures/policy-v2-legacy-compatible.toml");
const LEGACY_MATRIX_PROVENANCE: &str =
    include_str!("fixtures/matrix-v2-legacy-plan-044697.provenance.json");
const HISTORICAL_VERIFIER_PROVENANCE: &str =
    include_str!("fixtures/historical-verifier-044697.provenance.json");
const LEGACY_MATRIX_COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const LEGACY_MATRIX_EVALUATED_AT: &str = "2026-08-16T10:01:00Z";
const LEGACY_MATRIX_PRODUCER: &str = "0.1.0+matrix-v2-legacy-v1";
const LEGACY_MATRIX_OUTER_DIGEST: &str =
    "sha256:3248c763ccc37fecac1e29727007232d274f561e0943fc2e5a1996a38526fe13";
const LEGACY_MATRIX_PY311_DIGEST: &str =
    "sha256:755f77f6815b1ed7b4415b3312c48a6528e2d752270775916efa6c10f1ffe192";
const LEGACY_MATRIX_PY312_DIGEST: &str =
    "sha256:be2eb7d200946e9f1dc84cebd8c0cca8739e424163f91c86063bbe4caed936f9";
const LEGACY_MATRIX_PY311_IMAGE: &str = "example.invalid/python311@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LEGACY_MATRIX_PY312_IMAGE: &str = "example.invalid/python312@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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

fn legacy_matrix_policy() -> MatrixVerificationPolicyV2 {
    MatrixVerificationPolicyV2::parse(LEGACY_MATRIX_POLICY).expect("legacy Matrix policy")
}

#[test]
fn retained_historical_verifier_provenance_is_separate_from_generator_provenance() {
    let generator: serde_json::Value =
        serde_json::from_str(LEGACY_MATRIX_PROVENANCE).expect("generator provenance JSON");
    let verifier: serde_json::Value =
        serde_json::from_str(HISTORICAL_VERIFIER_PROVENANCE).expect("verifier provenance JSON");

    assert_eq!(generator["commit"], verifier["commit"]);
    assert_eq!(generator["tree"], verifier["tree"]);
    assert_eq!(
        generator["binary_sha256_status"],
        "observed_at_fixture_generation; temporary_binary_not_retained"
    );
    assert_eq!(
        verifier["binary_sha256_status"],
        "retained_historical_verifier"
    );
    assert_eq!(
        verifier["build_argv"],
        serde_json::json!(["cargo", "build", "--locked", "--offline"])
    );
    assert_eq!(verifier["plan_command_argv"][1], "plan");
    assert_eq!(verifier["output_sha256"], generator["output_sha256"]);
    assert_eq!(verifier["outer_digest"], generator["outer_digest"]);
    assert_eq!(verifier["runtime_digests"], generator["runtime_digests"]);
}

fn legacy_runtime_receipt(
    runtime_id: &str,
    image_reference: &str,
    configuration_digest: &str,
    check_id: &str,
) -> ReceiptEnvelopeV1 {
    let image_digest = image_reference
        .rsplit_once('@')
        .expect("pinned image")
        .1
        .to_owned();
    ReceiptEnvelopeV1::seal(ReceiptV1 {
        schema_version: "1.0".to_owned(),
        producer: ProducerEvidence {
            name: "commit-ci-preflight".to_owned(),
            version: LEGACY_MATRIX_PRODUCER.to_owned(),
        },
        repository: RepositoryEvidence {
            repository: "example/legacy-matrix".to_owned(),
            commit_sha: LEGACY_MATRIX_COMMIT.to_owned(),
            dirty: false,
        },
        run: RunEvidence {
            run_id: format!("legacy-{runtime_id}"),
            generation: 1,
            started_at_utc: "2026-08-16T10:00:00Z".to_owned(),
            finished_at_utc: "2026-08-16T10:00:01Z".to_owned(),
        },
        platform: PlatformEvidence {
            host_os: "macos".to_owned(),
            host_arch: "aarch64".to_owned(),
            runtime_kind: "docker_compatible".to_owned(),
            runtime_version: "test".to_owned(),
            image_reference: image_reference.to_owned(),
            image_digest,
        },
        configuration_digest: configuration_digest.to_owned(),
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
            output_digest: Some(configuration_digest.to_owned()),
            incomplete_reason: None,
        }],
        overall_status: EvidenceStatus::Pass,
        incomplete_reason: None,
        redaction_policy_version: "1.0".to_owned(),
    })
    .expect("seal legacy runtime receipt")
}

fn legacy_matrix_receipt() -> MatrixReceiptEnvelopeV2 {
    MatrixReceiptEnvelopeV2::seal(MatrixReceiptV2 {
        schema_version: "2.0".to_owned(),
        producer: ProducerEvidence {
            name: "commit-ci-preflight".to_owned(),
            version: LEGACY_MATRIX_PRODUCER.to_owned(),
        },
        repository: RepositoryEvidence {
            repository: "example/legacy-matrix".to_owned(),
            commit_sha: LEGACY_MATRIX_COMMIT.to_owned(),
            dirty: false,
        },
        run: RunEvidence {
            run_id: "legacy-matrix".to_owned(),
            generation: 1,
            started_at_utc: "2026-08-16T10:00:00Z".to_owned(),
            finished_at_utc: "2026-08-16T10:00:02Z".to_owned(),
        },
        configuration_digest: LEGACY_MATRIX_OUTER_DIGEST.to_owned(),
        runtime_receipts: vec![
            MatrixRuntimeReceiptV2 {
                runtime_id: "python311".to_owned(),
                receipt: legacy_runtime_receipt(
                    "python311",
                    LEGACY_MATRIX_PY311_IMAGE,
                    LEGACY_MATRIX_PY311_DIGEST,
                    "python311-version",
                ),
            },
            MatrixRuntimeReceiptV2 {
                runtime_id: "python312".to_owned(),
                receipt: legacy_runtime_receipt(
                    "python312",
                    LEGACY_MATRIX_PY312_IMAGE,
                    LEGACY_MATRIX_PY312_DIGEST,
                    "python312-version",
                ),
            },
        ],
        overall_status: EvidenceStatus::Pass,
        incomplete_reason: None,
        redaction_policy_version: "1.0".to_owned(),
    })
    .expect("seal legacy Matrix receipt")
}

fn matrix_report(
    bytes: &[u8],
    policy: &MatrixVerificationPolicyV2,
    commit: &str,
) -> commit_ci_preflight::verify::VerificationReportV1 {
    verify_matrix_receipt_document(bytes, policy, commit, LEGACY_MATRIX_EVALUATED_AT)
        .expect("verify Matrix receipt")
}

fn assert_matrix_failure(
    report: &commit_ci_preflight::verify::VerificationReportV1,
    finding_code: &str,
) {
    assert_eq!(report.decision, VerificationDecision::Fail);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == finding_code),
        "missing {finding_code}: {:?}",
        report.findings
    );
}

fn historical_verifier() -> PathBuf {
    let path = env::var_os("CCP_HISTORICAL_VERIFIER_044697")
        .map(PathBuf::from)
        .expect("set CCP_HISTORICAL_VERIFIER_044697 to the reviewed 044697 verifier binary");
    let expected = serde_json::from_str::<serde_json::Value>(HISTORICAL_VERIFIER_PROVENANCE)
        .expect("provenance JSON")["binary_sha256"]
        .as_str()
        .expect("provenance binary SHA-256")
        .to_owned();
    let actual = {
        use sha2::{Digest, Sha256};
        Sha256::digest(fs::read(&path).expect("read historical verifier"))
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        actual, expected,
        "refusing to invoke CCP_HISTORICAL_VERIFIER_044697 because it is not the provenance-pinned historical verifier"
    );
    path
}

fn with_historical_fixture<T>(operation: impl FnOnce(&Path) -> T) -> T {
    let mut directory = env::temp_dir();
    directory.push(format!(
        "commit-ci-preflight-historical-044697-{}-{}",
        std::process::id(),
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&directory).expect("create historical fixture directory");
    let result = operation(&directory);
    fs::remove_dir_all(&directory).expect("remove historical fixture directory");
    result
}

fn write_matrix_document(directory: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = directory.join(name);
    fs::write(&path, bytes).expect("write matrix test document");
    path
}

fn run_historical_verifier(
    verifier: &Path,
    receipt: &Path,
    policy: &Path,
    expected_commit: &str,
) -> std::process::Output {
    Command::new(verifier)
        .args([
            "verify",
            "--receipt",
            receipt.to_str().expect("UTF-8 receipt path"),
            "--policy",
            policy.to_str().expect("UTF-8 policy path"),
            "--expected-commit",
            expected_commit,
            "--evaluated-at-utc",
            LEGACY_MATRIX_EVALUATED_AT,
            "--json",
        ])
        .output()
        .expect("invoke provenance-pinned historical verifier")
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
fn receipt_v2_rejects_unknown_execution_plan_fields_before_policy_evaluation() {
    let mut raw: serde_json::Value = serde_json::from_slice(RECEIPT_V2).expect("receipt JSON");
    raw["receipt"]["execution_plan"]["unexpected"] = serde_json::Value::Bool(true);
    let report = verify_receipt_document(
        &serde_json::to_vec(&raw).expect("tampered bytes"),
        &policy(),
        COMMIT,
        EVALUATED_AT,
    )
    .expect("verification report");
    assert_eq!(report.integrity_status, VerificationStatus::Fail);
    assert_eq!(report.policy_status, VerificationStatus::NotRun);
    assert!(finding_codes(&report).contains(&"receipt.parse_or_shape"));
}

#[test]
fn trusted_plan_policy_reconstructs_the_plan() {
    let policy = Path::new(env!("CARGO_MANIFEST_DIR")).join(TRUSTED_PLAN_POLICY);
    let report = verify_receipt_document_for_policy_path(RECEIPT_V2, &policy, COMMIT, EVALUATED_AT)
        .expect("trusted-plan verification");
    assert_eq!(report.decision, VerificationDecision::Pass);
}

#[test]
fn trusted_plan_policy_reports_the_changed_execution_plan_field() {
    let policy = Path::new(env!("CARGO_MANIFEST_DIR")).join(ALTERED_TRUSTED_PLAN_POLICY);
    let report = verify_receipt_document_for_policy_path(RECEIPT_V2, &policy, COMMIT, EVALUATED_AT)
        .expect("trusted-plan mismatch report");
    assert_eq!(report.integrity_status, VerificationStatus::Pass);
    assert_eq!(report.policy_status, VerificationStatus::Fail);
    assert_eq!(report.decision, VerificationDecision::Fail);
    assert!(report.findings.iter().any(|finding| {
        finding.code == "policy.execution_plan.field_mismatch"
            && finding.field == "execution_plan/checks/0/argv/2"
    }));
    assert!(report.findings.iter().all(|finding| {
        !finding.message.contains("--release") && !finding.message.contains("cargo")
    }));
}

#[test]
fn repository_policy_uses_the_trusted_plan_contract() {
    let policy = Path::new(env!("CARGO_MANIFEST_DIR")).join(ROOT_POLICY);
    assert!(matches!(
        load_verification_policy_document(&policy).expect("root policy"),
        VerificationPolicyDocument::V1_1(_)
    ));
}

#[test]
fn trusted_plan_policy_rejects_downgrade_unsupported_producer_and_snapshot_strategy() {
    let policy = Path::new(env!("CARGO_MANIFEST_DIR")).join(TRUSTED_PLAN_POLICY);
    let v1 = verify_receipt_document_for_policy_path(RECEIPT, &policy, COMMIT, EVALUATED_AT)
        .expect("downgrade report");
    assert_eq!(v1.integrity_status, VerificationStatus::Pass);
    assert!(finding_codes(&v1).contains(&"policy.receipt_schema"));

    let original: ReceiptEnvelopeV2 = serde_json::from_slice(RECEIPT_V2).expect("v2 receipt");
    let mut unsupported = original.receipt.clone();
    unsupported.producer.version = "0.1.1".to_owned();
    let unsupported = ReceiptEnvelopeV2::seal(unsupported).expect("seal producer receipt");
    let unsupported = verify_receipt_document_for_policy_path(
        &unsupported.canonical_bytes().expect("producer bytes"),
        &policy,
        COMMIT,
        EVALUATED_AT,
    )
    .expect("producer report");
    assert!(finding_codes(&unsupported).contains(&"policy.producer_unsupported"));

    let mut strategy = original.receipt;
    strategy.source_snapshot.strategy =
        commit_ci_preflight::receipt::SourceSnapshotStrategy::GitArchive;
    let strategy = ReceiptEnvelopeV2::seal(strategy).expect("seal snapshot receipt");
    let strategy = verify_receipt_document_for_policy_path(
        &strategy.canonical_bytes().expect("snapshot bytes"),
        &policy,
        COMMIT,
        EVALUATED_AT,
    )
    .expect("snapshot report");
    assert!(finding_codes(&strategy).contains(&"policy.source_snapshot_strategy"));
}

#[test]
fn trusted_plan_policy_parser_rejects_unsafe_config_path_and_overlapping_producers() {
    let policy = std::str::from_utf8(include_bytes!("fixtures/policy-v1_1-trusted-plan.toml"))
        .expect("policy UTF-8");
    let unsafe_path = policy.replace(
        "trusted_config = \"config-v1-trusted-plan.toml\"",
        "trusted_config = \"../other.toml\"",
    );
    assert!(matches!(
        VerificationPolicyV1_1::parse(unsafe_path.as_bytes()),
        Err(PolicyError::InvalidField("trusted_config"))
    ));
    let overlap = format!(
        "{policy}\n[[revoked_producers]]\nname = \"commit-ci-preflight\"\nversion = \"0.1.0\"\n"
    );
    assert!(matches!(
        VerificationPolicyV1_1::parse(overlap.as_bytes()),
        Err(PolicyError::DuplicateValue("producer_contracts"))
    ));
}

#[test]
fn current_matrix_verifier_accepts_legacy_profile_receipt_and_rejects_mutations() {
    let policy = legacy_matrix_policy();
    let original = legacy_matrix_receipt();
    let original_bytes = original.canonical_bytes().expect("legacy receipt bytes");
    let valid = matrix_report(&original_bytes, &policy, LEGACY_MATRIX_COMMIT);
    assert_eq!(valid.decision, VerificationDecision::Pass);
    assert!(valid.findings.is_empty());

    let mut producer = serde_json::to_value(&original).expect("legacy receipt JSON");
    producer["receipt"]["producer"]["version"] = serde_json::Value::String("0.1.0".to_owned());
    let producer = matrix_report(
        &serde_json::to_vec(&producer).expect("producer mutation"),
        &policy,
        LEGACY_MATRIX_COMMIT,
    );
    assert_matrix_failure(&producer, "receipt.semantic_or_digest_invalid");

    let wrong_commit = matrix_report(&original_bytes, &policy, &"b".repeat(40));
    assert_matrix_failure(&wrong_commit, "policy.commit");

    let mut outer = original.clone().receipt;
    outer.configuration_digest = format!("sha256:{}", "d".repeat(64));
    let outer = MatrixReceiptEnvelopeV2::seal(outer).expect("reseal outer digest mutation");
    let outer = matrix_report(
        &outer.canonical_bytes().expect("outer mutation bytes"),
        &policy,
        LEGACY_MATRIX_COMMIT,
    );
    assert_matrix_failure(&outer, "policy.configuration");

    let mut runtime = original.clone().receipt;
    runtime.runtime_receipts[0]
        .receipt
        .receipt
        .configuration_digest = format!("sha256:{}", "d".repeat(64));
    let inner = runtime.runtime_receipts[0].receipt.receipt.clone();
    runtime.runtime_receipts[0].receipt = ReceiptEnvelopeV1::seal(inner).expect("reseal runtime");
    let runtime = MatrixReceiptEnvelopeV2::seal(runtime).expect("reseal runtime mutation");
    let runtime = matrix_report(
        &runtime.canonical_bytes().expect("runtime mutation bytes"),
        &policy,
        LEGACY_MATRIX_COMMIT,
    );
    assert_matrix_failure(&runtime, "policy.runtime_configuration");

    let mut check_binding = policy.clone();
    check_binding.required_checks[0].runtime_id = "python312".to_owned();
    check_binding.required_checks[1].runtime_id = "python311".to_owned();
    let check_binding = matrix_report(&original_bytes, &check_binding, LEGACY_MATRIX_COMMIT);
    assert_matrix_failure(&check_binding, "policy.check_runtime");

    let mut runtime_binding = original.clone().receipt;
    runtime_binding.runtime_receipts[0].runtime_id = "python312".to_owned();
    runtime_binding.runtime_receipts[1].runtime_id = "python311".to_owned();
    let runtime_binding =
        MatrixReceiptEnvelopeV2::seal(runtime_binding).expect("reseal runtime binding mutation");
    let runtime_binding = matrix_report(
        &runtime_binding
            .canonical_bytes()
            .expect("runtime binding mutation bytes"),
        &policy,
        LEGACY_MATRIX_COMMIT,
    );
    assert_matrix_failure(&runtime_binding, "policy.runtime_image");

    let mut altered_byte = original_bytes;
    let index = altered_byte
        .windows(b"legacy-matrix".len())
        .position(|window| window == b"legacy-matrix")
        .expect("legacy project in canonical receipt");
    altered_byte[index] = b'x';
    let altered_byte = matrix_report(&altered_byte, &policy, LEGACY_MATRIX_COMMIT);
    assert_matrix_failure(&altered_byte, "receipt.semantic_or_digest_invalid");
}

#[test]
#[ignore = "set CCP_HISTORICAL_VERIFIER_044697 to the provenance-pinned 044697 binary, then run with --ignored"]
fn historical_matrix_verifier_accepts_legacy_profile_receipt_and_rejects_mutations() {
    let verifier = historical_verifier();
    with_historical_fixture(|directory| {
        let original = legacy_matrix_receipt();
        let policy =
            write_matrix_document(directory, "policy.toml", LEGACY_MATRIX_POLICY.as_bytes());
        let receipt = write_matrix_document(
            directory,
            "receipt.json",
            &original.canonical_bytes().expect("legacy receipt bytes"),
        );
        let valid = run_historical_verifier(&verifier, &receipt, &policy, LEGACY_MATRIX_COMMIT);
        assert!(
            valid.status.success(),
            "historical verifier stderr: {}",
            String::from_utf8_lossy(&valid.stderr)
        );
        let valid_report: serde_json::Value =
            serde_json::from_slice(&valid.stdout).expect("historical valid report");
        assert_eq!(valid_report["decision"], "PASS");

        let mut producer = serde_json::to_value(&original).expect("legacy receipt JSON");
        producer["receipt"]["producer"]["version"] = serde_json::Value::String("0.1.0".to_owned());
        let producer = write_matrix_document(
            directory,
            "producer.json",
            &serde_json::to_vec(&producer).expect("producer mutation"),
        );
        let producer = run_historical_verifier(&verifier, &producer, &policy, LEGACY_MATRIX_COMMIT);
        assert_eq!(producer.status.code(), Some(3));
        let producer: serde_json::Value =
            serde_json::from_slice(&producer.stdout).expect("producer report");
        assert_eq!(
            producer["findings"][0]["code"],
            "receipt.semantic_or_digest_invalid"
        );

        let wrong_commit = run_historical_verifier(&verifier, &receipt, &policy, &"b".repeat(40));
        assert_eq!(wrong_commit.status.code(), Some(3));
        let wrong_commit: serde_json::Value =
            serde_json::from_slice(&wrong_commit.stdout).expect("commit report");
        assert!(
            wrong_commit["findings"]
                .as_array()
                .expect("commit findings")
                .iter()
                .any(|finding| finding["code"] == "policy.commit")
        );

        let mut outer = original.clone().receipt;
        outer.configuration_digest = format!("sha256:{}", "d".repeat(64));
        let outer = MatrixReceiptEnvelopeV2::seal(outer).expect("reseal outer digest mutation");
        let outer = write_matrix_document(
            directory,
            "outer.json",
            &outer.canonical_bytes().expect("outer mutation bytes"),
        );
        let outer = run_historical_verifier(&verifier, &outer, &policy, LEGACY_MATRIX_COMMIT);
        assert_eq!(outer.status.code(), Some(3));
        let outer: serde_json::Value = serde_json::from_slice(&outer.stdout).expect("outer report");
        assert!(
            outer["findings"]
                .as_array()
                .expect("outer findings")
                .iter()
                .any(|finding| finding["code"] == "policy.configuration")
        );

        let mut runtime = original.clone().receipt;
        runtime.runtime_receipts[0]
            .receipt
            .receipt
            .configuration_digest = format!("sha256:{}", "d".repeat(64));
        let inner = runtime.runtime_receipts[0].receipt.receipt.clone();
        runtime.runtime_receipts[0].receipt =
            ReceiptEnvelopeV1::seal(inner).expect("reseal runtime");
        let runtime = MatrixReceiptEnvelopeV2::seal(runtime).expect("reseal runtime mutation");
        let runtime = write_matrix_document(
            directory,
            "runtime.json",
            &runtime.canonical_bytes().expect("runtime mutation bytes"),
        );
        let runtime = run_historical_verifier(&verifier, &runtime, &policy, LEGACY_MATRIX_COMMIT);
        assert_eq!(runtime.status.code(), Some(3));
        let runtime: serde_json::Value =
            serde_json::from_slice(&runtime.stdout).expect("runtime report");
        assert!(
            runtime["findings"]
                .as_array()
                .expect("runtime findings")
                .iter()
                .any(|finding| finding["code"] == "policy.runtime_configuration")
        );

        let policy_with_wrong_check_binding = LEGACY_MATRIX_POLICY
            .replace(
                "id = \"python311-version\"\nruntime_id = \"python311\"",
                "id = \"python311-version\"\nruntime_id = \"python312\"",
            )
            .replace(
                "id = \"python312-version\"\nruntime_id = \"python312\"",
                "id = \"python312-version\"\nruntime_id = \"python311\"",
            );
        let check_binding = write_matrix_document(
            directory,
            "check-binding.toml",
            policy_with_wrong_check_binding.as_bytes(),
        );
        let check_binding =
            run_historical_verifier(&verifier, &receipt, &check_binding, LEGACY_MATRIX_COMMIT);
        assert_eq!(check_binding.status.code(), Some(3));
        let check_binding: serde_json::Value =
            serde_json::from_slice(&check_binding.stdout).expect("check binding report");
        assert!(
            check_binding["findings"]
                .as_array()
                .expect("check binding findings")
                .iter()
                .any(|finding| finding["code"] == "policy.check_runtime")
        );

        let mut runtime_binding = original.clone().receipt;
        runtime_binding.runtime_receipts[0].runtime_id = "python312".to_owned();
        runtime_binding.runtime_receipts[1].runtime_id = "python311".to_owned();
        let runtime_binding = MatrixReceiptEnvelopeV2::seal(runtime_binding)
            .expect("reseal runtime binding mutation");
        let runtime_binding = write_matrix_document(
            directory,
            "runtime-binding.json",
            &runtime_binding
                .canonical_bytes()
                .expect("runtime binding mutation bytes"),
        );
        let runtime_binding =
            run_historical_verifier(&verifier, &runtime_binding, &policy, LEGACY_MATRIX_COMMIT);
        assert_eq!(runtime_binding.status.code(), Some(3));
        let runtime_binding: serde_json::Value =
            serde_json::from_slice(&runtime_binding.stdout).expect("runtime binding report");
        assert!(
            runtime_binding["findings"]
                .as_array()
                .expect("runtime binding findings")
                .iter()
                .any(|finding| finding["code"] == "policy.runtime_image")
        );

        let mut altered_byte = original.canonical_bytes().expect("legacy receipt bytes");
        let index = altered_byte
            .windows(b"legacy-matrix".len())
            .position(|window| window == b"legacy-matrix")
            .expect("legacy project in canonical receipt");
        altered_byte[index] = b'x';
        let altered_byte = write_matrix_document(directory, "byte.json", &altered_byte);
        let altered_byte =
            run_historical_verifier(&verifier, &altered_byte, &policy, LEGACY_MATRIX_COMMIT);
        assert_eq!(altered_byte.status.code(), Some(3));
        let altered_byte: serde_json::Value =
            serde_json::from_slice(&altered_byte.stdout).expect("byte report");
        assert_eq!(
            altered_byte["findings"][0]["code"],
            "receipt.semantic_or_digest_invalid"
        );
    });
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
fn generated_trusted_plan_policy_schema_matches_pinned_bytes() {
    assert_eq!(
        trusted_plan_policy_schema_json().expect("trusted-plan policy schema"),
        TRUSTED_PLAN_POLICY_SCHEMA
    );
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
