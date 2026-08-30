# M2 Capability Pack Contract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a bounded, versioned, inert Capability Pack library contract that validates untrusted TOML and expands one explicitly selected profile into CCP's existing `ExecutionPlanEnvelopeV1` without executing project code or changing existing CLI, receipt, policy, or plan bytes.

**Architecture:** A new public `capability_pack` adapter parses a strict TOML manifest, validates pack-specific trust metadata, and delegates all executable configuration validation to an internally constructed schema-`1.3` `ConfigV1`. Validation produces a canonical, privacy-bounded pack envelope plus private validated profile material; expansion requires an explicit repository binding and returns a pack-bound wrapper around the existing plan envelope. M2 is library-first: it does not add a CLI command or any execution path, so the M0 compatibility corpus remains byte-stable.

**Tech Stack:** Rust 1.87+, Serde, TOML, Schemars, serde_json, existing CCP configuration normalization and canonical JSON/SHA-256 helpers, Cargo offline tests.

**Spec:** `docs/superpowers/specs/2026-08-30-capability-packs-clean-architecture-design.md`

## Global Constraints

- Existing CLI syntax/help, exit codes, valid configuration bytes and digests, receipt bytes and IDs, policy schemas, JSON shapes, and public Rust facade remain compatible.
- M2 adds no analyzer-specific dependency, receipt field, policy field, configuration schema version, runtime execution, subprocess, shell invocation, Docker probe, network access, host installation, evidence publication, or mutable-image pull.
- The only accepted manifest serialization in schema `1.0` is UTF-8 TOML; JSON input and dual-format autodetection are out of scope.
- Manifest input is at most `1_048_576` bytes and uses `deny_unknown_fields` at every object boundary.
- A profile expands through schema `1.3` `ConfigV1`; it must use `docker_compatible`, a lowercase `sha256:`-pinned OCI image, `network = false`, `pull_policy = "never"`, `swap_mode = "disabled"`, and an explicit storage policy.
- A profile is selected explicitly by ID. One expansion produces exactly one existing `ExecutionPlanEnvelopeV1`; M2 does not infer a matrix or combine profiles.
- Pack identity is `(pack_id, pack_version, pack_digest)`. Pack-version text is strict `MAJOR.MINOR.PATCH` with three unsigned decimal components and no prefix, whitespace, prerelease, or build suffix. Tool versions are bounded control-free strings because real pinned tools may use date/nightly or vendor version syntax; their source digest remains mandatory.
- License fields accept one bounded SPDX-style identifier, not an expression: 1-128 ASCII alphanumeric, `.`, `-`, or `+`; `NOASSERTION` and `NONE` are rejected. M2 validates syntax, not legal compatibility.
- Every source, tool, rule, type-stub, corpus, advisory, or vulnerability database record uses an `https://` URL without credentials, fragments, controls, or whitespace and a lowercase `sha256:` digest followed by exactly 64 hexadecimal characters.
- Integrity and freshness remain distinct. `advisory-database` and `vulnerability-database` inputs require both `snapshot_created_at_utc` in strict `YYYY-MM-DDTHH:MM:SSZ` form and `max_age_seconds` in `1..=31_536_000`; rules, type stubs, and corpora require neither.
- Pack canonicalization uses the existing `receipt::canonical_json` and `canonical_digest` over the normalized pack only. The pack digest never replaces or enters the execution-plan digest or a receipt.
- Normalized collections are duplicate-free and deterministically sorted by stable ID or enum value before canonicalization.
- Official pack execution remains out of scope until M3. Inspection, validation, schema generation, canonical bytes, and expansion must be demonstrably inert.
- The public repository uses hosted CI as its integration gate. Do not begin Task 1 production implementation until the exact M0-M1-plus-plan head has terminal hosted CI success.
- All production behavior follows RED/GREEN TDD. Every accepted task receives an independent spec-and-quality review before the next task.
- No CCP heavy command, Docker workload, network access, push, PR mutation, merge, stable installation, tag, release, package publication, or evidence publication is authorized by this plan.

---

## File Structure

| Path | Responsibility |
|---|---|
| `src/capability_pack.rs` | Manifest input types, bounds, validation, normalized envelope, canonical digest, inspection, explicit profile binding, inert expansion, typed errors, and focused unit helpers. |
| `src/lib.rs` | Additive public module export only. |
| `tests/capability_pack_contract.rs` | Black-box parser, validator, canonicalization, schema, expansion, and no-execution contract tests. |
| `tests/fixtures/capability-pack-v1/*.toml` | Valid and adversarial manifest inputs. |
| `tests/fixtures/capability-pack-v1/*.json` | Exact canonical inspection and expansion golden bytes. |
| `schema/capability-pack-v1.schema.json` | Pinned generated JSON Schema for the TOML data model. |
| `docs/CAPABILITY_PACKS.md` | Trust boundary, format, inspection/expansion API, evidence classes, limitations, and M3 handoff. |
| `CHANGELOG.md` | Pre-1.0 additive library-contract entry; no claim of executable official packs. |
| `docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/progress.md` | Exact M2 commits, validation evidence, residual risks, and next gate. |

The existing `src/config.rs`, `src/main.rs`, receipt/policy modules, and pinned compatibility fixtures are not modified by M2. Pack validation reuses their public types and `ConfigV1::into_plan`; it does not extract or duplicate execution semantics.

### Task 1: Strict bounded manifest parser and identity contract

**Files:**
- Create: `src/capability_pack.rs`
- Modify: `src/lib.rs`
- Create: `tests/capability_pack_contract.rs`
- Create: `tests/fixtures/capability-pack-v1/valid-minimal.toml`
- Create: `tests/fixtures/capability-pack-v1/unknown-field.toml`
- Create: `tests/fixtures/capability-pack-v1/unknown-version.toml`

