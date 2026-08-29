use serde::{
    Deserializer,
    de::{self, MapAccess, Visitor},
};
use sha2::{Digest, Sha256};
use std::fmt;
use std::{
    collections::{BTreeSet, HashSet},
    fs,
    path::{Path, PathBuf},
};

fn root_to_core(
    value: commit_ci_preflight::receipt::ReceiptEnvelopeV2,
) -> ccp_core::receipt::ReceiptEnvelopeV2 {
    value
}
fn core_to_root(
    value: ccp_core::config::ExecutionPlanV1,
) -> commit_ci_preflight::config::ExecutionPlanV1 {
    value
}
fn root_error_to_core(
    value: commit_ci_preflight::receipt::ReceiptError,
) -> ccp_core::errors::ReceiptError {
    value
}
fn core_runtime_to_root(
    value: ccp_core::runtime_evidence::RuntimeCapabilityEvidenceV1,
) -> commit_ci_preflight::runtime::RuntimeCapabilityEvidenceV1 {
    value
}

fn root_model_to_core(
    value: commit_ci_preflight::verify::VerificationReportV1,
) -> ccp_core::verification_model::VerificationReportV1 {
    value
}
fn core_model_to_root(
    value: ccp_core::verification_model::AcceptedPlatformV1,
) -> commit_ci_preflight::verify::AcceptedPlatformV1 {
    value
}
fn core_status_to_root(
    value: ccp_core::verification_model::VerificationStatus,
) -> commit_ci_preflight::verify::VerificationStatus {
    value
}
fn core_decision_to_root(
    value: ccp_core::verification_model::VerificationDecision,
) -> commit_ci_preflight::verify::VerificationDecision {
    value
}
fn core_finding_to_root(
    value: ccp_core::verification_model::VerificationFindingV1,
) -> commit_ci_preflight::verify::VerificationFindingV1 {
    value
}
fn root_policy_error_to_core(
    value: commit_ci_preflight::verify::PolicyError,
) -> ccp_core::errors::PolicyError {
    value
}
fn root_trusted_error_to_core(
    value: commit_ci_preflight::verify::TrustedPlanError,
) -> ccp_core::errors::TrustedPlanError {
    value
}
fn root_verification_error_to_core(
    value: commit_ci_preflight::verify::VerificationError,
) -> ccp_core::errors::VerificationError {
    value
}

#[test]
fn protocol_types_are_nominally_identical_across_root_and_core_paths() {
    let _: fn(
        commit_ci_preflight::receipt::ReceiptEnvelopeV2,
    ) -> ccp_core::receipt::ReceiptEnvelopeV2 = root_to_core;
    let _: fn(ccp_core::config::ExecutionPlanV1) -> commit_ci_preflight::config::ExecutionPlanV1 =
        core_to_root;
    let _: fn(commit_ci_preflight::receipt::ReceiptError) -> ccp_core::errors::ReceiptError =
        root_error_to_core;
    let _: fn(
        ccp_core::runtime_evidence::RuntimeCapabilityEvidenceV1,
    ) -> commit_ci_preflight::runtime::RuntimeCapabilityEvidenceV1 = core_runtime_to_root;
    let _: fn(
        commit_ci_preflight::verify::VerificationReportV1,
    ) -> ccp_core::verification_model::VerificationReportV1 = root_model_to_core;
    let _: fn(
        ccp_core::verification_model::AcceptedPlatformV1,
    ) -> commit_ci_preflight::verify::AcceptedPlatformV1 = core_model_to_root;
    let _: fn(
        ccp_core::verification_model::VerificationStatus,
    ) -> commit_ci_preflight::verify::VerificationStatus = core_status_to_root;
    let _: fn(
        ccp_core::verification_model::VerificationDecision,
    ) -> commit_ci_preflight::verify::VerificationDecision = core_decision_to_root;
    let _: fn(
        ccp_core::verification_model::VerificationFindingV1,
    ) -> commit_ci_preflight::verify::VerificationFindingV1 = core_finding_to_root;
    let _: fn(commit_ci_preflight::verify::PolicyError) -> ccp_core::errors::PolicyError =
        root_policy_error_to_core;
    let _: fn(commit_ci_preflight::verify::TrustedPlanError) -> ccp_core::errors::TrustedPlanError =
        root_trusted_error_to_core;
    let _: fn(
        commit_ci_preflight::verify::VerificationError,
    ) -> ccp_core::errors::VerificationError = root_verification_error_to_core;
}

