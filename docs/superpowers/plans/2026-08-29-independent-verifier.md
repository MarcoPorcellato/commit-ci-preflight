# M2 Physically Independent Verifier Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract CCP's receipt and policy verification trust root into `ccp-core` and ship a dedicated `ccp-verifier` binary without changing existing receipt bytes, schemas, public Rust paths, root CLI output, or exit behavior.

**Architecture:** Keep `commit-ci-preflight` as the root workspace package and compatibility facade. Define protocol and verification types once in `ccp-core`; make the root runner and thin `ccp-verifier` binary depend inward on that crate. Split pure Matrix contract/verification code from Matrix execution before moving the verification dispatcher.

**Tech Stack:** Rust edition 2024, MSRV 1.87, Cargo resolver 3 workspace, Clap, Serde, Schemars, SHA-256, TOML, golden JSON/TOML/schema fixtures.

**Spec:** `docs/superpowers/specs/2026-08-29-independent-verifier-design.md`

## Global Constraints

- All shell commands begin with `rtk`.
- Work only in `/Users/marco1/Documents/CODICE con VS CODE/ccp-worktrees/independent-verifier-v1`, based on exact source `6ff736b1e2a1dfde8778330efdd4b82c845d45e7`.
- Use TDD for every behavior or boundary change: focused RED, minimal GREEN, focused review, then proportional validation.
- Preserve all existing receipt, policy, report, schema, canonical-byte, public-error, root-CLI, and exit-code behavior.
- Approved Task 4 exception: pure Matrix methods change their pre-1.0 Rust
  error return from root `MatrixError` to core `MatrixContractError`. Root
  `MatrixError` variants/displays, CLI output/exit codes, and all wire/schema
  bytes remain unchanged through an exhaustive root adapter.
- Define every protocol type once. Compatibility uses `pub use`, never duplicated look-alike structs or enums.
- `ccp-core` direct dependencies are limited to `schemars`, `serde`, `serde_json`, `sha2`, and `toml`.
- `ccp-verifier` normal dependencies are limited to `ccp-core` and narrowly
  configured `clap`; `serde_json` is allowed only as a dev-dependency for CLI
  output assertions.
- The verifier dependency closure must exclude the root package, Docker/runtime, process, cache, admission, resource, benchmark, GitHub migration, `ctrlc`, `fs2`, `process-wrap`, `nix`, `saphyr`, and `saphyr-parser`.
- Keep `verify-benchmark`, runner config/plan commands, GitHub Actions migration, publication, and all heavy execution out of `ccp-verifier`.
- User-visible changes update `CHANGELOG.md`.
- Local commits, push, PR, CCP, evidence publication, ready transition, and merge remain separate gates.
- Deterministic validation precedes any Docker, network, native, CCP, or publication action.
- Full workspace suites and release builds remain behind the exact live-host
  `guard exec` authorization required by the global CCP operator contract.

---

### Task 1: Freeze the M2 compatibility envelope

**Files:**
- Create: `tests/fixtures/m2-compatibility-envelope-v1.json`
- Create: `tests/independent_verifier_contract.rs`
- Create: `tests/public_api_compat_contract.rs`
- Modify: `tests/receipt_contract.rs`
- Modify: `tests/verification_contract.rs`

**Interfaces:**
- Consumes: existing fixture/schema files and root public modules.
- Produces: `M2_COMPATIBILITY_ENVELOPE_V1`, a checked-in hash manifest and executable pre-extraction baseline.

