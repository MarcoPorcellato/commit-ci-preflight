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

//! The canonical v2 receipt schema shared by single-runtime and matrix receipts.
//!
//! The two receipt families remain separate strict Rust contracts. This module
//! only composes their generated JSON Schema documents so one pinned public
//! schema cannot silently invalidate either family during a staged upgrade.

use schemars::schema_for;
use serde_json::{Map, Value};

use crate::matrix::MatrixReceiptEnvelopeV2;
use crate::receipt::ReceiptEnvelopeV2;

pub(crate) fn combined_receipt_v2_schema_json() -> Result<String, serde_json::Error> {
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
