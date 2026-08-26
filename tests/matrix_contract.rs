// Copyright 2026 Marco Porcellato
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.

use commit_ci_preflight::config::{
    ArtifactKind, NormalizedArtifactContract, NormalizedFixedEnvironment,
    NormalizedRuntimeInternalEnvironment, RuntimePullPolicy, RuntimeSwapMode,
};
use commit_ci_preflight::matrix::{
    MATRIX_CONFIG_SCHEMA_VERSION, MATRIX_POLICY_SCHEMA_VERSION, MATRIX_RECEIPT_SCHEMA_VERSION,
    MatrixConfigV2, MatrixError, MatrixPlanEnvelopeV2, MatrixPlanProfile, MatrixReceiptEnvelopeV2,
    MatrixReceiptV2, MatrixRequiredCheckV2, MatrixRuntimePolicyV2, MatrixRuntimeReceiptV2,
    MatrixVerificationPolicyV2, build_matrix_plan, matrix_config_schema_json,
    matrix_policy_schema_json, matrix_receipt_schema_json, verify_matrix_receipt_document,
};
use commit_ci_preflight::receipt::{
    CheckEvidence, EvidenceStatus, PlatformEvidence, ProducerEvidence, ReceiptEnvelopeV1,
    ReceiptV1, RepositoryEvidence, RunEvidence,
};
use commit_ci_preflight::verify::{
    AcceptedPlatformV1, VerificationDecision, VerificationPolicyDocument, VerificationStatus,
    verify_receipt_document_for_policy,
};
use serde_json::Value;

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const IMAGE_311: &str = "example.invalid/python311@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const IMAGE_312: &str = "example.invalid/python312@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const CONFIG_SCHEMA: &str = include_str!("../schema/config-v2.schema.json");
const RECEIPT_SCHEMA: &str = include_str!("../schema/receipt-v2.schema.json");
const POLICY_SCHEMA: &str = include_str!("../schema/policy-v2.schema.json");
const LEGACY_COMPATIBLE_POLICY: &str = include_str!("fixtures/policy-v2-legacy-compatible.toml");
type MatrixEnvelopeMutator = fn(&mut MatrixPlanEnvelopeV2);

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
fn legacy_profile_reproduces_historical_plan() {
    let provenance: Value = serde_json::from_str(include_str!(
        "fixtures/matrix-v2-legacy-plan-044697.provenance.json"
    ))
    .expect("provenance JSON");
    let config = MatrixConfigV2::parse(legacy_compatible_config()).expect("parse fixture");
    let envelope = build_matrix_plan(config, MatrixPlanProfile::LegacyV1).expect("legacy plan");

    assert_eq!(MatrixPlanProfile::default(), MatrixPlanProfile::CurrentV2);
    assert_eq!(envelope.profile(), MatrixPlanProfile::LegacyV1);
    assert_eq!(
        envelope.plan_digest().expect("legacy digest"),
        provenance["outer_digest"].as_str().expect("outer digest")
    );
    for runtime in ["python311", "python312"] {
        assert_eq!(
            envelope
                .runtime_configuration_digest(runtime)
                .expect("known runtime"),
            provenance["runtime_digests"][runtime]
                .as_str()
                .expect("runtime digest")
        );
    }
    assert!(matches!(
        envelope.runtime_configuration_digest("unknown"),
        Err(MatrixError::UnknownRuntime(id)) if id == "unknown"
    ));
}

