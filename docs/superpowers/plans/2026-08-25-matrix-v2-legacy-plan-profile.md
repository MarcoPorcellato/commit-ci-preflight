# Matrix V2 Legacy Plan Profile Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an explicit `matrix-v2-legacy-v1` plan profile that lets the current CCP producer reproduce historical Matrix V2 outer and per-runtime digests while retaining the current coordinator, runtime, recovery, cache, and verification safeguards.

**Architecture:** Keep the current Matrix plan as the only executable representation. A focused `matrix_legacy` module projects representable current normalized values into the exact historical serialization shape and derives all compatibility digests from canonical bytes. A profile-bearing Matrix envelope owns the current plan plus the optional immutable legacy basis; all four configuration-consuming commands use one builder, and runtime/receipt boundaries revalidate the projection before execution and sealing.

**Tech Stack:** Rust 2024, Rust 1.87 minimum, Clap 4.6, Serde/serde_json, SHA-256 through the existing canonical receipt helpers, existing Matrix V2 and strict verifier contracts.

**Spec:** `docs/superpowers/specs/2026-08-25-matrix-v2-legacy-plan-profile-design.md`

## Global Constraints

- Baseline is `2b4b55ce1a4be0a2b610656ae4a56a7641b29f26`; historical plan authority is `044697dee9a0d678d30a4847d62ddf9b4970505b` with tree `5220164edf17831ce0c42dae1c14300ed1045015`.
- `current-v2` is the default and its CLI JSON, human output, digests, receipts, and execution semantics remain byte-for-byte unchanged.
- `matrix-v2-legacy-v1` is accepted only for configuration schema `2.0`; it is never inferred from policy, receipt, repository, filename, or digest.
- Production code contains no Latent-TRIZ expected digest constant or repository-to-digest lookup.
- The current normalized plan is the only runtime input; legacy structures are digest bases, never executable plans.
- Every current-only semantic field omitted by the historical shape has an explicit representability check; unknown or non-default semantics fail closed before admission or mutation.
- Matrix receipts remain outer schema `2.0` with inner schema `1.0` receipts. No policy, config, receipt, admission, or cache schema version changes are allowed.
- The legacy profile emits producer version `0.1.0+matrix-v2-legacy-v1`; current profile remains `0.1.0`.
- Legacy digests deliberately select a distinct managed-cache namespace; current cache entries are not relabelled or promoted.
- `plan` is runtime-free, `doctor` runs probes only, `dry-run` spawns no project process, and only `run` acquires admission and executes checks.
- No new dependency is introduced.
- Every production change follows a witnessed RED-GREEN test cycle.
- Every user-visible change updates `CHANGELOG.md`; the accepted architecture decision remains recorded in `docs/adr/0005-matrix-v2-legacy-plan-profile.md`.
- Every shell command in this repository begins with `rtk`.

---

## File map