#[test]
fn matrix_config_and_plan_paths_are_nominally_identical() {
    fn same<T>(_: Option<T>, _: Option<T>) {}
    same(
        None::<commit_ci_preflight::matrix::MatrixConfigV2>,
        None::<ccp_core::matrix::MatrixConfigV2>,
    );
    same(
        None::<commit_ci_preflight::matrix::MatrixEnvironmentConfigV2>,
        None::<ccp_core::matrix::MatrixEnvironmentConfigV2>,
    );
    same(
        None::<commit_ci_preflight::matrix::MatrixRuntimeConfigV2>,
        None::<ccp_core::matrix::MatrixRuntimeConfigV2>,
    );
    same(
        None::<commit_ci_preflight::matrix::MatrixCheckConfigV2>,
        None::<ccp_core::matrix::MatrixCheckConfigV2>,
    );
    same(
        None::<commit_ci_preflight::matrix::MatrixPlanEnvelopeV2>,
        None::<ccp_core::matrix::MatrixPlanEnvelopeV2>,
    );
    same(
        None::<commit_ci_preflight::matrix::MatrixPlanProfile>,
        None::<ccp_core::matrix::MatrixPlanProfile>,
    );
    same(
        None::<commit_ci_preflight::matrix::MatrixPlanV2>,
        None::<ccp_core::matrix::MatrixPlanV2>,
    );
    same(
        None::<commit_ci_preflight::matrix::MatrixRuntimePlanV2>,
        None::<ccp_core::matrix::MatrixRuntimePlanV2>,
    );
}

#[test]
fn matrix_receipt_policy_and_required_check_paths_are_nominally_identical() {
    fn same<T>(_: Option<T>, _: Option<T>) {}
    same(
        None::<commit_ci_preflight::matrix::MatrixReceiptEnvelopeV2>,
        None::<ccp_core::matrix::MatrixReceiptEnvelopeV2>,
    );
    same(
        None::<commit_ci_preflight::matrix::MatrixReceiptV2>,
        None::<ccp_core::matrix::MatrixReceiptV2>,
    );
    same(
        None::<commit_ci_preflight::matrix::MatrixRuntimeReceiptV2>,
        None::<ccp_core::matrix::MatrixRuntimeReceiptV2>,
    );
    same(
        None::<commit_ci_preflight::matrix::MatrixVerificationPolicyV2>,
        None::<ccp_core::matrix::MatrixVerificationPolicyV2>,
    );
    same(
        None::<commit_ci_preflight::matrix::MatrixRequiredCheckV2>,
        None::<ccp_core::matrix::MatrixRequiredCheckV2>,
    );
    same(
        None::<commit_ci_preflight::matrix::MatrixRuntimePolicyV2>,
        None::<ccp_core::matrix::MatrixRuntimePolicyV2>,
    );
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/matrix.rs")).unwrap();
    for token in [
        "pub struct MatrixReceiptEnvelopeV2",
        "pub struct MatrixReceiptV2",
        "pub struct MatrixRuntimeReceiptV2",
        "pub struct MatrixVerificationPolicyV2",
        "pub struct MatrixRequiredCheckV2",
        "pub struct MatrixRuntimePolicyV2",
    ] {
        assert!(
            !source.contains(token),
            "duplicate definition remains: {token}"
        );
    }
}

#[test]
fn root_verify_is_a_compatibility_facade_for_moved_models_and_errors() {
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/verify.rs")).unwrap();
    for token in [
        "pub enum VerificationStatus",
        "pub enum VerificationDecision",
        "pub struct VerificationReportV1",
        "pub enum PolicyError",
        "pub enum TrustedPlanError",
        "pub enum VerificationError",
    ] {
        assert!(
            !source.contains(token),
            "duplicate definition remains: {token}"
        );
    }
}

#[test]
fn core_protocol_module_dag_has_no_back_edges_or_duplicate_evidence_status() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let canonical = fs::read_to_string(root.join("crates/ccp-core/src/canonical.rs")).unwrap();
    let errors = fs::read_to_string(root.join("crates/ccp-core/src/errors.rs")).unwrap();
    let config = fs::read_to_string(root.join("crates/ccp-core/src/config.rs")).unwrap();
    let receipt = fs::read_to_string(root.join("crates/ccp-core/src/receipt.rs")).unwrap();
    assert!(!canonical.contains("crate::receipt"));
    assert!(!errors.contains("crate::receipt"));
    assert!(!config.contains("crate::receipt"));
    assert!(!receipt.contains("pub enum EvidenceStatus"));
    assert!(errors.contains("pub enum EvidenceStatus"));
}