#[test]
fn legacy_compatible_policy_fixture_binds_historical_profile_digests() {
    let provenance: Value = serde_json::from_str(include_str!(
        "fixtures/matrix-v2-legacy-plan-044697.provenance.json"
    ))
    .expect("provenance JSON");
    let historical_plan: Value =
        serde_json::from_str(include_str!("fixtures/matrix-v2-legacy-plan-044697.json"))
            .expect("historical plan JSON");
    let policy = MatrixVerificationPolicyV2::parse(LEGACY_COMPATIBLE_POLICY).expect("policy");
    let planned_runtimes = historical_plan["plan"]["runtimes"]
        .as_array()
        .expect("historical runtimes");

    assert_eq!(policy.project, "example/legacy-matrix");
    assert_eq!(policy.configuration_digest, provenance["outer_digest"]);
    let policy_check_bindings: Vec<_> = policy
        .required_checks
        .iter()
        .map(|check| (check.id.as_str(), check.runtime_id.as_str()))
        .collect();
    let planned_check_bindings: Vec<_> = planned_runtimes
        .iter()
        .flat_map(|runtime| {
            let runtime_id = runtime["id"].as_str().expect("historical runtime ID");
            runtime["checks"]
                .as_array()
                .expect("historical runtime checks")
                .iter()
                .filter(|check| check["required"] == true)
                .map(move |check| {
                    (
                        check["id"].as_str().expect("historical check ID"),
                        runtime_id,
                    )
                })
        })
        .collect();
    assert_eq!(policy_check_bindings, planned_check_bindings);

    for runtime in &policy.runtimes {
        assert_eq!(
            runtime.configuration_digest,
            provenance["runtime_digests"][&runtime.id]
        );
        let planned_runtime = planned_runtimes
            .iter()
            .find(|candidate| candidate["id"] == runtime.id)
            .expect("planned runtime matching policy runtime");
        assert_eq!(
            runtime.image_reference,
            planned_runtime["runtime"]["image"]
                .as_str()
                .expect("historical runtime image")
        );
        assert_eq!(
            planned_runtime["runtime"]["kind"], "docker_compatible",
            "historical runtime kind"
        );
        assert_eq!(runtime.platforms.len(), 1, "fixed host platform tuple");
        assert_eq!(runtime.platforms[0].host_os, "macos");
        assert_eq!(runtime.platforms[0].host_arch, "aarch64");
        assert_eq!(runtime.platforms[0].runtime_kind, "docker_compatible");
    }
}

#[test]
fn legacy_receipt_provenance_is_uniform() {
    let legacy_version = MatrixPlanProfile::LegacyV1.producer_version();
    assert_eq!(legacy_version, "0.1.0+matrix-v2-legacy-v1");

    let mut legacy = receipt().receipt;
    legacy.producer.version = legacy_version.to_owned();
    for runtime in &mut legacy.runtime_receipts {
        runtime.receipt.receipt.producer.version = legacy_version.to_owned();
        let inner = runtime.receipt.receipt.clone();
        runtime.receipt = ReceiptEnvelopeV1::seal(inner).expect("reseal legacy inner receipt");
    }
    let legacy = MatrixReceiptEnvelopeV2::seal(legacy).expect("seal legacy matrix receipt");

    assert_eq!(legacy.receipt.producer.version, legacy_version);
    assert!(
        legacy.receipt.runtime_receipts.iter().all(|runtime| runtime
            .receipt
            .receipt
            .producer
            .version
            == legacy_version)
    );

    let mut mixed = legacy.receipt;
    mixed.runtime_receipts[1].receipt.receipt.producer.version = "0.1.0".to_owned();
    let inner = mixed.runtime_receipts[1].receipt.receipt.clone();
    mixed.runtime_receipts[1].receipt = ReceiptEnvelopeV1::seal(inner).expect("reseal mixed inner");
    assert!(matches!(
        MatrixReceiptEnvelopeV2::seal(mixed),
        Err(MatrixError::InvalidReceipt)
    ));
}

#[test]
fn legacy_profile_is_canonical_across_runtime_and_check_declaration_order() {
    let first = MatrixConfigV2::parse(legacy_compatible_config()).expect("parse first");
    let mut reordered = MatrixConfigV2::parse(legacy_compatible_config()).expect("parse second");
    reordered.runtimes.reverse();
    reordered.checks.reverse();

    let first = build_matrix_plan(first, MatrixPlanProfile::LegacyV1).expect("first plan");
    let second = build_matrix_plan(reordered, MatrixPlanProfile::LegacyV1).expect("second plan");
    assert_eq!(
        first.plan_digest().expect("first digest"),
        second.plan_digest().expect("second digest")
    );
    for runtime in ["python311", "python312"] {
        assert_eq!(
            first
                .runtime_configuration_digest(runtime)
                .expect("first runtime"),
            second
                .runtime_configuration_digest(runtime)
                .expect("second runtime")
        );
    }
}