- Create `src/matrix_legacy.rs`: historical private serialization types, representability proof, canonical legacy bases, and derived digest accessors.
- Modify `src/lib.rs`: expose the focused compatibility module to the Matrix implementation and integration tests.
- Modify `src/matrix.rs`: closed profile enum, profile-bearing envelope, shared builder, invariant rechecks, runtime-envelope conversion, and receipt provenance agreement.
- Modify `src/run.rs`: accept an explicit already-validated producer version at receipt sealing instead of reading the package version inside the seal path.
- Modify `src/main.rs`: add one shared Clap value enum/argument to `plan`, `doctor`, `dry-run`, and `run`; route all Matrix command paths through the same profile-aware builder; preserve default output bytes.
- Create `tests/fixtures/config-v2-legacy-compatible.toml`: generic two-runtime Matrix input valid at historical and current producers.
- Create `tests/fixtures/matrix-v2-legacy-plan-044697.json`: raw historical canonical plan output with provenance sidecar fields kept outside the hashed plan document.
- Create `tests/fixtures/config-v2-legacy-nonrepresentable-*.toml`: negative current-only semantic fixtures if parser-reachable; otherwise construct typed mutations in unit tests.
- Modify `tests/matrix_contract.rs`: projection, digest, mutation, ordering, tamper, receipt, producer, and verifier contracts.
- Modify `tests/plan_cli.rs`: CLI parsing, current byte stability, legacy disclosure, and single-runtime rejection.
- Modify `tests/runtime_cli.rs`: doctor/dry-run digest parity and non-execution behavior.
- Modify `tests/receipt_contract.rs`: explicit producer version sealing and current default stability.
- Modify `tests/verification_contract.rs` and `tests/verify_cli.rs`: historical Matrix verifier acceptance and fail-closed mutations.
- Modify `tests/repository_hygiene_contract.rs`: prohibit adopter digest constants in production source and require compatibility documentation.
- Modify `docs/CONFIGURATION.md`, `docs/LOCAL_RUN.md`, `docs/MULTI_RUNTIME_RECEIPTS.md`, `docs/RECEIPT_SPEC.md`, `docs/GITHUB_GATE.md`, `docs/ADOPTION_GUIDE.md`, `docs/CACHE_AND_WORKSPACE.md`, `docs/TROUBLESHOOTING.md`, `docs/INVARIANT_EVIDENCE_MATRIX.md`, `docs/TESTING_AND_FAULT_INJECTION.md`, and `CHANGELOG.md`: operator contract, boundaries, evidence, adoption, and failure diagnostics.

---

### Task 1: Independent historical golden fixture

**Files:**
- Create: `tests/fixtures/config-v2-legacy-compatible.toml`
- Create: `tests/fixtures/matrix-v2-legacy-plan-044697.json`
- Create: `tests/fixtures/matrix-v2-legacy-plan-044697.provenance.json`
- Test: `tests/matrix_contract.rs`

**Interfaces:**
- Consumes: historical source `044697dee9a0d678d30a4847d62ddf9b4970505b` and its ordinary `plan --json` command.
- Produces: immutable historical canonical JSON fixture and `legacy_compatible_config() -> &'static str` test input for Tasks 2-7.

- [ ] **Step 1: Add the generic compatibility configuration**

Use schema `2.0`, project `example/legacy-matrix`, receipt `.ccp/receipt.json`, inherited `SOURCE_DATE_EPOCH`, one cache `cargo` at `.cache/cargo`, two digest-pinned Docker-compatible runtimes `python311` and `python312`, and one required `python -V` check bound to each runtime. Use only fields accepted by both exact producers.

- [ ] **Step 2: Materialize the historical plan fixture independently**

Build the exact historical source in an isolated `/private/tmp` checkout and run only the read-only plan command:

```bash
rtk cargo build --locked --manifest-path /private/tmp/ccp-044697/Cargo.toml
rtk /private/tmp/ccp-044697/target/debug/commit-ci-preflight plan --config tests/fixtures/config-v2-legacy-compatible.toml --json
```

Store stdout byte-for-byte in `matrix-v2-legacy-plan-044697.json`. Store commit, tree, binary SHA-256, command argv, fixture SHA-256, output SHA-256, outer digest, and ordered per-runtime digests in the provenance JSON. Do not copy any of these digest values into production Rust.

- [ ] **Step 3: Write the failing fixture-integrity test**

Add a test that parses the raw JSON, recomputes `canonical_digest(json["plan"])`, asserts it equals `json["plan_digest"]`, and asserts the provenance digest fields equal the raw document. The test must fail before the files are complete or if any fixture byte is changed without updating provenance.

- [ ] **Step 4: Run the focused test and witness RED, then GREEN**

Run:

```bash
rtk cargo test --test matrix_contract historical_legacy_fixture_is_self_consistent -- --exact --nocapture
```

Expected RED: missing fixture or provenance mismatch. Expected GREEN after the exact historical output and provenance are installed.

- [ ] **Step 5: Commit the independent evidence fixture**

