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
    CAPABILITY_PACK_SCHEMA_VERSION, CapabilityInputKindV1, CapabilityPackEnvelopeV1,
    CapabilityPackError, CapabilityPackManifestV1, CapabilityRuntimeFeatureV1,
    MAX_CAPABILITY_PACK_BYTES,
};
use commit_ci_preflight::config::{ConfigError, RuntimeKind};

const VALID: &str = include_str!("fixtures/capability-pack-v1/valid-minimal.toml");
const UNKNOWN_FIELD: &str = include_str!("fixtures/capability-pack-v1/unknown-field.toml");
const UNKNOWN_VERSION: &str = include_str!("fixtures/capability-pack-v1/unknown-version.toml");
const INVALID_IMAGE: &str = include_str!("fixtures/capability-pack-v1/invalid-image.toml");
const INVALID_LICENSE: &str = include_str!("fixtures/capability-pack-v1/invalid-license.toml");
const INVALID_PROVENANCE: &str =
    include_str!("fixtures/capability-pack-v1/invalid-provenance.toml");
const INVALID_PATH: &str = include_str!("fixtures/capability-pack-v1/invalid-path.toml");
const SHELL_ENTRYPOINT: &str = include_str!("fixtures/capability-pack-v1/shell-entrypoint.toml");
const DEPENDENCY_CYCLE: &str = include_str!("fixtures/capability-pack-v1/dependency-cycle.toml");
const REORDERED_VALID: &str =
    include_str!("fixtures/capability-pack-v1/valid-minimal-reordered.toml");
const PINNED_CANONICAL: &[u8] =
    include_bytes!("fixtures/capability-pack-v1/valid-minimal.canonical.json");

fn validate_fixture(source: &str) -> Result<CapabilityPackEnvelopeV1, CapabilityPackError> {
    CapabilityPackManifestV1::parse(source)?.validate()
}

fn valid_manifest() -> CapabilityPackManifestV1 {
    CapabilityPackManifestV1::parse(VALID).expect("valid input model")
}

fn assert_invalid_field(
    mutate: impl FnOnce(&mut CapabilityPackManifestV1),
    expected: &'static str,
) {
    let mut manifest = valid_manifest();
    mutate(&mut manifest);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::InvalidField(field)) if field == expected
    ));
}

fn assert_invalid_freshness(created: Option<&str>, max_age_seconds: Option<u64>) {
    let mut manifest = valid_manifest();
    let input = manifest.profiles[0]
        .inputs
        .first_mut()
        .expect("valid fixture database input");
    input.snapshot_created_at_utc = created.map(str::to_owned);
    input.max_age_seconds = max_age_seconds;
    let expected = if created.is_some() && max_age_seconds.is_some() {
        "profiles.inputs.snapshot_created_at_utc"
    } else {
        "profiles.inputs.freshness"
    };
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::InvalidField(field)) if field == expected
    ));
}

#[test]
fn strict_manifest_parser_accepts_only_schema_1_0_toml() {
    let manifest = CapabilityPackManifestV1::parse(VALID).expect("valid manifest");
    assert_eq!(manifest.schema_version, CAPABILITY_PACK_SCHEMA_VERSION);
    assert_eq!(manifest.pack_id, "ccp.rust-minimal");
    assert!(CapabilityPackManifestV1::parse(UNKNOWN_FIELD).is_err());
    assert!(matches!(
        CapabilityPackManifestV1::parse(UNKNOWN_VERSION)
            .and_then(CapabilityPackManifestV1::validate),
        Err(CapabilityPackError::UnsupportedSchemaVersion(version)) if version == "2.0"
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

#[test]
fn validator_rejects_untrusted_metadata_and_unsafe_execution_shape() {
    assert!(matches!(
        validate_fixture(INVALID_LICENSE),
        Err(CapabilityPackError::InvalidField("license"))
    ));
    assert!(matches!(
        validate_fixture(INVALID_PROVENANCE),
        Err(CapabilityPackError::InvalidField("profiles.tools.digest"))
    ));
    assert!(
        matches!(validate_fixture(SHELL_ENTRYPOINT), Err(CapabilityPackError::ShellEntrypoint(id)) if id == "clippy")
    );
    assert!(matches!(
        validate_fixture(INVALID_IMAGE),
        Err(CapabilityPackError::Config(ConfigError::InvalidField(
            "runtime.image"
        )))
    ));
    assert!(matches!(
        validate_fixture(INVALID_PATH),
        Err(CapabilityPackError::Config(ConfigError::InvalidField(
            "cache.mount_path"
        )))
    ));
    assert!(matches!(
        validate_fixture(DEPENDENCY_CYCLE),
        Err(CapabilityPackError::Config(ConfigError::DependencyCycle(_)))
    ));
}

#[test]
fn validator_rejects_shells_invoked_through_env() {
    let mut manifest = valid_manifest();
    manifest.profiles[0].checks[0].argv = vec![
        "/usr/bin/env".to_owned(),
        "-i".to_owned(),
        "bash".to_owned(),
        "-c".to_owned(),
        "cargo clippy".to_owned(),
    ];
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::ShellEntrypoint(id)) if id == "clippy"
    ));
}