#[test]
fn legacy_profile_accessors_reject_mutated_public_plan() {
    let mut envelope = build_matrix_plan(
        MatrixConfigV2::parse(legacy_compatible_config()).expect("parse fixture"),
        MatrixPlanProfile::LegacyV1,
    )
    .expect("legacy plan");
    envelope.plan.project = "example/mutated-project".to_owned();

    assert!(matches!(
        envelope.plan_digest(),
        Err(MatrixError::PlanDigestMismatch)
    ));
    assert!(matches!(
        envelope.runtime_configuration_digest("python311"),
        Err(MatrixError::PlanDigestMismatch)
    ));
    assert!(matches!(
        envelope.canonical_bytes(),
        Err(MatrixError::PlanDigestMismatch)
    ));
}

#[test]
fn legacy_runtime_envelopes_recheck_projection() {
    let legacy = build_matrix_plan(
        MatrixConfigV2::parse(legacy_compatible_config()).expect("parse fixture"),
        MatrixPlanProfile::LegacyV1,
    )
    .expect("legacy plan");
    let current = build_matrix_plan(
        MatrixConfigV2::parse(legacy_compatible_config()).expect("parse fixture"),
        MatrixPlanProfile::CurrentV2,
    )
    .expect("current plan");

    let legacy_runtimes = legacy.runtime_envelopes().expect("legacy envelopes");
    let current_runtimes = current.runtime_envelopes().expect("current envelopes");
    for ((legacy_id, legacy_runtime), (current_id, current_runtime)) in
        legacy_runtimes.iter().zip(current_runtimes.iter())
    {
        assert_eq!(legacy_id, current_id);
        assert_eq!(
            legacy_runtime.plan_digest,
            legacy
                .runtime_configuration_digest(legacy_id)
                .expect("legacy runtime digest")
        );
        assert_ne!(legacy_runtime.plan_digest, current_runtime.plan_digest);
    }

    let cases: [(&str, MatrixEnvelopeMutator); 4] = [
        ("runtime", |envelope| {
            envelope.plan.runtimes[0].runtime.image = "example.invalid/mutated@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned();
        }),
        ("check", |envelope| {
            envelope.plan.runtimes[0].checks[0]
                .argv
                .push("--mutated".to_owned());
        }),
        ("environment", |envelope| {
            envelope.plan.environment.inherit.push("MUTATED".to_owned());
        }),
        ("non-representable runtime field", |envelope| {
            envelope.plan.runtimes[0].runtime.pull_policy = Some(RuntimePullPolicy::Never);
        }),
    ];

    for (field, mutate) in cases {
        let mut envelope = legacy.clone();
        mutate(&mut envelope);
        assert!(
            matches!(
                envelope.runtime_envelopes(),
                Err(MatrixError::PlanDigestMismatch | MatrixError::LegacyPlanNotRepresentable(_))
            ),
            "runtime conversion must reject mutated {field}"
        );
    }
}

#[test]
fn legacy_profile_rejects_each_non_representable_current_field() {
    let cases: [(&str, MatrixEnvelopeMutator); 6] =
        [
            ("runtime.pull_policy", |envelope| {
                envelope.plan.runtimes[0].runtime.pull_policy = Some(RuntimePullPolicy::Never);
            }),
            ("runtime.swap_mode", |envelope| {
                envelope.plan.runtimes[0].runtime.swap_mode = Some(RuntimeSwapMode::Disabled);
            }),
            ("environment.fixed", |envelope| {
                envelope
                    .plan
                    .environment
                    .fixed
                    .push(NormalizedFixedEnvironment {
                        name: "FIXED".to_owned(),
                        value_digest: DIGEST.to_owned(),
                    });
            }),
            ("environment.runtime_internal", |envelope| {
                envelope.plan.environment.runtime_internal.push(
                    NormalizedRuntimeInternalEnvironment {
                        name: "INTERNAL".to_owned(),
                        cache_id: "cargo".to_owned(),
                        container_target: "/cache".to_owned(),
                    },
                );
            }),
            ("environment.remote_secret_only", |envelope| {
                envelope
                    .plan
                    .environment
                    .remote_secret_only
                    .push("SECRET".to_owned());
            }),
            ("checks.artifact_contracts", |envelope| {
                envelope.plan.runtimes[0].checks[0].artifact_contracts.push(
                    NormalizedArtifactContract {
                        path: "artifact.txt".to_owned(),
                        kind: ArtifactKind::RegularFile,
                        max_bytes: 1,
                        max_entries: 1,
                        producer_check: "python311-version".to_owned(),
                    },
                );
            }),
        ];

    for (field, mutate) in cases {
        let mut envelope = build_matrix_plan(
            MatrixConfigV2::parse(legacy_compatible_config()).expect("parse fixture"),
            MatrixPlanProfile::LegacyV1,
        )
        .expect("legacy plan");
        mutate(&mut envelope);
        assert!(matches!(
            envelope.plan_digest(),
            Err(MatrixError::LegacyPlanNotRepresentable(actual)) if actual == field
        ));
    }
}