- [ ] **Step 1: Add the fixture/hash manifest**

  Record the exact SHA-256 values inventoried at the base head, including:

  ```json
  {
    "schema_version": "1.0",
    "source_head": "6ff736b1e2a1dfde8778330efdd4b82c845d45e7",
    "files": {
      "tests/fixtures/receipt-v1-pass.json": "f6824b28ded398d26620f5cc53c8e891997f909ff83b6ff7e17eef2ca821b017",
      "tests/fixtures/receipt-v2-pass.json": "35d15b81c281f13fe07a49cf20a1aa942ab07f3e8b2ddbd400d9747e37eba8fe",
      "tests/fixtures/policy-v1.toml": "21d0f45765ba8bc4ea1f443ad0fa76093cf5edeb7b98f181e50e34052ca8bac3",
      "tests/fixtures/policy-v1_1-trusted-plan.toml": "f9c3293ba7ddeb100a85680d1b931b2246621fcc62314dffd06c4a4e16d8ca79",
      "tests/fixtures/policy-v1_1-trusted-plan-altered.toml": "39798543fa24b779b415472188d588f3901f142b216424af4f340275bd4e36d3",
      "tests/fixtures/policy-v1_1-missing-config.toml": "f30f7caea1cd37bd3bcd206abfbc70b8041df2ff7a0b437d17d01f7cfd9fffa1",
      "tests/fixtures/policy-v2-legacy-compatible.toml": "846a94bd6b298ca1a35a4c0f39bed31ae08d1b94e448fa82fc058ea16550afec",
      "tests/fixtures/plan-v2-current-default.stdout.json": "6c86ecad1b7c213945656282aa1f2844909302a13e1a585c3a2c93be6081458a",
      "tests/fixtures/historical-verifier-044697.provenance.json": "cd75dea3593307e6dbf72abe5773e3031056a3c61033caeba6c7bea2a4a79838",
      "tests/fixtures/matrix-v2-legacy-plan-044697.provenance.json": "dc6f4f409fe31003cf1f50fe895ca7a114a614ca4eb3c7f526da02981abd9e1f",
      "schema/receipt-v1.schema.json": "684b81b685a6013181e3fdd4bcf29573c01959da5e510bc158d5f94b8aeb8abc",
      "schema/receipt-v2.schema.json": "bf315a2609366adc94520f5e8966d3048529e86759e66701a544be10eca9adbf",
      "schema/policy-v1.schema.json": "e76d92f8a328c714ace3aab8c7c88e1ef58c4092ca48961fd2e57ca347b53d79",
      "schema/policy-v1_1.schema.json": "b06ee6753cab21db0a4cca5bfbdbd90ec2cdeccd2a7d2eaaed68b826d0944b00",
      "schema/policy-v2.schema.json": "2cc87c4bbfdbe787144a4ff96ff2a20dd004525f704f9f1cbd80117225426316",
      "schema/verification-report-v1.schema.json": "67ec9524749d471d6a23e22ba0f9d52377273448f5c25ef318fbbd1a9f8f9b23"
    }
  }
  ```

- [ ] **Step 2: Add a green baseline hash test**

  Parse the manifest, hash each repository-relative file with `sha2::Sha256`,
  and assert exact equality. Reject absolute paths and duplicate entries.
  Negative fixtures not listed above remain behavior-pinned by the explicit
  malformed/oversized/stale/mutation tests rather than byte-pinned.

- [ ] **Step 3: Freeze root API/error behavior**

  Add compile assertions that import the frozen receipt, normalized-plan,
  policy, report, Matrix, and error families from `commit_ci_preflight`.
  Record an explicit path/type matrix in the test. Pattern-match every adopted
  public error variant and assert the adopted `Display`, `source()`, and
  `Send + Sync` behavior for all constructible cases. This task does not import
  `ccp_core`; cross-crate type identity begins only after Task 2 adds that
  package as a root dev-dependency.

- [ ] **Step 4: Run the focused baseline**

  ```console
  rtk cargo test --locked --test independent_verifier_contract --test public_api_compat_contract --test receipt_contract --test verification_contract
  ```

  Expected: PASS on the pre-extraction root package.

- [ ] **Step 5: Request the local commit gate**

  Proposed commit: `test: freeze verifier compatibility envelope`.

### Task 2: Introduce the workspace and dependency-boundary RED/GREEN cycle

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Create: `crates/ccp-core/Cargo.toml`
- Create: `crates/ccp-core/src/lib.rs`
- Create: `crates/ccp-verifier/Cargo.toml`
- Create: `crates/ccp-verifier/src/main.rs`
- Modify: `tests/independent_verifier_contract.rs`

**Interfaces:**
- Consumes: Task 1 compatibility manifest.
- Produces: workspace members named `ccp-core` and `ccp-verifier`; no verifier behavior yet.