**Interfaces:**
- Consumes: existing `RuntimeConfig`, `EnvironmentConfig`, `CacheConfig`, `StorageConfig`, `CheckConfig`, `ReceiptConfig`, `ExecutionPlanEnvelopeV1`, `canonical_json`, and `canonical_digest`.
- Produces: `CAPABILITY_PACK_SCHEMA_VERSION`, `MAX_CAPABILITY_PACK_BYTES`, `CapabilityPackManifestV1::parse`, `CapabilityPackManifestV1::load`, and `CapabilityPackError`.

- [ ] **Step 1: Add the strict manifest fixtures**

Create `valid-minimal.toml` with this complete shape; all SHA-256 values contain exactly 64 lowercase hex characters:

```toml
schema_version = "1.0"
pack_id = "ccp.rust-minimal"
pack_version = "1.0.0"
license = "Apache-2.0"
description = "One inert profile used to prove the Capability Pack contract."

[[upstream_sources]]
id = "pack-source"
url = "https://github.com/MarcoPorcellato/commit-ci-preflight"
digest = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

[[profiles]]
id = "strict-clippy"
description = "Compile all targets with warnings denied."
evidence_class = "deterministic"
pass_semantics = "The exact pinned command exits zero."
known_blind_spots = ["Does not prove dynamic behavior."]
supported_hosts = ["macos-aarch64"]
target_platforms = ["linux-arm64"]
required_runtime_features = ["linux-userland", "no-network", "read-only-source"]
offline_preparation = "none"

[[profiles.tools]]
id = "clippy"
version = "1.87.0"
license = "Apache-2.0"
url = "https://github.com/rust-lang/rust"
digest = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"

[[profiles.inputs]]
id = "rustsec-db"
kind = "advisory-database"
url = "https://github.com/RustSec/advisory-db"
digest = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
snapshot_created_at_utc = "2026-08-30T00:00:00Z"
max_age_seconds = 604800

[profiles.runtime]
kind = "docker_compatible"
image = "ghcr.io/example/rust@sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
cpu_count = 2
memory_mib = 2048
pids_limit = 256
network = false
pull_policy = "never"
swap_mode = "disabled"

[profiles.environment]
fixed = { CARGO_NET_OFFLINE = "true" }

[profiles.storage]
min_free_bytes = 1048576
receipt_journal_reserve_bytes = 4096
max_cache_growth_bytes = 1048576

[[profiles.caches]]
id = "cargo"
mount_path = ".cache/cargo"

[[profiles.checks]]
id = "clippy"
required = true
argv = ["cargo", "clippy", "--locked", "--offline", "--all-targets", "--all-features", "--", "-D", "warnings"]
working_directory = "."
timeout_seconds = 1800
```

Create `unknown-field.toml` by adding `unexpected = true` at the root, and `unknown-version.toml` by changing only `schema_version` to `"2.0"`.

- [ ] **Step 2: Write failing black-box parser tests**

Add tests with these exact names and assertions:

```rust
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
```

- [ ] **Step 3: Run the focused test and capture RED**

Run:

```bash
rtk cargo test --locked --offline --test capability_pack_contract strict_manifest_parser -- --exact
rtk cargo test --locked --offline --test capability_pack_contract manifest_parser_rejects_more_than_one_mebibyte_before_toml_decode -- --exact
```

Expected: compilation fails because the module and types do not exist.

- [ ] **Step 4: Add the minimal strict input model**

Create the module with these public top-level interfaces and private nested input types:

```rust
pub const CAPABILITY_PACK_SCHEMA_VERSION: &str = "1.0";
pub const MAX_CAPABILITY_PACK_BYTES: usize = 1_048_576;
const MAX_PACK_PROFILES: usize = 32;
const MAX_PACK_SOURCES: usize = 16;
const MAX_PROFILE_TOOLS: usize = 32;
const MAX_PROFILE_INPUTS: usize = 64;
const MAX_PROFILE_HOSTS: usize = 8;
const MAX_PROFILE_TARGETS: usize = 8;
const MAX_PROFILE_FEATURES: usize = 16;
const MAX_BLIND_SPOTS: usize = 32;
const MAX_LICENSE_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityPackManifestV1 {
    pub schema_version: String,
    pub pack_id: String,
    pub pack_version: String,
    pub license: String,
    pub description: String,
    pub upstream_sources: Vec<CapabilitySourceV1>,
    pub profiles: Vec<CapabilityProfileConfigV1>,
}

impl CapabilityPackManifestV1 {
    pub fn parse(input: &str) -> Result<Self, CapabilityPackError>;
    pub fn load(path: &Path) -> Result<Self, CapabilityPackError>;
    pub fn validate(self) -> Result<CapabilityPackEnvelopeV1, CapabilityPackError>;
}
```

Every nested manifest object derives `Deserialize` and `JsonSchema` and uses `#[serde(deny_unknown_fields)]`. Task 1 may leave `validate` returning `UnsupportedSchemaVersion` first and a typed `InvalidField("profiles")` for unimplemented semantic validation; Task 2 replaces that temporary branch. `load` checks metadata size before `read_to_string` and records the path in `CapabilityPackError::Io`.