#[test]
fn default_matrix_plan_matches_explicit_current_profile() {
    let config = MatrixConfigV2::parse(legacy_compatible_config()).expect("parse fixture");
    let default = config.clone().into_plan().expect("default plan");
    let current = build_matrix_plan(config, MatrixPlanProfile::CurrentV2).expect("current plan");

    assert_eq!(default, current);
    assert_eq!(
        default.plan_digest().expect("default digest"),
        current.plan_digest().expect("current digest")
    );
}

#[test]
fn production_sources_do_not_embed_adopter_expected_digests() {
    let prohibited = [
        "25b35b942a6ff9b6237ebed7cefbdbc96b968bbe8954a38b606942f36b8df4b2",
        "b3d8beef1542566d9d925bfee77d2244995dc74adcd879128ef65e82ed1d354b",
        "d446c4ca0602c09eee61c796ad2972f58ab0eebe84a39f928fd90aac5bfb535c",
        "13f4cb39b7e1a8ed31cae64502cc8e4d80d040230d3fb410a6afc3bad3b76178",
        "eff5b7d55bb0220890dbfb050bb68a1e0fbba8f9a30a69e2f66085354fcc8562",
        "7afb3e6dd435d9d5a317e4d9d85e80527431044312bbe299e9a70b6ba9e994c8",
    ];
    for path in ["src/matrix.rs", "src/matrix_legacy.rs"] {
        let source = std::fs::read_to_string(format!("{}/{}", env!("CARGO_MANIFEST_DIR"), path))
            .expect("production source");
        for digest in prohibited {
            assert!(
                !source.contains(digest),
                "production source {path} embeds adopter digest {digest}"
            );
        }
    }
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
        provenance["commit"],
        "044697dee9a0d678d30a4847d62ddf9b4970505b"
    );
    assert_eq!(
        provenance["tree"],
        "5220164edf17831ce0c42dae1c14300ed1045015"
    );
    assert_eq!(
        provenance["command_argv"],
        serde_json::json!([
            "commit-ci-preflight",
            "plan",
            "--config",
            "tests/fixtures/config-v2-legacy-compatible.toml",
            "--json"
        ])
    );
    assert_eq!(provenance["plan_digest"], plan_digest);
    assert_eq!(provenance["outer_digest"], plan_digest);
    let runtimes = plan["runtimes"].as_array().expect("runtimes");
    assert_eq!(
        runtimes[0]["configuration_digest"],
        provenance["runtime_digests"]["python311"]
    );
    assert_eq!(
        runtimes[1]["configuration_digest"],
        provenance["runtime_digests"]["python312"]
    );
    let binary_hash = provenance["binary_sha256"].as_str().expect("binary hash");
    assert_eq!(binary_hash.len(), 64);
    assert!(binary_hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(
        commit_ci_preflight::receipt::canonical_digest(plan).expect("canonical digest"),
        plan_digest
    );
    assert_eq!(provenance["output_sha256"], sha256_hex(raw.as_bytes()));
    assert_eq!(provenance["plan_digest"], plan_digest);
    assert_eq!(
        provenance["config_sha256"],
        sha256_hex(legacy_compatible_config().as_bytes())
    );
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
