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
    assert_no_forbidden_sources(&root.join("crates/ccp-verifier"));
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