- [ ] **Step 1: Write the failing workspace-boundary test**

  Add assertions equivalent to:

  ```rust
  assert_eq!(workspace_manifest_paths(), [
      "Cargo.toml",
      "crates/ccp-core/Cargo.toml",
      "crates/ccp-verifier/Cargo.toml",
  ]);
  assert_eq!(workspace_default_member_names(), ["commit-ci-preflight"]);
  assert_eq!(workspace_resolver(), "3");
  assert_allowed_dependencies("crates/ccp-core/Cargo.toml", CORE_ALLOWED);
  assert_allowed_dependencies("crates/ccp-verifier/Cargo.toml", VERIFIER_ALLOWED);
  assert_no_forbidden_source_imports("crates/ccp-verifier");
  ```

- [ ] **Step 2: Run the test and verify RED**

  ```console
  rtk cargo test --locked --test independent_verifier_contract workspace_members_are_explicit_and_verifier_dependencies_are_bounded -- --exact
  ```

  Expected: FAIL because the two package manifests do not exist.

- [ ] **Step 3: Add minimal package scaffolding**

  Keep the root `[package]`; add explicit workspace members/default member and
  `resolver = "3"`. Add `ccp-core = { path = "crates/ccp-core" }` to the root
  dev-dependencies so later cross-path integration tests name the core crate.
  Both new packages use `edition = "2024"` and `rust-version = "1.87"`. The
  temporary verifier binary prints no success claim; invoking it returns usage
  exit `2` until Task 6 adds commands.

- [ ] **Step 4: Run focused GREEN and metadata inspection**

  ```console
  rtk cargo test --locked --test independent_verifier_contract workspace_members_are_explicit_and_verifier_dependencies_are_bounded -- --exact
  rtk cargo metadata --locked --format-version 1 --no-deps
  ```

  Expected: PASS; member-only metadata lists exactly the root, `ccp-core`, and
  `ccp-verifier` packages and resolves the default member to the root manifest.
  No dependency-closure conclusion is drawn from `--no-deps`.

- [ ] **Step 5: Request the local commit gate**

  Proposed commit: `refactor: establish verifier workspace boundary`.

### Task 3: Extract the protocol nucleus into `ccp-core`

**Files:**
- Create: `crates/ccp-core/src/canonical.rs`
- Create: `crates/ccp-core/src/errors.rs`
- Create: `crates/ccp-core/src/config.rs`
- Create: `crates/ccp-core/src/runtime_evidence.rs`
- Create: `crates/ccp-core/src/receipt.rs`
- Modify: `crates/ccp-core/src/lib.rs`
- Replace with compatibility facades: `src/config.rs`, `src/receipt.rs`
- Modify: `src/runtime.rs`
- Modify: `tests/independent_verifier_contract.rs`

**Interfaces:**
- Consumes: existing config/plan and receipt wire definitions.
- Produces: one nominal definition for config, normalized plan, receipt,
  canonicalization, `ReceiptError`, and `RuntimeCapabilityEvidenceV1`.

- [ ] **Step 1: Add failing type-identity assertions**

  Import the same type through both paths and require assignment without
  conversion:

  ```rust
  fn root_to_core(value: commit_ci_preflight::receipt::ReceiptEnvelopeV2)
      -> ccp_core::receipt::ReceiptEnvelopeV2 { value }

  fn core_to_root(value: ccp_core::config::ExecutionPlanV1)
      -> commit_ci_preflight::config::ExecutionPlanV1 { value }
  ```

- [ ] **Step 2: Verify RED**

  Run the focused test. Expected: compilation fails because `ccp_core` does not
  yet expose these definitions.

- [ ] **Step 3: Move the cyclic contract cluster together**

  Define `ReceiptError` once in core `errors`; re-export it from core/root
  `receipt`. Make both `canonical` and `config` depend on `errors`, never on the
  receipt module. Move canonical JSON/digest, config parse/validate/normalize
  types, receipt models/validation, and the runtime capability evidence wire
  type into the core. Keep Docker probes and runtime adapters in root. Replace root
  modules with explicit `pub use ccp_core::...::*`; re-export the capability
  evidence from root `runtime`.