```bash
rtk git add tests/fixtures/config-v2-legacy-compatible.toml tests/fixtures/matrix-v2-legacy-plan-044697.json tests/fixtures/matrix-v2-legacy-plan-044697.provenance.json tests/matrix_contract.rs
rtk git commit -m "test: pin historical Matrix V2 plan fixture"
```

---

### Task 2: Closed profile and historical projection

**Files:**
- Create: `src/matrix_legacy.rs`
- Modify: `src/lib.rs:15-34`
- Modify: `src/matrix.rs:147-317,899-980`
- Test: `tests/matrix_contract.rs`

**Interfaces:**
- Consumes: `MatrixPlanV2`, `ExecutionPlanV1`, `NormalizedRuntime`, `NormalizedEnvironment`, `NormalizedCheck`, `NormalizedCache`, `NormalizedReceipt`.
- Produces:
  - `pub enum MatrixPlanProfile { CurrentV2, LegacyV1 }`
  - `impl Default for MatrixPlanProfile` returning `CurrentV2`
  - `pub fn build_matrix_plan(config: MatrixConfigV2, profile: MatrixPlanProfile) -> Result<MatrixPlanEnvelopeV2, MatrixError>`
  - `pub(crate) struct LegacyMatrixDigestBasisV1`
  - `pub(crate) fn project_legacy_basis(plan: &MatrixPlanV2) -> Result<LegacyMatrixDigestBasisV1, MatrixError>`
  - `LegacyMatrixDigestBasisV1::outer_digest() -> Result<String, MatrixError>`
  - `LegacyMatrixDigestBasisV1::runtime_digest(id: &str) -> Result<&str, MatrixError>`

- [ ] **Step 1: Write failing profile and golden projection tests**

Test that `CurrentV2` remains the default, `LegacyV1` projects the generic fixture to the exact historical outer/per-runtime digests, key/table reordering is stable, and an unknown runtime lookup fails. Assert that current and historical expected Latent values occur only in test/fixture/doc paths.

- [ ] **Step 2: Run the projection tests and witness RED**

```bash
rtk cargo test --test matrix_contract legacy_profile_reproduces_historical_plan -- --exact --nocapture
```

Expected: compile failure because `MatrixPlanProfile` and `build_matrix_plan` do not exist.

- [ ] **Step 3: Implement exact private historical types**

