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

use commit_ci_preflight::receipt::{ReceiptEnvelopeV1, receipt_schema_json};

const PASS_FIXTURE: &[u8] = include_bytes!("fixtures/receipt-v1-pass.json");
const PINNED_SCHEMA: &str = include_str!("../schema/receipt-v1.schema.json");

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