Use exactly these nested input types so later tasks do not invent fields or enum spelling:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilitySourceV1 {
    pub id: String,
    pub url: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityProfileConfigV1 {
    pub id: String,
    pub description: String,
    pub evidence_class: CapabilityEvidenceClassV1,
    pub pass_semantics: String,
    pub known_blind_spots: Vec<String>,
    pub supported_hosts: Vec<CapabilityHostPlatformV1>,
    pub target_platforms: Vec<CapabilityTargetPlatformV1>,
    pub required_runtime_features: Vec<CapabilityRuntimeFeatureV1>,
    pub offline_preparation: OfflinePreparationV1,
    pub tools: Vec<CapabilityToolV1>,
    #[serde(default)]
    pub inputs: Vec<CapabilityInputProvenanceV1>,
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub environment: EnvironmentConfig,
    #[serde(default)]
    pub caches: Vec<CacheConfig>,
    pub storage: StorageConfig,
    pub checks: Vec<CheckConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityToolV1 {
    pub id: String,
    pub version: String,
    pub license: String,
    pub url: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityInputProvenanceV1 {
    pub id: String,
    pub kind: CapabilityInputKindV1,
    pub url: String,
    pub digest: String,
    #[serde(default)]
    pub snapshot_created_at_utc: Option<String>,
    #[serde(default)]
    pub max_age_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityEvidenceClassV1 { Deterministic, ScheduleSensitive, BoundedNondeterministic }

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityInputKindV1 { Rules, TypeStubs, Corpus, AdvisoryDatabase, VulnerabilityDatabase }

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityHostPlatformV1 { MacosAarch64, LinuxArm64, LinuxAmd64, WindowsAmd64 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityTargetPlatformV1 { LinuxArm64, LinuxAmd64 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityRuntimeFeatureV1 {
    LinuxUserland,
    NoNetwork,
    ReadOnlySource,
    WritableCaches,
    BoundedArtifacts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OfflinePreparationV1 { None, RequiredExternal }
```

The fixture value is `offline_preparation = "none"`; future manifests that require a separately reviewed preparation step use `"required-external"`.

Define the complete stable error surface now:

```rust
#[derive(Debug)]
pub enum CapabilityPackError {
    Io { path: PathBuf, source: std::io::Error },
    Parse(toml::de::Error),
    Json(serde_json::Error),
    Receipt(ReceiptError),
    Config(ConfigError),
    UnsupportedSchemaVersion(String),
    ManifestTooLarge { actual: usize, maximum: usize },
    InvalidField(&'static str),
    TooManyItems { field: &'static str, actual: usize, maximum: usize },
    DuplicateId { field: &'static str, id: String },
    DuplicateValue(&'static str),
    ShellEntrypoint(String),
    UnknownProfile(String),
    PackDigestMismatch,
}
```

Implement `Display`, `Error::source`, `From<ConfigError>`, and `From<ReceiptError>` with deterministic text. `canonical_json` and `canonical_digest` failures map to `CapabilityPackError::Receipt`; `serde_json::to_string_pretty` schema-generation failures map to `CapabilityPackError::Json`. Do not include raw manifest contents or fixed environment values in an error.

Add `pub mod capability_pack;` to `src/lib.rs`. Do not modify `src/main.rs`.

- [ ] **Step 5: Run focused GREEN checks**

Run:

```bash
rtk cargo fmt --check
rtk cargo test --locked --offline --test capability_pack_contract strict_manifest_parser_accepts_only_schema_1_0_toml -- --exact
rtk cargo test --locked --offline --test capability_pack_contract manifest_parser_rejects_more_than_one_mebibyte_before_toml_decode -- --exact
rtk cargo clippy --locked --offline --lib --test capability_pack_contract -- -D warnings
```

Expected: all commands PASS; no subprocess, Docker, network, or project command is invoked.

- [ ] **Step 6: Commit the reviewed parser slice**

```bash
rtk git add src/capability_pack.rs src/lib.rs tests/capability_pack_contract.rs tests/fixtures/capability-pack-v1/valid-minimal.toml tests/fixtures/capability-pack-v1/unknown-field.toml tests/fixtures/capability-pack-v1/unknown-version.toml
rtk git commit -m "feat: add strict capability pack parser"
```

### Task 2: Pack-specific trust validation and canonical envelope

**Files:**
- Modify: `src/capability_pack.rs`
- Modify: `tests/capability_pack_contract.rs`
- Create: `tests/fixtures/capability-pack-v1/invalid-image.toml`
- Create: `tests/fixtures/capability-pack-v1/invalid-license.toml`
- Create: `tests/fixtures/capability-pack-v1/invalid-provenance.toml`
- Create: `tests/fixtures/capability-pack-v1/invalid-path.toml`
- Create: `tests/fixtures/capability-pack-v1/shell-entrypoint.toml`
- Create: `tests/fixtures/capability-pack-v1/dependency-cycle.toml`
- Create: `tests/fixtures/capability-pack-v1/valid-minimal-reordered.toml`
- Create: `tests/fixtures/capability-pack-v1/valid-minimal.canonical.json`

**Interfaces:**
- Consumes: Task 1 manifest types; `ConfigV1::into_plan`; `receipt::canonical_json`; `receipt::canonical_digest`; the already-`pub(crate)` `verify::parse_utc_seconds` helper.
- Produces: `CapabilityPackEnvelopeV1`, `NormalizedCapabilityPackV1`, normalized profile/provenance metadata, `canonical_bytes`, `inspection`, and stable typed validation errors.

- [ ] **Step 1: Write the semantic rejection fixtures**

Derive each fixture from `valid-minimal.toml`, changing only the named field:

| Fixture | Exact mutation | Expected field/error |
|---|---|---|
| `invalid-image.toml` | remove `@sha256:<64 hex>` from `profiles.runtime.image` | wrapped `ConfigError::InvalidField("runtime.image")` |
| `invalid-license.toml` | set root license to `"Apache-2.0 OR MIT"` | `InvalidField("license")` |
| `invalid-provenance.toml` | uppercase one hex character in the tool digest | `InvalidField("profiles.tools.digest")` |
| `invalid-path.toml` | set cache mount to `"../cargo"` | wrapped `ConfigError::InvalidField("cache.mount_path")` |
| `shell-entrypoint.toml` | set check argv to `["/bin/sh", "-c", "cargo clippy"]` | `ShellEntrypoint("clippy")` |
| `dependency-cycle.toml` | add a second required check and make both checks depend on each other | wrapped `ConfigError::DependencyCycle` |

- [ ] **Step 2: Write failing semantic, bound, and canonicalization tests**

Add exact tests for:

```rust
const INVALID_IMAGE: &str = include_str!("fixtures/capability-pack-v1/invalid-image.toml");
const INVALID_LICENSE: &str = include_str!("fixtures/capability-pack-v1/invalid-license.toml");
const INVALID_PROVENANCE: &str = include_str!("fixtures/capability-pack-v1/invalid-provenance.toml");
const INVALID_PATH: &str = include_str!("fixtures/capability-pack-v1/invalid-path.toml");
const SHELL_ENTRYPOINT: &str = include_str!("fixtures/capability-pack-v1/shell-entrypoint.toml");
const DEPENDENCY_CYCLE: &str = include_str!("fixtures/capability-pack-v1/dependency-cycle.toml");
const REORDERED_VALID: &str = include_str!("fixtures/capability-pack-v1/valid-minimal-reordered.toml");
const PINNED_CANONICAL: &[u8] = include_bytes!("fixtures/capability-pack-v1/valid-minimal.canonical.json");

fn validate_fixture(source: &str) -> Result<CapabilityPackEnvelopeV1, CapabilityPackError> {
    CapabilityPackManifestV1::parse(source)?.validate()
}

#[test]
fn validator_rejects_untrusted_metadata_and_unsafe_execution_shape() {
    assert!(matches!(validate_fixture(INVALID_LICENSE), Err(CapabilityPackError::InvalidField("license"))));
    assert!(matches!(validate_fixture(INVALID_PROVENANCE), Err(CapabilityPackError::InvalidField("profiles.tools.digest"))));
    assert!(matches!(validate_fixture(SHELL_ENTRYPOINT), Err(CapabilityPackError::ShellEntrypoint(id)) if id == "clippy"));
    assert!(matches!(validate_fixture(INVALID_IMAGE), Err(CapabilityPackError::Config(ConfigError::InvalidField("runtime.image")))));
    assert!(matches!(validate_fixture(INVALID_PATH), Err(CapabilityPackError::Config(ConfigError::InvalidField("cache.mount_path")))));
    assert!(matches!(validate_fixture(DEPENDENCY_CYCLE), Err(CapabilityPackError::Config(ConfigError::DependencyCycle(_)))));
}

#[test]
fn normalized_pack_is_order_independent_and_matches_pinned_canonical_bytes() {
    let first = validate_fixture(VALID).expect("first pack");
    let reordered = validate_fixture(REORDERED_VALID).expect("reordered pack");
    assert_eq!(first.pack_digest, reordered.pack_digest);
    assert_eq!(first.canonical_bytes().expect("canonical bytes"), PINNED_CANONICAL);
}

#[test]
fn freshness_metadata_is_required_as_an_atomic_pair() {
    assert_invalid_freshness(None, Some(86_400));
    assert_invalid_freshness(Some("2026-08-30T00:00:00Z"), None);
    assert_invalid_freshness(Some("2026-02-30T00:00:00Z"), Some(86_400));
}
```

Use these helpers for all in-memory semantic mutations:

```rust
fn valid_manifest() -> CapabilityPackManifestV1 {
    CapabilityPackManifestV1::parse(VALID).expect("valid input model")
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
```

The fixture already contains one `advisory-database` input with a valid pair: ID `rustsec-db`, URL `https://github.com/RustSec/advisory-db`, digest with 64 lowercase `d` characters, timestamp `2026-08-30T00:00:00Z`, and maximum age `604800`.

Add table-driven mutations with these exact expected variants:

| Mutation | Expected error |
|---|---|
| zero profiles or zero upstream sources | `InvalidField("profiles")` or `InvalidField("upstream_sources")` |
| zero tools, hosts, targets, runtime features, or blind spots in one profile | `InvalidField("profiles.tools")`, `InvalidField("profiles.supported_hosts")`, `InvalidField("profiles.target_platforms")`, `InvalidField("profiles.required_runtime_features")`, or `InvalidField("profiles.known_blind_spots")` |
| zero checks or no required check | wrapped `ConfigError::NoChecks` or `ConfigError::NoRequiredChecks` |
| 33 profiles | `TooManyItems { field: "profiles", actual: 33, maximum: 32 }` |
| 17 upstream sources | `TooManyItems { field: "upstream_sources", actual: 17, maximum: 16 }` |
| 33 tools | `TooManyItems { field: "profiles.tools", actual: 33, maximum: 32 }` |
| 65 inputs | `TooManyItems { field: "profiles.inputs", actual: 65, maximum: 64 }` |
| 9 hosts | `TooManyItems { field: "profiles.supported_hosts", actual: 9, maximum: 8 }` |
| 9 targets | `TooManyItems { field: "profiles.target_platforms", actual: 9, maximum: 8 }` |
| 17 runtime features | `TooManyItems { field: "profiles.required_runtime_features", actual: 17, maximum: 16 }` |
| 33 blind spots | `TooManyItems { field: "profiles.known_blind_spots", actual: 33, maximum: 32 }` |
| 129 checks | `Config(ConfigError::TooManyItems { field: "checks", actual: 129, maximum: 128 })` |
| 33 caches | `Config(ConfigError::TooManyItems { field: "caches", actual: 33, maximum: 32 })` |
| 65 argv parts | `Config(ConfigError::InvalidField("check.argv"))` |
| 4,097-byte root description | `InvalidField("description")` |
| invalid pack/profile/source/tool/input stable ID | `InvalidField("pack_id")`, `InvalidField("profiles.id")`, `InvalidField("upstream_sources.id")`, `InvalidField("profiles.tools.id")`, or `InvalidField("profiles.inputs.id")` |
| duplicate profile/source/tool/input IDs | `DuplicateId` with field `profiles.id`, `upstream_sources.id`, `profiles.tools.id`, or `profiles.inputs.id` and the duplicated ID |
| duplicate check/cache IDs | wrapped `ConfigError::DuplicateId` with field `check.id` or `cache.id` |
| duplicate hosts/targets/features/blind spots | `DuplicateValue` with field `profiles.supported_hosts`, `profiles.target_platforms`, `profiles.required_runtime_features`, or `profiles.known_blind_spots` |
| duplicate check dependency/artifact | wrapped `ConfigError::DuplicateValue("check.depends_on")` or `ConfigError::DuplicateValue("check.artifacts")` |
| pack versions `v1.0.0`, `1.0`, `01.0.0`, `1.0.0-alpha` | `InvalidField("pack_version")` |
| empty, control-containing, or 4,097-byte tool version | `InvalidField("profiles.tools.version")` |
| source URL with `http://`, credentials, fragment, whitespace, or control | `InvalidField("upstream_sources.url")` |
| equivalent invalid tool/input URL | `InvalidField("profiles.tools.url")` or `InvalidField("profiles.inputs.url")` |
| root license `NOASSERTION`, `NONE`, `Apache-2.0 OR MIT`, or 129 bytes | `InvalidField("license")` |
| invalid tool license | `InvalidField("profiles.tools.license")` |
| empty root/profile description or pass semantics | `InvalidField("description")`, `InvalidField("profiles.description")`, or `InvalidField("profiles.pass_semantics")` |
| `RuntimeKind::Host` | `InvalidField("profiles.runtime.kind")` |
| `network = true` | `InvalidField("profiles.runtime.network")` |
| absent pull policy or absent swap mode | `Config(ConfigError::MissingRuntimeCapabilityPolicy)` |
| TOML with the complete `[profiles.storage]` table removed | `CapabilityPackError::Parse(_)` |

For list-size cases that require repeated stable IDs, generate unique IDs with `format!("item-{index}")` so the size error is observed before any duplicate error. For the missing-storage case, remove the exact storage table and its three key lines from `VALID` before calling `parse`; do not attempt an impossible in-memory `None` mutation because the input field is non-optional.

`valid-minimal-reordered.toml` contains the same values as `valid-minimal.toml` but places root scalar keys, profile scalar keys, and the `supported_hosts`, `target_platforms`, `required_runtime_features`, and `known_blind_spots` arrays in a different TOML order. It must validate to the same normalized object and pack digest; it does not add or remove a semantic value.

- [ ] **Step 3: Run the semantic suite and capture RED**

Run:

```bash
rtk cargo test --locked --offline --test capability_pack_contract validator_ -- --nocapture
rtk cargo test --locked --offline --test capability_pack_contract normalized_pack_is_order_independent_and_matches_pinned_canonical_bytes -- --exact
rtk cargo test --locked --offline --test capability_pack_contract freshness_metadata_is_required_as_an_atomic_pair -- --exact
```

Expected: FAIL because semantic normalization, envelope types, and canonical methods are absent.

- [ ] **Step 4: Implement pack-specific validation and reuse ConfigV1 normalization**

Use these public normalized interfaces:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapabilityPackEnvelopeV1 {
    pub pack_digest: String,
    pub pack: NormalizedCapabilityPackV1,
    #[serde(skip)]
    profile_configs: BTreeMap<String, ValidatedProfileConfigV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NormalizedCapabilityPackV1 {
    pub schema_version: String,
    pub pack_id: String,
    pub pack_version: String,
    pub license: String,
    pub description: String,
    pub upstream_sources: Vec<CapabilitySourceV1>,
    pub profiles: Vec<NormalizedCapabilityProfileV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NormalizedCapabilityProfileV1 {
    pub id: String,
    pub description: String,
    pub evidence_class: CapabilityEvidenceClassV1,
    pub pass_semantics: String,
    pub known_blind_spots: Vec<String>,
    pub supported_hosts: Vec<CapabilityHostPlatformV1>,
    pub target_platforms: Vec<CapabilityTargetPlatformV1>,
    pub required_runtime_features: Vec<CapabilityRuntimeFeatureV1>,
    pub offline_preparation: OfflinePreparationV1,
    pub tools: Vec<CapabilityToolV1>,
    pub inputs: Vec<CapabilityInputProvenanceV1>,
    pub runtime: NormalizedRuntime,
    pub environment: NormalizedEnvironment,
    pub caches: Vec<NormalizedCache>,
    pub storage: NormalizedStorage,
    pub checks: Vec<NormalizedCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ValidatedProfileConfigV1 {
    evidence_class: CapabilityEvidenceClassV1,
    runtime: RuntimeConfig,
    environment: EnvironmentConfig,
    caches: Vec<CacheConfig>,
    storage: StorageConfig,
    checks: Vec<CheckConfig>,
}

impl CapabilityPackEnvelopeV1 {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CapabilityPackError>;
    pub fn inspection(&self) -> &NormalizedCapabilityPackV1;
}
```

For each profile, first validate all pack-specific fields, collections, and cross-field rules, then construct a private schema-`1.3` `ConfigV1` using:

```rust
ConfigV1 {
    schema_version: "1.3".to_owned(),
    project: "capability-pack/validation".to_owned(),
    runtime: profile.runtime.clone(),
    receipt: ReceiptConfig {
        output: ".ccp/capability-pack-validation.json".to_owned(),
        freshness_seconds: 86_400,
    },
    environment: profile.environment.clone(),
    caches: profile.caches.clone(),
    storage: Some(profile.storage.clone()),
    checks: profile.checks.clone(),
}
.into_plan()
```

Extract the normalized runtime, environment, caches, storage, and checks into `NormalizedCapabilityProfileV1`; do not include the sentinel project or receipt in the pack digest. Retain the validated raw profile inputs privately for later binding. Sort profiles, sources, tools, input provenance, host platforms, target platforms, runtime features, and blind spots deterministically. Reject duplicates before sorting.

The pack-specific validators use these exact rules:

- stable IDs reuse `crate::config::validate_identifier`;
- `description`, `pass_semantics`, and blind spots use the existing 4,096-byte/control-free text bound;
- pack semantic-version components contain ASCII decimal digits, have no leading zero unless the component is exactly `0`, and parse as `u64`; tool versions use only the bounded text rule;
- HTTPS URLs start with `https://`, have a non-empty authority, contain no `@` in the authority, `#`, whitespace, or control character, and are at most 4,096 bytes;
- digests match lowercase `sha256:` plus exactly 64 lowercase hexadecimal characters;
- license identifiers use the Global Constraints rule and reject the two reserved values;
- database kinds require the complete valid freshness pair; all other kinds reject either freshness field; call existing `crate::verify::parse_utc_seconds` and map `None` to `InvalidField("profiles.inputs.snapshot_created_at_utc")`;
- the pack has at least one upstream source and one profile; every profile has at least one tool, one supported host, one target, one runtime feature, one blind spot, one check, and one required check;
- `NoNetwork`, `ReadOnlySource`, and `LinuxUserland` are mandatory runtime features; `WritableCaches` is present if and only if caches are non-empty; `BoundedArtifacts` is present if and only if checks declare artifacts;
- only `DockerCompatible` is accepted; runtime network must be false; pull policy and swap mode are validated by schema `1.3` conversion;
- `RequiredExternal` is metadata only and never performs preparation during validation or expansion.

Reject shell entrypoints by lowercase basename for `sh`, `bash`, `dash`, `zsh`, `ksh`, `fish`, `csh`, `tcsh`, `cmd`, `cmd.exe`, `powershell`, `powershell.exe`, `pwsh`, and `pwsh.exe`; also reject `/usr/bin/env` or `env` when its next non-option token is one of those names. This is a fail-closed entrypoint rule, not a claim to inspect the behavior of arbitrary binaries.

Compute `pack_digest = canonical_digest(&normalized_pack)`. `canonical_bytes` first recomputes that digest and returns `PackDigestMismatch` on mismatch, then canonicalizes the public envelope. Do not include private raw fixed-environment values; the normalized environment already binds their canonical digests.

- [ ] **Step 5: Run GREEN semantic and canonical tests**

Run:

```bash
rtk cargo fmt --check
rtk cargo test --locked --offline --test capability_pack_contract
rtk cargo clippy --locked --offline --lib --test capability_pack_contract -- -D warnings
```

Expected: all Task 1-2 tests PASS and the golden canonical bytes match exactly.

- [ ] **Step 6: Commit the reviewed trust contract**

```bash
rtk git add src/capability_pack.rs tests/capability_pack_contract.rs tests/fixtures/capability-pack-v1
rtk git commit -m "feat: validate capability pack trust metadata"
```

### Task 3: Explicit inert profile expansion

**Files:**
- Modify: `src/capability_pack.rs`
- Modify: `tests/capability_pack_contract.rs`
- Create: `tests/fixtures/capability-pack-v1/valid-minimal.strict-clippy.expansion.json`

**Interfaces:**
- Consumes: Task 2 validated private profile material and existing `ConfigV1::into_plan`.
- Produces: `CapabilityPackBindingV1`, `CapabilityPackExpansionV1`, `CapabilityPackEnvelopeV1::expand`, `CapabilityPackExpansionV1::canonical_bytes`, and explicit unknown-profile failure.

- [ ] **Step 1: Write failing binding, expansion, and inertness tests**

Use these exact public signatures in the tests:

```rust
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

const PINNED_EXPANSION: &[u8] = include_bytes!(
    "fixtures/capability-pack-v1/valid-minimal.strict-clippy.expansion.json"
);
static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityPackBindingV1 {
    pub project: String,
    pub profile_id: String,
    pub receipt: ReceiptConfig,
}

fn valid_binding() -> CapabilityPackBindingV1 {
    CapabilityPackBindingV1 {
        project: "example/project".to_owned(),
        profile_id: "strict-clippy".to_owned(),
        receipt: ReceiptConfig {
            output: ".ccp/receipt.json".to_owned(),
            freshness_seconds: 300,
        },
    }
}

fn binding_for(profile_id: &str) -> CapabilityPackBindingV1 {
    CapabilityPackBindingV1 { profile_id: profile_id.to_owned(), ..valid_binding() }
}

fn binding_for_project(project: &str) -> CapabilityPackBindingV1 {
    CapabilityPackBindingV1 { project: project.to_owned(), ..valid_binding() }
}

fn unique_test_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "ccp-{label}-{}-{}",
        std::process::id(),
        TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed),
    ))
}

fn valid_manifest_with_argv(argv: Vec<String>) -> String {
    let original = "argv = [\"cargo\", \"clippy\", \"--locked\", \"--offline\", \"--all-targets\", \"--all-features\", \"--\", \"-D\", \"warnings\"]";
    let replacement = format!("argv = {}", serde_json::to_string(&argv).expect("argv JSON"));
    assert!(VALID.contains(original));
    VALID.replacen(original, &replacement, 1)
}

#[test]
fn one_explicit_profile_expands_to_one_existing_plan_envelope() {
    let pack = validate_fixture(VALID).expect("pack");
    let expansion = pack.expand(CapabilityPackBindingV1 {
        project: "example/project".to_owned(),
        profile_id: "strict-clippy".to_owned(),
        receipt: ReceiptConfig {
            output: ".ccp/receipt.json".to_owned(),
            freshness_seconds: 300,
        },
    }).expect("expansion");
    assert_eq!(expansion.pack_digest, pack.pack_digest);
    assert_eq!(expansion.profile_id, "strict-clippy");
    assert_eq!(expansion.execution_plan.plan.project, "example/project");
    assert_eq!(expansion.execution_plan.plan.schema_version, "1.3");
    assert_eq!(
        expansion.canonical_bytes().expect("canonical expansion"),
        PINNED_EXPANSION
    );
}

#[test]
fn inspection_and_expansion_never_execute_declared_argv() {
    let root = unique_test_root("pack-inertness");
    std::fs::create_dir_all(&root).expect("create owned test root");
    let marker = root.join("must-not-exist");
    let source = valid_manifest_with_argv(vec![
        "/usr/bin/touch".to_owned(),
        marker.display().to_string(),
    ]);
    let pack = CapabilityPackManifestV1::parse(&source)
        .and_then(CapabilityPackManifestV1::validate)
        .expect("inert pack");
    let _inspection = pack.inspection();
    let _expansion = pack.expand(valid_binding()).expect("inert expansion");
    assert!(!marker.exists());
    std::fs::remove_dir(&root).expect("remove empty owned test root");
}

#[test]
fn expansion_rejects_unknown_profile_and_invalid_repository_binding() {
    let pack = validate_fixture(VALID).expect("pack");
    assert!(matches!(pack.expand(binding_for("missing")), Err(CapabilityPackError::UnknownProfile(id)) if id == "missing"));
    assert!(matches!(pack.expand(binding_for_project("not-a-repository")), Err(CapabilityPackError::Config(ConfigError::InvalidField("project")))));
}
```

- [ ] **Step 2: Run expansion tests and capture RED**

Run:

```bash
rtk cargo test --locked --offline --test capability_pack_contract one_explicit_profile_expands_to_one_existing_plan_envelope -- --exact
rtk cargo test --locked --offline --test capability_pack_contract inspection_and_expansion_never_execute_declared_argv -- --exact
rtk cargo test --locked --offline --test capability_pack_contract expansion_rejects_unknown_profile_and_invalid_repository_binding -- --exact
```

Expected: compilation fails because binding and expansion interfaces do not exist.

- [ ] **Step 3: Implement the minimal explicit expansion adapter**

Add:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapabilityPackExpansionV1 {
    pub schema_version: String,
    pub pack_id: String,
    pub pack_version: String,
    pub pack_digest: String,
    pub profile_id: String,
    pub evidence_class: CapabilityEvidenceClassV1,
    pub execution_plan: ExecutionPlanEnvelopeV1,
}

impl CapabilityPackEnvelopeV1 {
    pub fn expand(
        &self,
        binding: CapabilityPackBindingV1,
    ) -> Result<CapabilityPackExpansionV1, CapabilityPackError>;
}

impl CapabilityPackExpansionV1 {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CapabilityPackError>;
}
```

`expand` must select exactly one validated profile by exact ID and reconstruct schema-`1.3` `ConfigV1` with only the caller's `project` and `receipt` substituted. It then calls `ConfigV1::into_plan`; it does not construct `ExecutionPlanV1` directly. This preserves all current configuration validation and plan-digest semantics. The expansion wrapper binds the pack identity separately; it does not modify the nested execution plan or any receipt contract.

`canonical_bytes` verifies both the pack digest format and `execution_plan.canonical_bytes()` before returning canonical JSON for the wrapper. It must not execute, probe, inspect the filesystem beyond an explicit prior `load`, resolve images, contact a registry, or write output.

- [ ] **Step 4: Run GREEN expansion and compatibility checks**

Run:

```bash
rtk cargo fmt --check
rtk cargo test --locked --offline --test capability_pack_contract
rtk cargo test --locked --offline --test compatibility_baseline
rtk cargo test --locked --offline --test plan_cli
rtk cargo test --locked --offline --test receipt_contract
rtk cargo test --locked --offline --test verification_contract
```

Expected: all tests PASS; compatibility fixtures remain unchanged; the marker does not exist.

- [ ] **Step 5: Commit the reviewed expansion slice**

```bash
rtk git add src/capability_pack.rs tests/capability_pack_contract.rs tests/fixtures/capability-pack-v1/valid-minimal.strict-clippy.expansion.json
rtk git commit -m "feat: expand inert capability pack profiles"
```

### Task 4: Pinned schema and operator documentation

**Files:**
- Modify: `src/capability_pack.rs`
- Modify: `tests/capability_pack_contract.rs`
- Create: `schema/capability-pack-v1.schema.json`
- Create: `docs/CAPABILITY_PACKS.md`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: stable Task 1-3 manifest and expansion types.
- Produces: `capability_pack_schema_json()` and documentation that distinguishes inspection, expansion, execution, evidence integrity, freshness, and qualification.

- [ ] **Step 1: Write the failing pinned-schema test**

```rust
const PINNED_SCHEMA: &str = include_str!("../schema/capability-pack-v1.schema.json");

#[test]
fn generated_capability_pack_schema_matches_pinned_bytes() {
    assert_eq!(
        capability_pack_schema_json().expect("capability pack schema"),
        PINNED_SCHEMA
    );
}
```

Run:

```bash
rtk cargo test --locked --offline --test capability_pack_contract generated_capability_pack_schema_matches_pinned_bytes -- --exact
```

Expected: FAIL because the function and pinned schema are absent.

- [ ] **Step 2: Add schema generation and pin exact bytes**

Implement:

```rust
pub fn capability_pack_schema_json() -> Result<String, CapabilityPackError> {
    let schema = schema_for!(CapabilityPackManifestV1);
    serde_json::to_string_pretty(&schema).map_err(CapabilityPackError::Json)
}
```

Generate the schema from the exact function output, not by hand, and add it at `schema/capability-pack-v1.schema.json`. The test is the drift gate.

- [ ] **Step 3: Document the bounded public contract**

`docs/CAPABILITY_PACKS.md` must contain these explicit sections and claims:

1. `Status: schema and inert library inspection/expansion only; no official pack execution in M2`.
2. TOML schema `1.0` fields and exact bounds.
3. Pack identity tuple and immutable-version expectation.
4. Digest-pinned image and provenance requirements.
5. SPDX-style identifier syntax versus legal review.
6. Integrity versus database/rules freshness.
7. Explicit profile binding and one-profile-to-one-plan expansion.
8. Why expansion is not execution and why no CLI command exists yet.
9. Network disabled, preparation external, source read-only, outputs explicit.
10. Deterministic, schedule-sensitive, and bounded-nondeterministic evidence classes.
11. Known non-goals: no workflow DSL, package manager, tool installer, report interpretation, receipt extension, publication, or policy override.
12. M3 entry criteria for the `rust-deep` reference pack.

Add an `Unreleased` changelog bullet stating that this is a pre-1.0 additive Rust library contract and does not change existing CLI or receipt schemas. Do not claim that users can execute an official pack yet.

- [ ] **Step 4: Run schema, docs, and compatibility checks**

Run:

```bash
rtk cargo fmt --check
rtk cargo test --locked --offline --test capability_pack_contract
rtk cargo test --locked --offline --test compatibility_baseline
rtk git diff --check
rtk rg -n "(TBD|TODO|FIXME|/Users/|container ID|secret value)" docs/CAPABILITY_PACKS.md schema/capability-pack-v1.schema.json
```

Expected: tests and diff check PASS; the bounded documentation scan returns no matches.

- [ ] **Step 5: Commit the reviewed schema and docs**

```bash
rtk git add src/capability_pack.rs tests/capability_pack_contract.rs schema/capability-pack-v1.schema.json docs/CAPABILITY_PACKS.md CHANGELOG.md
rtk git commit -m "docs: define capability pack contract"
```

### Task 5: M2 exact-head qualification and durable closure

**Files:**
- Modify: `docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/progress.md`
- Create: `docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/m2-manifest.json`

**Interfaces:**
- Consumes: accepted Task 1-4 commits and their review reports.
- Produces: terminal local M2 evidence, a deterministic file-hash manifest, explicit residual boundaries, and the exact M3 gate.

- [ ] **Step 1: Run narrow-to-broad offline qualification**

Run in this order:

```bash
rtk cargo fmt --check
rtk cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings
rtk cargo test --locked --offline --test capability_pack_contract
rtk cargo test --locked --offline --test compatibility_baseline
rtk cargo test --locked --offline --test plan_cli
rtk cargo test --locked --offline --test matrix_contract
rtk cargo test --locked --offline --test receipt_contract
rtk cargo test --locked --offline --test verification_contract
rtk cargo test --locked --offline --workspace --all-targets --all-features
```

If the sandbox denies an otherwise ordinary filesystem operation, record the exact denial and repeat only the same command with the narrow host permission allowed by the operator contract. Do not reinterpret a sandbox denial as a product failure or PASS.

- [ ] **Step 2: Prove no compatibility fixture or execution surface changed**

Run:

```bash
rtk cargo test --locked --offline --test compatibility_baseline manifest_matches_the_exact_compatibility_corpus -- --exact
rtk git diff 5fed7c443504969e62980141048f9279f9fa1dfe -- src/main.rs src/config.rs src/matrix.rs src/receipt.rs src/verify.rs schema/config-v1.schema.json schema/config-v2.schema.json schema/receipt-v1.schema.json schema/receipt-v2.schema.json schema/policy-v1.schema.json schema/policy-v1_1.schema.json schema/policy-v2.schema.json
```

Expected: manifest test PASS and the scoped diff is empty. Any non-empty scoped diff blocks M2 closure.

- [ ] **Step 3: Write the failing deterministic M2 manifest test**

Add `m2_manifest_matches_exact_file_bytes` to `tests/capability_pack_contract.rs`. The test parses `m2-manifest.json`, requires schema version `1.0`, requires the exact sorted file set below, rejects duplicates or additional entries, and recomputes each file's byte length and lowercase SHA-256 with the existing `sha2` dev-visible dependency.

Run:

```bash
rtk cargo test --locked --offline --test capability_pack_contract m2_manifest_matches_exact_file_bytes -- --exact
```

Expected: FAIL because `m2-manifest.json` does not exist yet.

- [ ] **Step 4: Create the deterministic M2 hash manifest**

`m2-manifest.json` uses schema version `1.0`, the exact branch HEAD before the closure commit, and sorted entries for:

```text
src/capability_pack.rs
src/lib.rs
tests/capability_pack_contract.rs
tests/fixtures/capability-pack-v1/dependency-cycle.toml
tests/fixtures/capability-pack-v1/invalid-image.toml
tests/fixtures/capability-pack-v1/invalid-license.toml
tests/fixtures/capability-pack-v1/invalid-path.toml
tests/fixtures/capability-pack-v1/invalid-provenance.toml
tests/fixtures/capability-pack-v1/shell-entrypoint.toml
tests/fixtures/capability-pack-v1/unknown-field.toml
tests/fixtures/capability-pack-v1/unknown-version.toml
tests/fixtures/capability-pack-v1/valid-minimal.canonical.json
tests/fixtures/capability-pack-v1/valid-minimal-reordered.toml
tests/fixtures/capability-pack-v1/valid-minimal.strict-clippy.expansion.json
tests/fixtures/capability-pack-v1/valid-minimal.toml
schema/capability-pack-v1.schema.json
docs/CAPABILITY_PACKS.md
CHANGELOG.md
```

Each entry contains `path`, `bytes`, and `sha256:<64 lowercase hex>`. A focused test in `tests/capability_pack_contract.rs` reads the manifest, rejects missing/extra/duplicate/unsorted entries, recomputes byte lengths and hashes, and verifies the exact file set above.

Run the same focused test again. Expected: PASS.

- [ ] **Step 5: Update the durable checkpoint without overstating evidence**

Record exact commits, local test counts, review verdicts, current HEAD, clean/dirty state, and these residual facts:

- M2 provides inert library validation and expansion only.
- No official pack, CLI entry point, tool/image qualification, Docker execution, hosted exact-head result, push, PR update, merge, stable installation, tag, or release is implied.
- M3 must first review the `rust-deep` tool/image/license matrix and design the smallest user entry point without weakening M0 compatibility guarantees.

- [ ] **Step 6: Run independent dual review**

Dispatch one spec-compliance review and one code-quality/security review over the complete M2 range. Both must inspect the task reports and review package. Critical or Important findings enter the SDD fix loop; Minor findings are recorded for final branch review.

- [ ] **Step 7: Commit the closure evidence**

```bash
rtk git add tests/capability_pack_contract.rs docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/m2-manifest.json docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/progress.md
rtk git commit -m "docs: close capability pack contract milestone"
```

- [ ] **Step 8: Stop at the next exact external gate**

Report the final local HEAD, worktree status, complete verification evidence, all reviewer findings/rulings, and the exact non-force push/hosted-CI authorization needed. Do not push, mutate a PR, run CCP, merge, install, tag, or release without that exact authorization.

## Plan Self-Review Checklist

- [ ] Every M2 outcome and exit-evidence requirement in the canonical spec maps to a task above.
- [ ] No task changes existing CLI, receipt, policy, configuration, matrix, or verification bytes.
- [ ] Parser, semantic validation, canonicalization, expansion, schema, inertness, docs, compatibility, and durable closure each have an explicit RED/GREEN or verification step.
- [ ] Every type and method consumed by a later task is produced by an earlier task with the same name and signature.
- [ ] No step contains a placeholder, open-ended implementation instruction, or unbounded research task.
- [ ] M3/M4 tool execution, report interpretation, and release work remain outside this plan.