#[test]
fn validator_rejects_shell_entrypoint_bypasses() {
    for argv in [
        vec![
            "env".to_owned(),
            "X=1".to_owned(),
            "bash".to_owned(),
            "-c".to_owned(),
            "cargo clippy".to_owned(),
        ],
        vec![
            "env".to_owned(),
            "-S".to_owned(),
            "bash -c cargo clippy".to_owned(),
        ],
        vec![
            "C:\\Windows\\System32\\cmd.exe".to_owned(),
            "/c".to_owned(),
            "cargo clippy".to_owned(),
        ],
    ] {
        let mut manifest = valid_manifest();
        manifest.profiles[0].checks[0].argv = argv;
        assert!(matches!(
            manifest.validate(),
            Err(CapabilityPackError::ShellEntrypoint(id)) if id == "clippy"
        ));
    }
}

#[test]
fn normalized_pack_is_order_independent_and_matches_pinned_canonical_bytes() {
    let first = validate_fixture(VALID).expect("first pack");
    let reordered = validate_fixture(REORDERED_VALID).expect("reordered pack");
    assert_eq!(first.pack_digest, reordered.pack_digest);
    assert_eq!(first.inspection(), reordered.inspection());
    assert_eq!(
        first.canonical_bytes().expect("canonical bytes"),
        PINNED_CANONICAL
    );
}

#[test]
fn freshness_metadata_is_required_as_an_atomic_pair() {
    assert_invalid_freshness(None, Some(86_400));
    assert_invalid_freshness(Some("2026-08-30T00:00:00Z"), None);
    assert_invalid_freshness(Some("2026-02-30T00:00:00Z"), Some(86_400));
}

#[test]
fn validator_enforces_pack_and_profile_collection_bounds() {
    assert_invalid_field(|m| m.profiles.clear(), "profiles");
    assert_invalid_field(|m| m.upstream_sources.clear(), "upstream_sources");
    assert_invalid_field(|m| m.profiles[0].tools.clear(), "profiles.tools");
    assert_invalid_field(
        |m| m.profiles[0].supported_hosts.clear(),
        "profiles.supported_hosts",
    );
    assert_invalid_field(
        |m| m.profiles[0].target_platforms.clear(),
        "profiles.target_platforms",
    );
    assert_invalid_field(
        |m| m.profiles[0].required_runtime_features.clear(),
        "profiles.required_runtime_features",
    );
    assert_invalid_field(
        |m| m.profiles[0].known_blind_spots.clear(),
        "profiles.known_blind_spots",
    );

    let mut manifest = valid_manifest();
    let profile = manifest.profiles[0].clone();
    manifest.profiles = (0..33)
        .map(|index| {
            let mut value = profile.clone();
            value.id = format!("profile-{index}");
            value
        })
        .collect();
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::TooManyItems {
            field: "profiles",
            actual: 33,
            maximum: 32
        })
    ));
    let mut manifest = valid_manifest();
    let source = manifest.upstream_sources[0].clone();
    manifest.upstream_sources = (0..17)
        .map(|index| {
            let mut value = source.clone();
            value.id = format!("source-{index}");
            value
        })
        .collect();
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::TooManyItems {
            field: "upstream_sources",
            actual: 17,
            maximum: 16
        })
    ));
    let mut manifest = valid_manifest();
    let tool = manifest.profiles[0].tools[0].clone();
    manifest.profiles[0].tools = (0..33)
        .map(|index| {
            let mut value = tool.clone();
            value.id = format!("tool-{index}");
            value
        })
        .collect();
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::TooManyItems {
            field: "profiles.tools",
            actual: 33,
            maximum: 32
        })
    ));
    let mut manifest = valid_manifest();
    let input = manifest.profiles[0].inputs[0].clone();
    manifest.profiles[0].inputs = (0..65)
        .map(|index| {
            let mut value = input.clone();
            value.id = format!("input-{index}");
            value
        })
        .collect();
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::TooManyItems {
            field: "profiles.inputs",
            actual: 65,
            maximum: 64
        })
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].supported_hosts = vec![manifest.profiles[0].supported_hosts[0]; 9];
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::TooManyItems {
            field: "profiles.supported_hosts",
            actual: 9,
            maximum: 8
        })
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].target_platforms = vec![manifest.profiles[0].target_platforms[0]; 9];
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::TooManyItems {
            field: "profiles.target_platforms",
            actual: 9,
            maximum: 8
        })
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].required_runtime_features =
        vec![CapabilityRuntimeFeatureV1::NoNetwork; 17];
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::TooManyItems {
            field: "profiles.required_runtime_features",
            actual: 17,
            maximum: 16
        })
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].known_blind_spots = (0..33).map(|i| format!("blind-{i}")).collect();
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::TooManyItems {
            field: "profiles.known_blind_spots",
            actual: 33,
            maximum: 32
        })
    ));
}

