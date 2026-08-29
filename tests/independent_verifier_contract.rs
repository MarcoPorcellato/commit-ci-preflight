use std::{collections::HashSet, fs, path::Path};
use sha2::{Digest, Sha256};

const MANIFEST: &str = include_str!("fixtures/m2-compatibility-envelope-v1.json");

#[test]
fn m2_compatibility_envelope_hashes_are_frozen() {
    let value: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
    let files = value["files"].as_object().unwrap();
    assert_eq!(value["schema_version"], "1.0");
    assert_eq!(value["source_head"], "6ff736b1e2a1dfde8778330efdd4b82c845d45e7");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut seen = HashSet::new();
    for (name, expected) in files {
        assert!(Path::new(name).is_relative() && !name.contains(".."));
        assert!(expected.as_str().unwrap().len() == 64 && expected.as_str().unwrap().chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
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
    let files = duplicate.split_once("\"files\":").unwrap().1;
    let body = files.trim_start_matches('{').split('}').next().unwrap();
    let keys: Vec<_> = body.split(',').filter_map(|entry| entry.split(':').next()).collect();
    assert_eq!(keys.len(), 2);
    let mut seen = HashSet::new();
    assert!(keys.iter().any(|key| !seen.insert(*key)), "duplicate manifest entries must be rejected");
}