const MANIFEST: &str = include_str!("fixtures/m2-compatibility-envelope-v1.json");

fn reject_duplicate_keys(input: &str) -> Result<(), serde_json::Error> {
    struct V;
    struct S;
    impl<'de> de::DeserializeSeed<'de> for S {
        type Value = ();
        fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
            d.deserialize_any(V)
        }
    }
    impl<'de> Visitor<'de> for V {
        type Value = ();
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("object")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<(), A::Error> {
            let mut seen = HashSet::new();
            while let Some(k) = m.next_key::<String>()? {
                if !seen.insert(k.clone()) {
                    return Err(de::Error::custom(format!("duplicate key: {k}")));
                }
                m.next_value_seed(S)?;
            }
            Ok(())
        }
        fn visit_seq<A: de::SeqAccess<'de>>(self, mut s: A) -> Result<(), A::Error> {
            while s.next_element_seed(S)?.is_some() {}
            Ok(())
        }
        fn visit_str<E: de::Error>(self, _: &str) -> Result<(), E> {
            Ok(())
        }
        fn visit_string<E: de::Error>(self, _: String) -> Result<(), E> {
            Ok(())
        }
        fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
            Ok(())
        }
        fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
            Ok(())
        }
        fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
            Ok(())
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
            Ok(())
        }
        fn visit_unit<E: de::Error>(self) -> Result<(), E> {
            Ok(())
        }
    }
    let mut d = serde_json::Deserializer::from_str(input);
    d.deserialize_map(V)
}