#[test]
fn validator_reuses_embedded_config_collection_checks() {
    let mut manifest = valid_manifest();
    manifest.profiles[0].checks.clear();
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(ConfigError::NoChecks))
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].checks[0].required = false;
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(ConfigError::NoRequiredChecks))
    ));
    let mut manifest = valid_manifest();
    let check = manifest.profiles[0].checks[0].clone();
    manifest.profiles[0].checks = (0..129)
        .map(|index| {
            let mut value = check.clone();
            value.id = format!("check-{index}");
            value
        })
        .collect();
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(ConfigError::TooManyItems {
            field: "checks",
            actual: 129,
            maximum: 128
        }))
    ));
    let mut manifest = valid_manifest();
    let cache = manifest.profiles[0].caches[0].clone();
    manifest.profiles[0].caches = (0..33)
        .map(|index| {
            let mut value = cache.clone();
            value.id = format!("cache-{index}");
            value.mount_path = format!(".cache/{index}");
            value
        })
        .collect();
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(ConfigError::TooManyItems {
            field: "caches",
            actual: 33,
            maximum: 32
        }))
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].checks[0].argv = vec!["command".to_owned(); 65];
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(ConfigError::InvalidField(
            "check.argv"
        )))
    ));
}

#[test]
fn validator_rejects_malformed_metadata_and_duplicates() {
    assert_invalid_field(|m| m.description = "x".repeat(4097), "description");
    assert_invalid_field(|m| m.pack_id = "bad id".to_owned(), "pack_id");
    assert_invalid_field(|m| m.profiles[0].id = "bad id".to_owned(), "profiles.id");
    assert_invalid_field(
        |m| m.upstream_sources[0].id = "bad id".to_owned(),
        "upstream_sources.id",
    );
    assert_invalid_field(
        |m| m.profiles[0].tools[0].id = "bad id".to_owned(),
        "profiles.tools.id",
    );
    assert_invalid_field(
        |m| m.profiles[0].inputs[0].id = "bad id".to_owned(),
        "profiles.inputs.id",
    );
    let mut manifest = valid_manifest();
    let value = manifest.profiles[0].clone();
    manifest.profiles.push(value);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::DuplicateId {
            field: "profiles.id",
            ..
        })
    ));
    let mut manifest = valid_manifest();
    let value = manifest.upstream_sources[0].clone();
    manifest.upstream_sources.push(value);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::DuplicateId {
            field: "upstream_sources.id",
            ..
        })
    ));
    let mut manifest = valid_manifest();
    let value = manifest.profiles[0].tools[0].clone();
    manifest.profiles[0].tools.push(value);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::DuplicateId {
            field: "profiles.tools.id",
            ..
        })
    ));
    let mut manifest = valid_manifest();
    let value = manifest.profiles[0].inputs[0].clone();
    manifest.profiles[0].inputs.push(value);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::DuplicateId {
            field: "profiles.inputs.id",
            ..
        })
    ));
    let mut manifest = valid_manifest();
    let value = manifest.profiles[0].checks[0].clone();
    manifest.profiles[0].checks.push(value);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(ConfigError::DuplicateId {
            field: "check.id",
            ..
        }))
    ));
    let mut manifest = valid_manifest();
    let value = manifest.profiles[0].caches[0].clone();
    manifest.profiles[0].caches.push(value);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(ConfigError::DuplicateId {
            field: "cache.id",
            ..
        }))
    ));
}

