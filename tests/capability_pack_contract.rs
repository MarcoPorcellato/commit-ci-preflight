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

use commit_ci_preflight::capability_pack::{
    CAPABILITY_PACK_SCHEMA_VERSION, CapabilityPackError, CapabilityPackManifestV1,
    MAX_CAPABILITY_PACK_BYTES,
};

const VALID: &str = include_str!("fixtures/capability-pack-v1/valid-minimal.toml");
const UNKNOWN_FIELD: &str = include_str!("fixtures/capability-pack-v1/unknown-field.toml");
const UNKNOWN_VERSION: &str = include_str!("fixtures/capability-pack-v1/unknown-version.toml");

#[test]
fn strict_manifest_parser_accepts_only_schema_1_0_toml() {
    let manifest = CapabilityPackManifestV1::parse(VALID).expect("valid manifest");
    assert_eq!(manifest.schema_version, CAPABILITY_PACK_SCHEMA_VERSION);
    assert_eq!(manifest.pack_id, "ccp.rust-minimal");
    assert!(CapabilityPackManifestV1::parse(UNKNOWN_FIELD).is_err());
    assert!(matches!(
        CapabilityPackManifestV1::parse(UNKNOWN_VERSION)
            .and_then(CapabilityPackManifestV1::validate),
        Err(CapabilityPackError::UnsupportedSchemaVersion(version))
            if version == "2.0"
    ));
}

#[test]
fn manifest_parser_rejects_more_than_one_mebibyte_before_toml_decode() {
    let oversized = "x".repeat(MAX_CAPABILITY_PACK_BYTES + 1);
    assert!(matches!(
        CapabilityPackManifestV1::parse(&oversized),
        Err(CapabilityPackError::ManifestTooLarge { actual, maximum })
            if actual == MAX_CAPABILITY_PACK_BYTES + 1 && maximum == MAX_CAPABILITY_PACK_BYTES
    ));
}