#[test]
fn m2_compatibility_envelope_hashes_are_frozen() {
    reject_duplicate_keys(MANIFEST).unwrap();
    let value: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
    let files = value["files"].as_object().unwrap();
    assert_eq!(value["schema_version"], "1.0");
    assert_eq!(
        value["source_head"],
        "6ff736b1e2a1dfde8778330efdd4b82c845d45e7"
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut seen = HashSet::new();
    for (name, expected) in files {
        assert!(Path::new(name).is_relative() && !name.contains(".."));
        assert!(
            expected.as_str().unwrap().len() == 64
                && expected
                    .as_str()
                    .unwrap()
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert!(seen.insert(name));
        let bytes = fs::read(root.join(name)).unwrap();
        let actual = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(actual, expected.as_str().unwrap(), "{name}");
    }
}

#[test]
fn duplicate_manifest_keys_are_rejected_by_contract_fixture() {
    let duplicate = r#"{"schema_version":"1.0","source_head":"x","files":{"a":"1","a":"2"}}"#;
    assert!(reject_duplicate_keys(duplicate).is_err());
}

#[test]
fn workspace_members_are_explicit_and_verifier_dependencies_are_bounded() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let value: toml::Value = toml::from_str(&manifest).unwrap();
    let workspace = value.get("workspace").unwrap().as_table().unwrap();
    assert_eq!(workspace["members"].as_array().unwrap().len(), 3);
    assert_eq!(
        workspace["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![".", "crates/ccp-core", "crates/ccp-verifier"]
    );
    assert_eq!(workspace["default-members"].as_array().unwrap().len(), 1);
    assert_eq!(
        workspace["default-members"].as_array().unwrap()[0]
            .as_str()
            .unwrap(),
        "."
    );
    assert_eq!(workspace["resolver"].as_str(), Some("3"));
    let root_dev = value["dev-dependencies"].as_table().unwrap();
    assert_eq!(
        root_dev["ccp-core"]["path"].as_str(),
        Some("crates/ccp-core")
    );
    assert_package_contract(
        &root.join("crates/ccp-core/Cargo.toml"),
        "ccp-core",
        ["schemars", "serde", "serde_json", "sha2", "toml"]
            .into_iter()
            .collect(),
        BTreeSet::new(),
        None,
    );
    assert_package_contract(
        &root.join("crates/ccp-verifier/Cargo.toml"),
        "ccp-verifier",
        ["ccp-core", "clap"].into_iter().collect(),
        ["serde_json"].into_iter().collect(),
        Some(
            ["derive", "error-context", "help", "std", "usage"]
                .into_iter()
                .collect(),
        ),
    );
    assert_no_forbidden_sources(&root.join("crates/ccp-verifier/src"));
}

fn assert_package_contract(
    path: &Path,
    name: &str,
    normal: BTreeSet<&str>,
    dev: BTreeSet<&str>,
    clap_features: Option<BTreeSet<&str>>,
) {
    let value: toml::Value = toml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(value["package"]["name"].as_str(), Some(name));
    assert_eq!(value["package"]["edition"].as_str(), Some("2024"));
    assert_eq!(value["package"]["rust-version"].as_str(), Some("1.87"));
    let actual: BTreeSet<_> = value["dependencies"]
        .as_table()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(actual, normal);
    let actual_dev: BTreeSet<_> = value
        .get("dev-dependencies")
        .and_then(toml::Value::as_table)
        .map(|t| t.keys().map(String::as_str).collect())
        .unwrap_or_default();
    assert_eq!(actual_dev, dev);
    if let Some(expected) = clap_features {
        let clap = &value["dependencies"]["clap"];
        assert_eq!(clap["default-features"].as_bool(), Some(false));
        let features: BTreeSet<_> = clap["features"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(features, expected);
    }
}

fn assert_no_forbidden_sources(root: &Path) {
    let mut files = Vec::new();
    collect_rs(root, &mut files);
    assert!(!files.is_empty());
    for path in files {
        let source = fs::read_to_string(path).unwrap().to_ascii_lowercase();
        for forbidden in [
            "commit_ci_preflight",
            "docker",
            "cache",
            "admission",
            "resource",
            "benchmark",
            "github",
            "serde_json",
        ] {
            assert!(
                !source.contains(forbidden),
                "forbidden source import/token: {forbidden}"
            );
        }
    }
}

fn collect_rs(root: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_rs(&path, files);
        } else if path.extension().and_then(|x| x.to_str()) == Some("rs") {
            files.push(path);
        }
    }
}
#[test]
fn core_and_root_verification_share_report_identity() {
    use ccp_core::verification_model::VerificationReportV1 as CoreReport;
    use commit_ci_preflight::verify::VerificationReportV1 as RootReport;
    fn assert_same_type<T>() {}
    assert_same_type::<RootReport>();
    assert_same_type::<CoreReport>();
    let _: fn(
        &[u8],
        &commit_ci_preflight::verify::VerificationPolicyV1,
        &str,
        &str,
    ) -> Result<RootReport, _> = commit_ci_preflight::verify::verify_receipt_document;
    let _: fn(
        &[u8],
        &commit_ci_preflight::verify::VerificationPolicyV1,
        &str,
        &str,
    ) -> Result<CoreReport, _> = ccp_core::verify::verify_receipt_document;

    let root_error = commit_ci_preflight::verify::VerificationError::InvalidExpectedCommit;
    let core_error = ccp_core::errors::VerificationError::InvalidExpectedCommit;
    assert_eq!(root_error.to_string(), core_error.to_string());
    assert_eq!(
        root_error.to_string(),
        "expected commit must be lowercase Git SHA-1 or SHA-256"
    );

    let report = ccp_core::verification_model::VerificationReportV1 {
        schema_version: "1.0".to_owned(),
        assurance_scope: "test".to_owned(),
        evaluated_at_utc: "2026-08-29T00:00:00Z".to_owned(),
        expected_commit: "0".repeat(40),
        receipt_id: None,
        integrity_status: ccp_core::verification_model::VerificationStatus::Fail,
        policy_status: ccp_core::verification_model::VerificationStatus::NotRun,
        decision: ccp_core::verification_model::VerificationDecision::Fail,
        findings: Vec::new(),
    };
    assert_eq!(report.exit_code(), 3);
}

#[test]
fn policy_document_error_source_and_display_are_compatible() {
    use std::error::Error;

    let root_io = commit_ci_preflight::verify::VerificationPolicyDocumentError::Io(
        std::io::Error::other("synthetic"),
    );
    assert!(root_io.source().is_none());
    assert_eq!(root_io.to_string(), "cannot read verification policy");

    let core_io =
        ccp_core::verify::VerificationPolicyDocumentError::Io(std::io::Error::other("synthetic"));
    assert!(core_io.source().is_none());

    let root_v2 = commit_ci_preflight::verify::VerificationPolicyDocumentError::V2(
        commit_ci_preflight::matrix::MatrixError::InvalidReceipt,
    );
    assert!(root_v2.source().is_none());
    let core_v2 = ccp_core::verify::VerificationPolicyDocumentError::V2(
        ccp_core::matrix::MatrixContractError::InvalidReceipt,
    );
    assert!(core_v2.source().is_none());
}
