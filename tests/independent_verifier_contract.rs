use serde::{
    Deserializer,
    de::{self, MapAccess, Visitor},
};
use sha2::{Digest, Sha256};
use std::fmt;
use std::{collections::HashSet, fs, path::Path};

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