- [ ] **Step 4: Prove GREEN and byte compatibility**

  ```console
  rtk cargo test --locked --test independent_verifier_contract --test public_api_compat_contract --test plan_cli --test receipt_contract --test matrix_contract
  ```

  Expected: PASS with unchanged fixture/schema hashes, Matrix still compiling
  against the re-exported receipt/plan types, and cross-path type assignment.

- [ ] **Step 5: Request the local commit gate**

  Proposed commit: `refactor: extract canonical receipt core`.

### Task 4: Separate Matrix contracts from Matrix execution

**Files:**
- Create: `crates/ccp-core/src/verification_model.rs`
- Create: `crates/ccp-core/src/matrix.rs`
- Create: `crates/ccp-core/src/matrix_legacy.rs`
- Create: `crates/ccp-core/src/schema.rs`
- Modify: `crates/ccp-core/src/lib.rs`
- Modify: `src/matrix.rs`
- Remove after migration: `src/matrix_legacy.rs`, `src/schema_contract.rs`
- Modify: `tests/matrix_contract.rs`, `tests/receipt_contract.rs`

**Interfaces:**
- Consumes: core plan/receipt types and root Matrix behavior.
- Produces: core-owned Matrix config/plan/receipt/policy/legacy digest and
  verification-model primitives; root Matrix retains execution only.

- [ ] **Step 1: Write the failing Matrix dependency test**

  Assert that `crates/ccp-core/src/matrix.rs` contains no imports or tokens for
  `cache`, `process`, `run`, `runtime`, `source_snapshot`, `workspace`, Docker,
  or admission. Require cross-path identity for `MatrixConfigV2`,
  `MatrixPlanEnvelopeV2`, `MatrixReceiptEnvelopeV2`, `MatrixReceiptV2`,
  `MatrixVerificationPolicyV2`, and the shared verification model types.
  Separately require exhaustive construction and conversion of every
  `MatrixContractError` variant into the existing root `MatrixError`; the two
  error types must not be nominally identical.

- [ ] **Step 2: Verify RED**

  Expected: FAIL because Matrix contract types still live in runner-owned
  `src/matrix.rs`.

- [ ] **Step 3: Establish the complete temporary verification-model boundary**

  Move `AcceptedPlatformV1`, `VerificationStatus`, `VerificationDecision`,
  `VerificationFindingV1`, `VerificationReportV1`, `finding`,
  `parse_utc_seconds`, and `validate_commit` into core `verification_model`.
  Also move the nominal `PolicyError`, `TrustedPlanError`, and
  `VerificationError` enums into core `errors` now, because pure Matrix
  verification needs `VerificationError` without a core-to-root dependency;
  Task 5 still owns moving their policy/dispatcher implementations.
  Re-export the currently public model types from root `verify` immediately;
  keep `finding`, `parse_utc_seconds`, and `validate_commit` `pub(crate)` inside
  `ccp-core` because only core Matrix verification consumes them. This gives
  Matrix a one-way dependency before the policy dispatcher moves in Task 5
  without expanding the public API.

- [ ] **Step 4: Move pure Matrix code**

  Move config normalization, plan sealing, receipt sealing, policy parsing,
  legacy digest compatibility, Matrix receipt verification, and schema
  assembly into core. These pure APIs return `MatrixContractError`, which
  contains no `RuntimeError` or `RunError`. The root Matrix module retains the
  existing public `MatrixError` and
  `MatrixRunOutcomeV2`, `MatrixRunMaterialV2`, `MatrixRunRequestV2`,
  `execute_matrix_run_v2`, `seal_matrix_run_material`, and
  `write_matrix_receipt`, plus source snapshots, cache, process execution,
  runtime probes, receipt writing, and run lifecycle. Add one exhaustive
  `From<MatrixContractError> for MatrixError` adapter preserving exact root
  `Display` behavior. In the same GREEN slice, change those root execution
  functions to import the new core contract and verification-model paths; no
  import of the old root verifier remains.

