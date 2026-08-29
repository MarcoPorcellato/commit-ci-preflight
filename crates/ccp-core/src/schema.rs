//! Stable schema entry points for independent consumers.
use crate::matrix::{MatrixConfigV2, MatrixReceiptEnvelopeV2, MatrixVerificationPolicyV2};
use crate::receipt::ReceiptEnvelopeV2;
use schemars::schema_for;
use serde_json::{Map, Value};

pub fn matrix_config_schema() -> Value {
    serde_json::to_value(schema_for!(MatrixConfigV2)).expect("schema serialization")
}
pub fn matrix_receipt_schema() -> Value {
    serde_json::to_value(schema_for!(MatrixReceiptEnvelopeV2)).expect("schema serialization")
}
pub fn matrix_policy_schema() -> Value {
    serde_json::to_value(schema_for!(MatrixVerificationPolicyV2)).expect("schema serialization")
}
pub fn matrix_policy_schema_json() -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&schema_for!(MatrixVerificationPolicyV2))
}

/// Return the canonical combined v2 schema shared by single-runtime and
/// matrix receipts.
pub fn combined_receipt_v2_schema_json() -> Result<String, serde_json::Error> {
    let matrix = serde_json::to_value(schema_for!(MatrixReceiptEnvelopeV2))?;
    let single_runtime = serde_json::to_value(schema_for!(ReceiptEnvelopeV2))?;
    let mut root = object_field(matrix, "root")?;
    let single_runtime_root = object_field(single_runtime, "single-runtime root")?;

    let mut definitions = object_field(
        root.remove("$defs")
            .ok_or_else(|| missing_schema_field("matrix.$defs"))?,
        "matrix.$defs",
    )?;
    let single_runtime_definitions = object_field(
        single_runtime_root
            .get("$defs")
            .cloned()
            .ok_or_else(|| missing_schema_field("single-runtime.$defs"))?,
        "single-runtime.$defs",
    )?;

    for (name, definition) in single_runtime_definitions {
        match definitions.get(&name) {
            Some(existing) if existing != &definition => {
                return Err(schema_conflict(&name));
            }
            Some(_) => {}
            None => {
                definitions.insert(name, definition);
            }
        }
    }

    let properties = root
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| missing_schema_field("matrix.properties"))?;
    properties.insert(
        "receipt".to_owned(),
        serde_json::json!({
            "oneOf": [
                {"$ref": "#/$defs/MatrixReceiptV2"},
                {"$ref": "#/$defs/ReceiptV2"}
            ]
        }),
    );
    root.insert(
        "title".to_owned(),
        Value::String("ReceiptContractsV2".to_owned()),
    );
    root.insert("$defs".to_owned(), Value::Object(definitions));

    let mut schema = serde_json::to_string_pretty(&Value::Object(root))?;
    schema.push('\n');
    Ok(schema)
}

fn object_field(value: Value, field: &str) -> Result<Map<String, Value>, serde_json::Error> {
    value
        .as_object()
        .cloned()
        .ok_or_else(|| missing_schema_field(field))
}

fn missing_schema_field(field: &str) -> serde_json::Error {
    serde_json::Error::io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("generated schema is missing {field}"),
    ))
}

fn schema_conflict(name: &str) -> serde_json::Error {
    serde_json::Error::io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("generated receipt schemas disagree on {name}"),
    ))
}
