use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::errors::ReceiptError;

pub fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, ReceiptError> {
    let value = serde_json::to_value(value).map_err(ReceiptError::Serialization)?;
    let normalized = normalize_json(value);
    serde_json::to_vec(&normalized).map_err(ReceiptError::Serialization)
}

pub fn canonical_digest<T: Serialize>(value: &T) -> Result<String, ReceiptError> {
    let bytes = canonical_json(value)?;
    let digest = Sha256::digest(bytes);
    Ok(format!("{}{}", "sha256:", encode_hex(&digest)))
}

fn normalize_json(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(normalize_json).collect()),
        Value::Object(items) => {
            let sorted: BTreeMap<_, _> = items
                .into_iter()
                .map(|(key, value)| (key, normalize_json(value)))
                .collect();
            Value::Object(sorted.into_iter().collect())
        }
        scalar => scalar,
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