- [ ] **Step 5: Prove Matrix and schema parity**

  ```console
  rtk cargo test --locked --test matrix_contract --test receipt_contract --test independent_verifier_contract
  ```

  Expected: PASS; Matrix mutation/ordering/legacy fixtures and combined receipt
  schema remain byte-identical; root CLI/exit behavior is unchanged. Add a
  schema dependency assertion proving that core schema generation cannot import
  runner Matrix execution code, plus exhaustive adapter tests proving every
  pure core error maps to the intended unchanged root error variant.

- [ ] **Step 6: Request the local commit gate**

  Proposed commit: `refactor: isolate matrix verification contracts`.

### Task 5: Extract policy dispatch and verification into the core

**Files:**
- Create: `crates/ccp-core/src/verify.rs`
- Modify: `crates/ccp-core/src/verification_model.rs`
- Replace with compatibility facade: `src/verify.rs`
- Modify: `tests/verification_contract.rs`
- Modify: `tests/github_gate_contract.rs`
- Modify: `tests/independent_verifier_contract.rs`

**Interfaces:**
- Consumes: core V1/V2/Matrix receipt and policy contracts.
- Produces: core `verify_receipt_document*`, policy loading/validation,
  trusted-plan reconstruction, reports, and errors; root facade preserves paths.

- [ ] **Step 1: Add the failing root/core verification identity test**

  Require that root and core verification functions accept the same policy and
  return the same nominal `VerificationReportV1`. Add representative exact
  `Display` and exit-decision assertions.

- [ ] **Step 2: Verify RED**

  Expected: FAIL because core has no verification dispatcher.

- [ ] **Step 3: Move bounded verification logic**

  Move policy document dispatch, strict bounded file/byte parsing, V1/V1.1 and
  Matrix verification, report construction, evaluation-time parsing, and
  trusted-config reconstruction. Do not move CLI rendering, benchmark
  verification, GitHub migration, or runner filesystem mutation.

  `ccp-core::verify` must expose `validate_verification_policy_path`,
  `system_evaluated_at_utc`, `verify_receipt_document`,
  `verify_receipt_document_for_policy`,
  `verify_receipt_document_for_policy_path`, and
  `receipt_input_failure_report`. `VerificationReportV1::canonical_bytes`
  remains the only runtime JSON serialization path used by the dedicated CLI.

- [ ] **Step 4: Run the complete verifier fixture matrix**

  ```console
  rtk cargo test --locked --test verification_contract --test github_gate_contract --test independent_verifier_contract
  ```

  Expected: PASS for positive, stale, wrong-head, malformed, oversized,
  unknown-field, missing-input, trusted-plan, revoked-producer, and legacy
  Matrix cases.

- [ ] **Step 5: Request the local commit gate**

  Proposed commit: `refactor: extract policy verification core`.

### Task 6: Add the dedicated `ccp-verifier` CLI with parity tests

**Files:**
- Modify: `crates/ccp-verifier/src/main.rs`
- Create: `crates/ccp-verifier/tests/verify_cli.rs`
- Create: `crates/ccp-verifier/tests/schema_cli.rs`
- Modify: `tests/verify_cli.rs`
- Modify: `tests/independent_verifier_contract.rs`

**Interfaces:**
- Consumes: core verification and schema APIs.
- Produces: exactly `verify` and `schema` subcommands; exit codes `0`, `2`, `3`,
  and `70` with root-compatible output.

- [ ] **Step 1: Add failing old/new parity tests**

  For each positive and negative fixture, run both binaries with the same
  explicit `--evaluated-at-utc`; assert exact stdout bytes and exit code.
  Normalize nothing. Stderr must match for stable errors and may differ only in
  the executable name inside Clap-generated usage text.

- [ ] **Step 2: Add failing schema inventory tests**

  Require the following exact mapping and compare stdout directly with each
  checked-in file. Reject unknown kinds with exit `2`.

  | Kind | File |
  |---|---|
  | `receipt-v1` | `schema/receipt-v1.schema.json` |
  | `receipt-v2` | `schema/receipt-v2.schema.json` |
  | `policy-v1` | `schema/policy-v1.schema.json` |
  | `policy-v1-1` | `schema/policy-v1_1.schema.json` |
  | `policy-v2` | `schema/policy-v2.schema.json` |
  | `verification-report-v1` | `schema/verification-report-v1.schema.json` |