#[test]
fn validator_rejects_duplicate_values_and_runtime_feature_contract_breaks() {
    let mut manifest = valid_manifest();
    let value = manifest.profiles[0].supported_hosts[0];
    manifest.profiles[0].supported_hosts.push(value);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::DuplicateValue(
            "profiles.supported_hosts"
        ))
    ));
    let mut manifest = valid_manifest();
    let value = manifest.profiles[0].target_platforms[0];
    manifest.profiles[0].target_platforms.push(value);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::DuplicateValue(
            "profiles.target_platforms"
        ))
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0]
        .required_runtime_features
        .push(CapabilityRuntimeFeatureV1::NoNetwork);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::DuplicateValue(
            "profiles.required_runtime_features"
        ))
    ));
    let mut manifest = valid_manifest();
    let value = manifest.profiles[0].known_blind_spots[0].clone();
    manifest.profiles[0].known_blind_spots.push(value);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::DuplicateValue(
            "profiles.known_blind_spots"
        ))
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].checks[0].depends_on = vec!["clippy".to_owned(), "clippy".to_owned()];
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(ConfigError::DuplicateValue(
            "check.depends_on"
        )))
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].checks[0].artifacts = vec!["artifact".to_owned(), "artifact".to_owned()];
    manifest.profiles[0]
        .required_runtime_features
        .push(CapabilityRuntimeFeatureV1::BoundedArtifacts);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(ConfigError::DuplicateValue(
            "check.artifacts"
        )))
    ));
    assert_invalid_field(
        |m| {
            m.profiles[0].required_runtime_features.pop();
        },
        "profiles.required_runtime_features",
    );
    let mut manifest = valid_manifest();
    manifest.profiles[0].checks[0].artifacts = vec!["artifact".to_owned()];
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::InvalidField(
            "profiles.required_runtime_features"
        ))
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].caches.clear();
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::InvalidField(
            "profiles.required_runtime_features"
        ))
    ));
    assert_invalid_field(
        |m| m.profiles[0].runtime.kind = RuntimeKind::Host,
        "profiles.runtime.kind",
    );
    assert_invalid_field(
        |m| m.profiles[0].runtime.network = true,
        "profiles.runtime.network",
    );
}

#[test]
fn validator_enforces_value_syntax_and_input_freshness() {
    for value in ["v1.0.0", "1.0", "01.0.0", "1.0.0-alpha"] {
        assert_invalid_field(|m| m.pack_version = value.to_owned(), "pack_version");
    }
    assert_invalid_field(
        |m| m.profiles[0].tools[0].version.clear(),
        "profiles.tools.version",
    );
    assert_invalid_field(
        |m| m.profiles[0].tools[0].version = "bad\u{0000}".to_owned(),
        "profiles.tools.version",
    );
    assert_invalid_field(
        |m| m.profiles[0].tools[0].version = "x".repeat(4097),
        "profiles.tools.version",
    );
    for value in [
        "http://example.com",
        "https://user@example.com",
        "https://example.com/#fragment",
        "https://example.com/space here",
        "https://example.com/\u{0000}",
    ] {
        assert_invalid_field(
            |m| m.upstream_sources[0].url = value.to_owned(),
            "upstream_sources.url",
        );
    }
    assert_invalid_field(
        |m| m.profiles[0].tools[0].url = "http://example.com".to_owned(),
        "profiles.tools.url",
    );
    assert_invalid_field(
        |m| m.profiles[0].inputs[0].url = "http://example.com".to_owned(),
        "profiles.inputs.url",
    );
    for value in ["NOASSERTION", "NONE", "Apache-2.0 OR MIT"] {
        assert_invalid_field(|m| m.license = value.to_owned(), "license");
    }
    assert_invalid_field(|m| m.license = "x".repeat(129), "license");
    assert_invalid_field(
        |m| m.profiles[0].tools[0].license = "Apache-2.0 OR MIT".to_owned(),
        "profiles.tools.license",
    );
    assert_invalid_field(|m| m.description.clear(), "description");
    assert_invalid_field(
        |m| m.profiles[0].description.clear(),
        "profiles.description",
    );
    assert_invalid_field(
        |m| m.profiles[0].pass_semantics.clear(),
        "profiles.pass_semantics",
    );
    assert_invalid_freshness(None, Some(86_400));
    assert_invalid_field(
        |m| m.profiles[0].inputs[0].max_age_seconds = Some(0),
        "profiles.inputs.max_age_seconds",
    );
    let mut manifest = valid_manifest();
    manifest.profiles[0].inputs[0].kind = CapabilityInputKindV1::Rules;
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::InvalidField(
            "profiles.inputs.freshness"
        ))
    ));
}

#[test]
fn parser_requires_explicit_storage_and_schema_13_runtime_policy() {
    let without_storage = VALID.replacen("[profiles.storage]\nmin_free_bytes = 1048576\nreceipt_journal_reserve_bytes = 4096\nmax_cache_growth_bytes = 1048576\n\n", "", 1);
    assert!(matches!(
        CapabilityPackManifestV1::parse(&without_storage),
        Err(CapabilityPackError::Parse(_))
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].runtime.pull_policy = None;
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(
            ConfigError::MissingRuntimeCapabilityPolicy
        ))
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].runtime.swap_mode = None;
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(
            ConfigError::MissingRuntimeCapabilityPolicy
        ))
    ));
}

#[test]
fn canonical_bytes_rejects_tampered_digest() {
    let mut envelope = validate_fixture(VALID).expect("valid envelope");
    envelope.pack_digest = "sha256:bad".to_owned();
    assert!(matches!(
        envelope.canonical_bytes(),
        Err(CapabilityPackError::PackDigestMismatch)
    ));
}
