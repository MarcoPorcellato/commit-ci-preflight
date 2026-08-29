use std::{collections::HashSet, fs, path::Path};
use sha2::{Digest, Sha256};

const MANIFEST: &str = include_str!("fixtures/m2-compatibility-envelope-v1.json");

#[test]
fn m2_compatibility_envelope_hashes_are_frozen() {
    let value: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
    let files = value["files"].as_object().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut seen = HashSet::new();
    for (name, expected) in files {
        assert!(Path::new(name).is_relative() && !name.contains(".."));
        assert!(seen.insert(name));
        let bytes = fs::read(root.join(name)).unwrap();
        let actual = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(actual, expected.as_str().unwrap(), "{name}");
    }
}