- [ ] **Step 3: Verify RED**

  ```console
  rtk cargo test --locked -p ccp-verifier
  ```

  Expected: FAIL because the temporary binary exposes no commands.

- [ ] **Step 4: Implement the thin CLI**

  Parse arguments with narrowly configured Clap, call only `ccp_core::verify`
  and `ccp_core::schema`, render reports exactly like the root CLI, and preserve
  failure mapping. Do not add runner imports or commands. Add a manifest/source
  assertion that production `ccp-verifier` has no `serde_json` dependency or
  import; JSON bytes come from core APIs.

- [ ] **Step 5: Prove GREEN and absence of extra commands**

  ```console
  rtk cargo test --locked -p ccp-verifier
  rtk cargo test --locked --test verify_cli --test independent_verifier_contract
  ```

  Expected: PASS; `ccp-verifier --help` contains `verify` and `schema` and none
  of `run`, `plan`, `doctor`, `dry-run`, `benchmark`, `guard`, or `migrate`.

- [ ] **Step 6: Request the local commit gate**

  Proposed commit: `feat: add independent receipt verifier`.

### Task 7: Prove physical dependency and public API independence

**Files:**
- Modify: `tests/independent_verifier_contract.rs`
- Create: `tests/fixtures/verifier-dependency-policy-v1.json`
- Create: `tests/fixtures/cargo-metadata-verifier-pass-v1.json`
- Create: `tests/fixtures/cargo-metadata-verifier-forbidden-v1.json`
- Modify: `Cargo.lock`

**Interfaces:**
- Consumes: completed crate split.
- Produces: machine-readable allowed/forbidden dependency policy and terminal
  Cargo graph evidence.

- [ ] **Step 1: Add the dependency-policy fixture**

  Record separate rules: exact allowed direct dependencies, explicit forbidden
  transitive package names/IDs, and allowed registry/path source classes.
  Platform-conditional normal dependencies are traversed and checked; unrelated
  workspace lockfile packages are ignored.

- [ ] **Step 2: Add the failing resolved-graph assertion**

  In the integration-test helper, parse synthetic PASS and forbidden metadata
  fixtures first. Then run `cargo metadata --locked --format-version 1`, locate
  the package whose name is exactly `ccp-verifier`, resolve its package ID to a
  node in `resolve.nodes`, and recursively walk only `deps` entries having at
  least one `dep_kinds` item with `kind == null`. Fail on the root package,
  forbidden package IDs/names, invalid sources, or an unexpected direct edge.
  Do not treat unrelated packages elsewhere in the workspace lockfile as linked.

- [ ] **Step 3: Run graph proof and correct only real leakage**

  ```console
  rtk cargo tree --locked -p ccp-verifier --edges normal,no-dev,no-build
  rtk cargo tree --locked -p ccp-verifier --depth 1
  rtk cargo metadata --locked --format-version 1
  rtk cargo test --locked --test independent_verifier_contract
  ```

  Expected: only the designed verifier closure is reachable; no root package or
  runner-only dependency appears. The parsed graph is controlling evidence;
  `cargo tree` is human-readable corroboration. Review exported signatures and
  every `pub use` path separately because metadata cannot prove API exposure.

- [ ] **Step 4: Prepare the bounded size-evidence gate**

  Prepare the exact `cargo build --locked --release -p ccp-verifier` guard
  envelope and record it in the local report, but do not execute it in this
  task. The release build runs in Task 9 only after the complete deterministic
  workspace suite has passed. Do not enforce an arbitrary threshold or claim
  another platform.

- [ ] **Step 5: Request the local commit gate**

  Proposed commit: `test: enforce verifier dependency isolation`.

### Task 8: Reconcile documentation and release metadata

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `docs/PRODUCT_ROADMAP.md`
- Modify: `docs/RELIABILITY_HARDENING_PLAN.md`
- Modify: `docs/ARCHITECTURE.md`
- Modify: `docs/INSTALLATION.md`
- Modify: `docs/BETA_SUPPORT.md`
- Modify: `README.md` only if a current command path must be documented
- Modify: release metadata sources generated by `examples/generate_release_metadata.rs`

