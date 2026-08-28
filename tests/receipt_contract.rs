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

use commit_ci_preflight::receipt::{
    ReceiptEnvelopeV1, ReceiptEnvelopeV2, receipt_schema_json, receipt_v2_schema_json,
};

const PASS_FIXTURE: &[u8] = include_bytes!("fixtures/receipt-v1-pass.json");
const PINNED_SCHEMA: &str = include_str!("../schema/receipt-v1.schema.json");
const PASS_V2_FIXTURE: &[u8] = include_bytes!("fixtures/receipt-v2-pass.json");
const PINNED_V2_SCHEMA: &str = include_str!("../schema/receipt-v2.schema.json");

#[test]
fn pinned_pass_fixture_round_trips_byte_for_byte() {
    let envelope: ReceiptEnvelopeV1 =
        serde_json::from_slice(PASS_FIXTURE).expect("pinned fixture parses");
    envelope.verify().expect("pinned fixture verifies");

    assert_eq!(
        envelope.canonical_bytes().expect("canonical bytes"),
        PASS_FIXTURE
    );
}

#[test]
fn explicit_producer_version_is_sealed() {
    let envelope: ReceiptEnvelopeV1 =
        serde_json::from_slice(PASS_FIXTURE).expect("pinned fixture parses");

    assert_eq!(envelope.receipt.producer.name, "commit-ci-preflight");
    assert_eq!(envelope.receipt.producer.version, "0.1.0");
    envelope.verify().expect("producer version remains sealed");
}

#[test]
fn generated_schema_matches_pinned_contract_byte_for_byte() {
    assert_eq!(
        receipt_schema_json().expect("generated schema"),
        PINNED_SCHEMA
    );
}

#[test]
fn fixture_contains_no_wall_clock_or_random_placeholder() {
    let fixture = std::str::from_utf8(PASS_FIXTURE).expect("UTF-8 fixture");
    for forbidden in ["now", "random", "localhost", "/Users/", "token", "secret"] {
        assert!(
            !fixture
                .to_ascii_lowercase()
                .contains(&forbidden.to_ascii_lowercase()),
            "fixture contains forbidden nondeterministic or sensitive marker: {forbidden}"
        );
    }
}

#[test]
fn pinned_v2_pass_fixture_round_trips_byte_for_byte() {
    let envelope: ReceiptEnvelopeV2 =
        serde_json::from_slice(PASS_V2_FIXTURE).expect("pinned fixture parses");
    envelope.verify().expect("pinned fixture verifies");

    assert_eq!(
        envelope.canonical_bytes().expect("canonical bytes"),
        PASS_V2_FIXTURE
    );
}

#[test]
fn v2_receipt_requires_a_normalized_execution_plan() {
    let mut document: serde_json::Value =
        serde_json::from_slice(PASS_V2_FIXTURE).expect("fixture JSON");
    document["receipt"]
        .as_object_mut()
        .expect("receipt object")
        .remove("execution_plan");

    assert!(serde_json::from_value::<ReceiptEnvelopeV2>(document).is_err());
}

#[test]
fn generated_v2_schema_matches_pinned_contract_byte_for_byte() {
    assert_eq!(
        receipt_v2_schema_json().expect("generated schema"),
        PINNED_V2_SCHEMA
    );
}

#[test]
fn v2_schema_exposes_optional_runtime_capability_evidence() {
    let schema: serde_json::Value =
        serde_json::from_str(PINNED_V2_SCHEMA).expect("pinned schema parses");
    let properties = &schema["$defs"]["ReceiptV2"]["properties"];

    assert_eq!(
        properties["runtime_capability_evidence"]["anyOf"][0]["$ref"],
        "#/$defs/RuntimeCapabilityEvidenceV1"
    );
    assert_eq!(
        properties["runtime_capability_evidence"]["anyOf"][1]["type"],
        "null"
    );
    assert!(
        !schema["$defs"]["ReceiptV2"]["required"]
            .as_array()
            .expect("required fields")
            .iter()
            .any(|field| field == "runtime_capability_evidence")
    );
}

#[test]
fn shared_v2_schema_accepts_both_receipt_families() {
    let schema: serde_json::Value =
        serde_json::from_str(PINNED_V2_SCHEMA).expect("pinned schema parses");
    let alternatives = schema["properties"]["receipt"]["oneOf"]
        .as_array()
        .expect("receipt alternatives");
    let refs: Vec<_> = alternatives
        .iter()
        .filter_map(|alternative| alternative["$ref"].as_str())
        .collect();

    assert_eq!(refs, vec!["#/$defs/MatrixReceiptV2", "#/$defs/ReceiptV2"]);
    assert!(schema["$defs"]["MatrixReceiptV2"].is_object());
    assert!(schema["$defs"]["ReceiptV2"].is_object());
}

#[test]
fn v2_fixture_contains_no_placeholders_or_environment_values() {
    let fixture = std::str::from_utf8(PASS_V2_FIXTURE).expect("UTF-8 fixture");
    for forbidden in ["now", "random", "localhost", "/Users/", "DEPLOY_TOKEN"] {
        assert!(
            !fixture
                .to_ascii_lowercase()
                .contains(&forbidden.to_ascii_lowercase()),
            "fixture contains forbidden nondeterministic or sensitive marker: {forbidden}"
        );
    }
    let envelope: ReceiptEnvelopeV2 = serde_json::from_slice(PASS_V2_FIXTURE).expect("fixture");
    assert!(envelope.receipt.execution_plan.environment.fixed.is_empty());
    assert!(
        envelope
            .receipt
            .execution_plan
            .environment
            .remote_secret_only
            .is_empty()
    );
}