In `matrix_legacy.rs`, define `Serialize`-only private types with the exact historical field order and names:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LegacyExecutionPlanV1 {
    schema_version: String,
    project: String,
    runtime: LegacyNormalizedRuntime,
    receipt: NormalizedReceipt,
    environment_allow: Vec<String>,
    caches: Vec<NormalizedCache>,
    checks: Vec<LegacyNormalizedCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LegacyMatrixPlanV2 {
    schema_version: String,
    project: String,
    receipt: NormalizedReceipt,
    environment_allow: Vec<String>,
    caches: Vec<NormalizedCache>,
    runtimes: Vec<LegacyMatrixRuntimePlanV2>,
}
```

`LegacyNormalizedRuntime` contains only `kind`, `image`, `cpu_count`, `memory_mib`, `pids_limit`, and `network`. `LegacyNormalizedCheck` contains only `id`, `required`, `argv`, `working_directory`, `timeout_seconds`, `depends_on`, and `artifacts`. `LegacyMatrixRuntimePlanV2` contains `id`, derived `configuration_digest`, legacy runtime, and legacy checks.

- [ ] **Step 4: Implement explicit representability checks**

Reject the first non-representable field with `MatrixError::LegacyPlanNotRepresentable(&'static str)` when:

```rust
runtime.pull_policy.is_some()
runtime.swap_mode.is_some()
!plan.environment.fixed.is_empty()
!plan.environment.runtime_internal.is_empty()
!plan.environment.remote_secret_only.is_empty()
plan.storage.is_some()
checks.iter().any(|check| !check.artifact_contracts.is_empty())
```

Map only `plan.environment.inherit` to historical `environment_allow`. Build every runtime legacy plan first, derive its digest with `canonical_digest`, then build and hash the legacy outer plan. Do not accept digest inputs.

- [ ] **Step 5: Implement the profile-bearing envelope**

Keep the current `MatrixPlanV2` public shape unchanged. Extend `MatrixPlanEnvelopeV2` with non-serialized private fields `profile: MatrixPlanProfile` and `legacy_basis: Option<LegacyMatrixDigestBasisV1>`. Provide derived accessors `profile()`, `plan_digest()`, and `runtime_configuration_digest(id)`. For current profile, derive from the current plan; for legacy, derive from the stored basis after re-projecting and equality checking. During construction, set each public `MatrixRuntimePlanV2.configuration_digest` to the selected profile's derived runtime digest; all executable runtime/check fields remain the current normalized values.

- [ ] **Step 6: Run focused tests and witness GREEN**

```bash
rtk cargo test --test matrix_contract legacy_profile -- --nocapture
rtk cargo test --test matrix_contract v2_config_is_canonical_across_runtime_declaration_order -- --exact
```

Expected: all selected tests PASS; current digest/order test remains unchanged.

- [ ] **Step 7: Commit the projection boundary**

```bash
rtk git add src/lib.rs src/matrix.rs src/matrix_legacy.rs tests/matrix_contract.rs
rtk git commit -m "feat: derive historical Matrix V2 plan profile"
```

---

### Task 3: Invariant-preserving runtime envelopes and cache identity

**Files:**
- Modify: `src/matrix.rs:280-317,328-427`
- Modify: `src/workspace.rs` only if a test reveals direct public-field dependence rather than accessor use
- Test: `tests/matrix_contract.rs`
- Test: `tests/runtime_cli.rs`

**Interfaces:**
- Consumes: `MatrixPlanEnvelopeV2::profile()`, `plan_digest()`, `runtime_configuration_digest(id)`.
- Produces: `MatrixPlanEnvelopeV2::runtime_envelopes() -> Result<Vec<(String, ExecutionPlanEnvelopeV1)>, MatrixError>` whose execution plans remain current but whose envelope digests are profile-derived.

- [ ] **Step 1: Write failing tamper and namespace tests**

Add tests that mutate a current runtime, check, environment, or legacy basis after construction and require `PlanDigestMismatch` or `LegacyPlanNotRepresentable` before runtime conversion. Assert the legacy and current runtime envelope digests differ and therefore resolve to distinct workspace/cache paths for the same runtime.

- [ ] **Step 2: Witness RED**

```bash
rtk cargo test --test matrix_contract legacy_runtime_envelopes_recheck_projection -- --exact --nocapture
rtk cargo test --test runtime_cli legacy_profile_uses_distinct_plan_cache_identity -- --exact --nocapture
```

Expected: current code recomputes current digests and cannot preserve the historical basis.

- [ ] **Step 3: Implement invariant rechecks**

Make `canonical_bytes()` and `runtime_envelopes()` call one `validate_profile_binding()` method. For legacy, reconstruct the basis from the current plan, compare it to the owned basis, recompute all digests, and use the derived historical runtime digest in `ExecutionPlanEnvelopeV1.plan_digest`. Never alter `ExecutionPlanV1` fields to obtain that digest.

- [ ] **Step 4: Run focused tests and witness GREEN**

```bash
rtk cargo test --test matrix_contract legacy_runtime_envelopes_recheck_projection -- --exact
rtk cargo test --test runtime_cli legacy_profile_uses_distinct_plan_cache_identity -- --exact
```

- [ ] **Step 5: Commit the execution-envelope binding**

```bash
rtk git add src/matrix.rs src/workspace.rs tests/matrix_contract.rs tests/runtime_cli.rs
rtk git commit -m "feat: bind legacy Matrix digests to runtime envelopes"
```

---

### Task 4: Shared CLI profile across plan, doctor, dry-run, and run

**Files:**
- Modify: `src/main.rs:91-153,444-469,951-1295,1419-1541,2202-2303`
- Test: `tests/plan_cli.rs`
- Test: `tests/runtime_cli.rs`

**Interfaces:**
- Consumes: `MatrixPlanProfile` through Clap `ValueEnum` or an equivalent closed parser.
- Produces: a flattened `MatrixPlanProfileArgs { matrix_plan_profile: MatrixPlanProfile }` shared verbatim by all four command variants and one `load_matrix_plan(path, profile)` helper.

- [ ] **Step 1: Capture default CLI output baseline**

Before production changes, run current `plan --json` and the four `--help` commands against checked-in fixtures and pin only the default plan JSON bytes needed for regression. Do not pin terminal-width-dependent help formatting.

- [ ] **Step 2: Write failing CLI tests**

Cover:

```text
plan|doctor|dry-run|run --matrix-plan-profile matrix-v2-legacy-v1
plan --matrix-plan-profile current-v2
unknown profile -> exit 2
legacy profile + schema 1.0/1.1/1.2/1.3 -> exit 2 before cache/admission
omitted profile -> exact baseline stdout
```

For legacy `plan --json`, assert a top-level `matrix_plan_profile` and a reconstructible `legacy_digest_basis`; for current/default JSON, assert these keys are absent and bytes match baseline.

- [ ] **Step 3: Witness RED**

```bash
rtk cargo test --test plan_cli matrix_legacy_profile -- --nocapture
rtk cargo test --test runtime_cli matrix_legacy_profile -- --nocapture
```

- [ ] **Step 4: Implement one shared argument and loader**

Add the identical flattened argument to the four commands, pass it through the dispatch match, and call:

```rust
fn load_matrix_plan(
    path: &Path,
    profile: MatrixPlanProfile,
) -> Result<MatrixPlanEnvelopeV2, CliError>
```

Reject a non-current profile before single-runtime `load_plan`, cache resolution, journal initialization, or admission. Avoid four separate profile parsers/builders.

- [ ] **Step 5: Preserve default output and disclose legacy output**

Keep `MatrixPlanEnvelopeV2`'s current serialized envelope unchanged for default output. Serialize a dedicated `LegacyMatrixPlanReportV1` only when legacy is selected; it contains `matrix_plan_profile`, derived `plan_digest`, current reviewable plan, and the normalized legacy digest basis. Add the profile label to legacy human output only.

- [ ] **Step 6: Run focused tests and witness GREEN**

```bash
rtk cargo test --test plan_cli
rtk cargo test --test runtime_cli
```

- [ ] **Step 7: Commit command parity**

```bash
rtk git add src/main.rs tests/plan_cli.rs tests/runtime_cli.rs
rtk git commit -m "feat: expose Matrix V2 compatibility profile"
```

---

### Task 5: Producer provenance and pre-seal receipt invariants

**Files:**
- Modify: `src/run.rs:55-61,373-422,638-703`
- Modify: `src/main.rs:1111-1126`
- Modify: `src/matrix.rs:328-427,480-550`
- Test: `tests/receipt_contract.rs`
- Test: `tests/matrix_contract.rs`

**Interfaces:**
- Consumes: `RunRequest { producer_version: &'a str }` and `MatrixPlanProfile::producer_version() -> &'static str`.
- Produces: inner and outer Matrix receipts whose producer name/version match exactly and whose configuration digests derive from the selected profile.

- [ ] **Step 1: Write failing producer-version tests**

Assert current single-runtime and Matrix paths still emit `0.1.0`; legacy Matrix inner and outer receipts emit `0.1.0+matrix-v2-legacy-v1`; mixed inner versions and digest mutations fail before outer sealing.

- [ ] **Step 2: Witness RED**

```bash
rtk cargo test --test receipt_contract explicit_producer_version_is_sealed -- --exact --nocapture
rtk cargo test --test matrix_contract legacy_receipt_provenance_is_uniform -- --exact --nocapture
```

- [ ] **Step 3: Move producer selection to the request boundary**

Add `producer_version: &'a str` to `RunRequest`. Replace the internal `env!("CARGO_PKG_VERSION")` assignment with `request.producer_version.to_owned()`. Update every constructor in source and tests to pass `env!("CARGO_PKG_VERSION")`, except the legacy Matrix orchestrator, which passes `envelope.profile().producer_version()`.

- [ ] **Step 4: Recheck immediately before execution and sealing**

At `execute_matrix_run_v2` entry, before each inner runtime, and before `MatrixReceiptEnvelopeV2::seal`, call `validate_profile_binding()`. Require every inner receipt producer to equal the first receipt producer and equal the selected profile producer tuple. Require every inner/outer configuration digest to equal the profile-derived accessors.

- [ ] **Step 5: Run focused tests and witness GREEN**

```bash
rtk cargo test --test receipt_contract
rtk cargo test --test matrix_contract
rtk cargo test --lib run::tests
```

- [ ] **Step 6: Commit provenance binding**

```bash
rtk git add src/run.rs src/main.rs src/matrix.rs tests/receipt_contract.rs tests/matrix_contract.rs
rtk git commit -m "feat: disclose Matrix compatibility receipt provenance"
```

---

### Task 6: Actual historical verifier compatibility and fail-closed mutations

**Files:**
- Create: `tests/fixtures/policy-v2-legacy-compatible.toml`
- Modify: `tests/verification_contract.rs`
- Modify: `tests/verify_cli.rs`
- Modify: `tests/matrix_contract.rs`

**Interfaces:**
- Consumes: a deterministic synthetic legacy-profile Matrix receipt and externally selected Matrix V2 policy.
- Produces: end-to-end evidence that the exact historical verifier accepts the valid receipt and rejects mutations.

- [ ] **Step 1: Write the synthetic sealed-receipt builder**

Build two inner schema `1.0` receipts and one outer schema `2.0` receipt with fixed commit, times, platform, check evidence, legacy producer suffix, and profile-derived digests. Do not execute Docker or project commands.

- [ ] **Step 2: Write failing historical-verifier tests**

Invoke the verifier built from exact historical source `044697...` against the synthetic receipt, exact policy, expected commit, and fixed evaluation time. Require PASS for the valid document and FAIL for mutations to producer evidence, expected commit, outer digest, one runtime digest, required check binding, runtime binding, and one receipt byte.

- [ ] **Step 3: Witness RED**

```bash
rtk cargo test --test verification_contract historical_matrix_verifier_accepts_legacy_profile_receipt -- --exact --nocapture
```

Expected: valid receipt fails until Tasks 2-5 supply exact historical digests and provenance.

- [ ] **Step 4: Make the harness hermetic**

The test must consume an exact prebuilt historical verifier path supplied by `CCP_HISTORICAL_VERIFIER_044697`, verify its SHA-256 against the pinned provenance fixture before invocation, and otherwise mark the test ignored with an explicit setup message. A separate pure-Rust current verifier test must always run. The test must never download or build historical source implicitly.

- [ ] **Step 5: Run current and historical verification suites**

```bash
rtk cargo test --test verification_contract
rtk cargo test --test verify_cli
rtk env CCP_HISTORICAL_VERIFIER_044697=/private/tmp/ccp-044697/target/debug/commit-ci-preflight cargo test --test verification_contract historical_matrix_verifier -- --nocapture
```

Expected: valid receipt PASS; every mutation FAIL with the expected finding class.

- [ ] **Step 6: Commit verifier evidence**

```bash
rtk git add tests/fixtures/policy-v2-legacy-compatible.toml tests/verification_contract.rs tests/verify_cli.rs tests/matrix_contract.rs
rtk git commit -m "test: prove historical Matrix verifier compatibility"
```

---

### Task 7: Fail-before-mutation command tests

**Files:**
- Modify: `tests/runtime_cli.rs`
- Modify: `tests/plan_cli.rs`
- Modify: `tests/matrix_contract.rs`

**Interfaces:**
- Consumes: profile-aware command paths from Task 4 and representability errors from Task 2.
- Produces: observable proof that invalid compatibility requests touch no admission, cache, journal, runtime, or receipt surface.

- [ ] **Step 1: Write filesystem and fake-runtime assertions**

For single-runtime misuse and every parser-reachable non-representable Matrix input, invoke `run` with isolated nonexistent cache/admission roots and a fake Docker executable that creates a marker if called. Assert exit code 2, named field in stderr, no cache root, no journal, no receipt, no marker, and byte-identical source tree.

- [ ] **Step 2: Witness RED**

```bash
rtk cargo test --test runtime_cli legacy_profile_rejection_precedes_shared_state -- --exact --nocapture
```

- [ ] **Step 3: Move validation earlier if any marker appears**

All profile parsing, schema selection, legacy projection, and invariant validation must complete before `resolve_cache_root`, `ManagedCache::initialize`, `RunJournalStore::initialize`, `AdmissionCoordinator::platform_for`, or runtime construction.

- [ ] **Step 4: Run focused suites and witness GREEN**

```bash
rtk cargo test --test runtime_cli
rtk cargo test --test plan_cli
rtk cargo test --test matrix_contract
```

- [ ] **Step 5: Commit pre-mutation safety**

```bash
rtk git add tests/runtime_cli.rs tests/plan_cli.rs tests/matrix_contract.rs src/main.rs src/matrix.rs
rtk git commit -m "test: enforce pre-admission compatibility failures"
```

---

### Task 8: Operator documentation and evidence matrix

**Files:**
- Modify: `docs/CONFIGURATION.md`
- Modify: `docs/LOCAL_RUN.md`
- Modify: `docs/MULTI_RUNTIME_RECEIPTS.md`
- Modify: `docs/RECEIPT_SPEC.md`
- Modify: `docs/GITHUB_GATE.md`
- Modify: `docs/ADOPTION_GUIDE.md`
- Modify: `docs/CACHE_AND_WORKSPACE.md`
- Modify: `docs/TROUBLESHOOTING.md`
- Modify: `docs/INVARIANT_EVIDENCE_MATRIX.md`
- Modify: `docs/TESTING_AND_FAULT_INJECTION.md`
- Modify: `CHANGELOG.md`
- Modify: `tests/repository_hygiene_contract.rs`

**Interfaces:**
- Consumes: completed CLI and receipt behavior.
- Produces: one consistent public operator contract and repository-enforced documentation coverage.

- [ ] **Step 1: Write failing documentation contract tests**

Require the docs to name the exact profile, Matrix-only scope, producer suffix, command parity, cache namespace separation, no policy inference, historical-verifier boundary, and rollback to `current-v2`. Scan `src/` to reject the three Latent adopter digest strings and the generic golden digest strings.

- [ ] **Step 2: Witness RED**

```bash
rtk cargo test --test repository_hygiene_contract matrix_legacy_profile_is_documented_without_production_digest_constants -- --exact --nocapture
```

- [ ] **Step 3: Update operator documentation**

Document the exact sequence:

```console
commit-ci-preflight plan --matrix-plan-profile matrix-v2-legacy-v1 --json
commit-ci-preflight doctor --matrix-plan-profile matrix-v2-legacy-v1 --json
commit-ci-preflight dry-run --matrix-plan-profile matrix-v2-legacy-v1 --json
commit-ci-preflight run --matrix-plan-profile matrix-v2-legacy-v1 --generation N --json
```

State that the operator copies reviewed digests into Matrix policy v2, never from a completed receipt; legacy and current cache identities are separate; `verify` has no profile flag; evidence branches remain append-once; and an old trusted verifier must accept the exact receipt before policy migration.

- [ ] **Step 4: Update evidence and fault-injection matrices**

Map each invariant to its focused test, including projection reproducibility, representability rejection, command parity, cache separation, producer uniformity, historical verifier acceptance, mutation rejection, and zero pre-admission mutation.

- [ ] **Step 5: Add the changelog entry and run docs tests**

```bash
rtk cargo test --test repository_hygiene_contract
rtk cargo test --test release_hardening_contract
```

- [ ] **Step 6: Commit documentation**

```bash
rtk git add CHANGELOG.md docs tests/repository_hygiene_contract.rs
rtk git commit -m "docs: document Matrix V2 compatibility profile"
```

---

### Task 9: Static qualification and review checkpoint

**Files:**
- Modify only files required to fix defects found by the gates; every fix gets a focused regression test first.

**Interfaces:**
- Consumes: Tasks 1-8.
- Produces: clean exact HEAD, complete static gate evidence, candidate binary path/SHA, and a new stop boundary before any CCP heavy run, installation, publication, or adopter execution.

- [ ] **Step 1: Run formatting and warnings-denied build**

```bash
rtk cargo fmt --all -- --check
rtk env RUSTFLAGS=-Dwarnings cargo build --locked --all-targets
```

- [ ] **Step 2: Run strict Clippy**

```bash
rtk cargo clippy --locked --all-targets --all-features -- -D warnings
```

- [ ] **Step 3: Run all tests and release metadata checks**

```bash
rtk cargo test --locked --all-targets --all-features
rtk cargo test --test release_hardening_contract
rtk cargo test --test repository_hygiene_contract
```

- [ ] **Step 4: Run the exact historical verifier integration**

Verify the historical binary SHA-256 immediately before the test, then run the non-ignored exact-verifier suite with `CCP_HISTORICAL_VERIFIER_044697` set. Record binary path, full SHA-256, historical commit/tree, test command, and terminal result.

- [ ] **Step 5: Verify CLI contracts without executing a run**

```bash
rtk cargo run --locked -- plan --config tests/fixtures/config-v2-legacy-compatible.toml --matrix-plan-profile current-v2 --json
rtk cargo run --locked -- plan --config tests/fixtures/config-v2-legacy-compatible.toml --matrix-plan-profile matrix-v2-legacy-v1 --json
rtk cargo run --locked -- dry-run --config tests/fixtures/config-v2-legacy-compatible.toml --matrix-plan-profile matrix-v2-legacy-v1 --cache-dir /private/tmp/ccp-legacy-dry-run-cache --repository . --json
```

Do not run `doctor` unless a bounded runtime-probe permission is explicitly available; do not run `run` in this task.

- [ ] **Step 6: Perform independent two-stage review**

First review spec conformance and evidence boundaries; then review implementation quality, error ordering, serialization stability, and test non-tautology. Resolve every accepted defect through RED-GREEN tests.

- [ ] **Step 7: Build and hash an isolated candidate**

Build from the clean exact HEAD into an isolated target directory, report absolute path, complete SHA-256, `--version`, source commit/tree, and working-tree status. Do not install or replace the stable binary.

- [ ] **Step 8: Stop at the authorization boundary**

Report the exact HEAD, tree, candidate path/SHA, current and legacy fixture digests, test results, remaining limitations, and the exact authorization text required for one CCP exact-head qualification. Do not start CCP, Docker-heavy execution, publish a receipt, push, open a PR, modify a ruleset, or run Latent-TRIZ.

- [ ] **Step 9: Commit only gate-driven fixes and the final checkpoint**

```bash
rtk git status --short --branch
rtk git log -1 --format='%H %T %s'
```

The branch must be clean before requesting the next authorization.

---

## Self-review record

- Spec coverage: every decision, CLI, canonical representation, runtime/receipt, recovery, verification, documentation, and stop-boundary requirement maps to Tasks 1-9.
- Placeholder scan: no unresolved markers, deferred implementation instruction, unnamed error-handling step, or unspecified test family remains.
- Type consistency: `MatrixPlanProfile`, `build_matrix_plan`, `MatrixPlanEnvelopeV2` accessors, `LegacyMatrixDigestBasisV1`, and `RunRequest::producer_version` are named once and consumed consistently.
- Scope separation: this plan changes CCP only. It performs no Latent-TRIZ receipt rewrite, ruleset bypass, adopter run, model access, or scientific execution.
- Official-contract alignment: Matrix V2 remains outer v2 plus inner v1; policy remains external; only `run` acquires admission; cache isolation follows derived digest identity.