**Interfaces:**
- Consumes: verified crate/binary behavior.
- Produces: truthful M2 documentation using `ccp-core` consistently and keeping
  availability, qualification, distribution, and identity separate.

- [ ] **Step 1: Write documentation contract failures**

  Extend repository/release hardening tests to require the actual workspace
  graph, verifier command scope, rollback/non-replacement warning, and explicit
  M3 deferral of static/multi-platform distribution.

- [ ] **Step 2: Verify RED**

  ```console
  rtk cargo test --locked --test repository_hygiene_contract --test release_hardening_contract
  ```

- [ ] **Step 3: Update documentation minimally**

  Replace the older `ccp-contract` name with canonical `ccp-core`; document
  local source-build use of `ccp-verifier` without claiming a published binary;
  record that `verify-benchmark` remains in the root CLI and static MUSL work is
  M3. Add the user-visible M2 entry to `CHANGELOG.md`.

- [ ] **Step 4: Prove docs and metadata GREEN**

  ```console
  rtk cargo test --locked --test repository_hygiene_contract --test release_hardening_contract
  rtk cargo run --locked --quiet --example generate_release_metadata -- --check
  ```

- [ ] **Step 5: Request the local commit gate**

  Proposed commit: `docs: document independent verifier boundary`.

### Task 9: Full deterministic qualification and review closure

**Files:**
- Modify only findings proven by review; no opportunistic refactor.
- Create ignored local report under `.superpowers/sdd/2026-08-29-independent-verifier/`.

**Interfaces:**
- Consumes: Tasks 1-8.
- Produces: exact-head deterministic evidence and a hash-bound CCP authorization envelope.

- [ ] **Step 1: Run formatting and focused compatibility checks**

  ```console
  rtk cargo fmt --all -- --check
  rtk cargo test --locked --test independent_verifier_contract --test receipt_contract --test verification_contract --test matrix_contract --test verify_cli
  ```

- [ ] **Step 2: Run full deterministic workspace gates**

  First materialize the exact launcher/argv, worktree HEAD, stable CCP path and
  SHA-256, resource profile, timeout, maximum guard count, and stop boundary;
  stop for the required `guard exec` authorization. Within that authorized
  heavy-work envelope run:

  ```console
  rtk cargo test --locked --workspace --all-targets --all-features
  rtk cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
  rtk cargo doc --locked --workspace --no-deps
  rtk cargo run --locked --quiet --example generate_release_metadata -- --check
  rtk cargo build --locked --release -p ccp-verifier
  ```

- [ ] **Step 3: Re-run dependency and hash evidence**

  ```console
  rtk cargo tree --locked -p ccp-verifier --edges normal,no-dev,no-build
  rtk cargo metadata --locked --format-version 1
  rtk shasum -a 256 tests/fixtures/receipt-v1-pass.json tests/fixtures/receipt-v2-pass.json schema/receipt-v1.schema.json schema/receipt-v2.schema.json schema/policy-v1.schema.json schema/policy-v1_1.schema.json schema/verification-report-v1.schema.json
  rtk shasum -a 256 target/release/ccp-verifier
  rtk stat -f '%z' target/release/ccp-verifier
  rtk rustc -vV
  ```

- [ ] **Step 4: Perform task-scoped and whole-branch review**

  Review crate ownership, type identity, compatibility, trust-boundary leakage,
  error/CLI parity, fixture/schema bytes, docs claims, and the complete diff from
  base `6ff736b1e2a1dfde8778330efdd4b82c845d45e7`.

- [ ] **Step 5: Freeze the exact-head CCP envelope and stop**

  Record worktree, complete HEAD, clean status, stable CCP absolute path and
  complete SHA-256, configuration digest, generation, maximum run count `1`,
  expected receipt path, preserved journal boundary, and stop point. Do not run
  CCP until the operator issues that exact authorization.

## Plan completion audit

- [ ] Every design requirement maps to one task and one terminal check.
- [ ] No task duplicates protocol types or weakens root compatibility.
- [ ] No verifier dependency reaches runner/runtime/state code.
- [ ] No static, native, distribution, identity, publication, or savings claim
      exceeds M2 evidence.
- [ ] Source changes, local commits, heavy qualification, and remote actions are
      separately authorized and reported.
