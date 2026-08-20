# T8 Runtime Capability Contract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an opt-in schema `1.3` Docker runtime contract that proves bounded daemon RAM/swap capability and local pinned-image resolution before an attestable run begins.

**Architecture:** Schema `1.3` is additive and opt-in. The normalized plan carries the explicit pull/swap policy; a read-only, injected runtime capability probe creates privacy-bounded evidence once, then the run path passes that evidence into receipt v2. Historical schemas and matrix v2 continue to omit this evidence.

**Tech Stack:** Rust 2024, serde/schemars, existing `ProcessSupervisor`, Docker CLI argv, SHA-256 canonical receipts, Cargo test.

**Spec:** `docs/superpowers/specs/2026-08-20-t8-runtime-capability-design.md`

## Global Constraints

- Add no dependency and no Docker Engine API client.
- Preserve schemas `1.0`–`1.2` and matrix v2 behaviour.
- Schema `1.3` requires `pull_policy = "never"`, `swap_mode = "disabled"`, explicit environment classes, and explicit storage policy.
- Never pull, build, start or remove an image/container during capability preflight.
- Do not serialize hostname, Docker root path, raw Docker output, context literal, absolute host paths, secrets, or credentials.
- Use bounded explicit argv and the existing runtime-discovery environment.
- Do not run Docker, OrbStack, CCP, or other native qualification during implementation.
- Regenerate pinned schemas with `cargo run --locked --quiet --example generate_contract`.

## Implementation record (2026-08-20)

This plan is implemented through the following local, reviewable source
checkpoints on `codex/runtime-capability-contract-v1`:

| Slice | Local checkpoint | Deterministic evidence |
|---|---|---|
| T8-C/1 — schema policy and argv | `36c2ec8` | Focused config/runtime tests, generated schemas, formatting, and diff check. |
| T8-C/2 — read-only capability preflight | `0a28e0f` | Fake-supervisor runtime/run tests and full deterministic suite. |
| T8-C/3 — receipt v2 evidence binding | `d841fac` | Receipt/run/schema-contract tests and full deterministic suite. |
| T8-C/4 — mount grammar hardening | `28ef209` | Table-driven mount tests and full deterministic suite. |

The latest source-only run completed `312` tests with `2` explicitly ignored.
It did not invoke Docker, OrbStack, CCP admission, a container, a network
fetch, or a native qualification workload. This is not platform qualification,
receipt verification for a candidate SHA, a release decision, or publication
authority.

Before claiming a platform PASS, T11 still requires an exact-commit native
receipt and independent verification on each claimed host class. Initial
artifact-state evidence, inode reserve, and owned cache-total enforcement also
remain outside this T8-C slice.

The unchecked task boxes below are the preserved approved execution worksheet,
not a live completion indicator. The implementation record above is the
authoritative status; T8-C/5 still awaits the owner-authorized documentation
checkpoint and separate publication authority.

---

### Task 1: Schema 1.3 runtime policy and deterministic argv

**Files:**
- Modify: `src/config.rs`
- Modify: `src/runtime.rs`
- Modify: `src/matrix.rs`
- Modify: `docs/CONFIGURATION.md`
- Modify: `docs/RUNTIME.md`
- Modify: `CHANGELOG.md`
- Modify: `schema/config-v1.schema.json`
- Modify: `schema/receipt-v2.schema.json`

**Interfaces:**
- Produces `RuntimePullPolicy::Never`, `RuntimeSwapMode::Disabled`, and optional normalized runtime policy fields.
- Produces `NormalizedRuntime { pull_policy: Option<RuntimePullPolicy>, swap_mode: Option<RuntimeSwapMode>, .. }`.
- Consumes existing `docker_dry_run_check(&NormalizedRuntime, ...)`.

- [ ] **Step 1: Write failing config tests**

```rust
#[test]
fn v1_3_requires_explicit_runtime_capability_policy() {
    let error = ConfigV1::parse(SCHEMA_1_3_WITHOUT_POLICY)
        .and_then(ConfigV1::into_plan)
        .expect_err("schema 1.3 must require runtime policy");
    assert!(matches!(error, ConfigError::MissingRuntimeCapabilityPolicy));
}

#[test]
fn v1_3_runtime_policy_changes_plan_digest() {
    assert_ne!(plan(SCHEMA_1_3_NEVER_DISABLED).plan_digest,
               plan(SCHEMA_1_3_DIFFERENT_POLICY).plan_digest);
}
```

- [ ] **Step 2: Run the config tests and confirm they fail**

Run: `rtk cargo test --locked config::tests::v1_3`

Expected: compilation or validation failure because schema `1.3` and the
runtime policy types do not exist.

- [ ] **Step 3: Add the minimal schema and normalization contract**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimePullPolicy { Never }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeSwapMode { Disabled }
```

Add optional source fields and normalized-plan fields. Accept `"1.3"` only
when both values are present and exactly the two supported values. Require the
same explicit-environment and storage policy rules as schema `1.2`. Reject the
new runtime fields for schemas `1.0`–`1.2`.

- [ ] **Step 4: Add exact argv tests and rendering**

```rust
assert_subsequence(
    &dry_run.argv,
    &["--pull", "never", "--memory", "256m", "--memory-swap", "256m"],
);
```

Render `--pull never` and `--memory-swap <memory_mib>m` only when both
normalized schema-`1.3` runtime policy fields are present. Preserve the exact
historical argv for older schemas.

- [ ] **Step 5: Run focused tests**

Run: `rtk cargo test --locked config::tests::v1_3`

Run: `rtk cargo test --locked runtime::tests`

Expected: all focused tests pass without invoking Docker.

- [ ] **Step 6: Regenerate schemas and update documentation**

Run: `rtk cargo run --locked --quiet --example generate_contract`

Document migration, opt-in status, no-pull behavior, disabled-swap rendering,
and the fact that no native capability claim is created by the schema alone.

- [ ] **Step 7: Commit Task 1**

```bash
git add -- src/config.rs src/runtime.rs src/matrix.rs docs/CONFIGURATION.md \
  docs/RUNTIME.md CHANGELOG.md schema/config-v1.schema.json schema/receipt-v2.schema.json
git commit -m "feat: declare schema 1.3 runtime policy"
```

### Task 2: Bounded runtime capability probe

**Files:**
- Modify: `src/runtime.rs`
- Modify: `src/run.rs`
- Modify: `src/main.rs`
- Test: `src/runtime.rs`
- Test: `src/run.rs`

**Interfaces:**
- Produces `RuntimeCapabilityEvidenceV1` containing only booleans, a context digest, resolved image ID, and exact configured image reference.
- Produces `RuntimeCapabilityProbe` with `probe(&ExecutionPlanEnvelopeV1, &dyn SupervisorPort, ...) -> Result<Option<RuntimeCapabilityEvidenceV1>, RuntimeError>`.
- Consumes `RuntimeProbe`, `ProcessRequest`, `runtime_environment()`, `doctor_guard`, and `canonical_digest`.

- [ ] **Step 1: Write failing parser and ordering tests**

```rust
#[test]
fn schema_1_3_rejects_daemon_without_swap_limit_capability() {
    assert!(matches!(interpret_runtime_capabilities(INFO_WITHOUT_SWAP, CONTEXT, IMAGE),
        Err(RuntimeError::UnsupportedCapability("swap_limit"))));
}

#[test]
fn rejected_capability_preflight_never_starts_git_or_workspace() {
    let error = execute_local_run_with_runtime_capability_probe(..., &RejectingProbe);
    assert!(matches!(error, RunError::Runtime(_)));
    assert_eq!(supervisor.git_revisions.load(Ordering::SeqCst), 0);
}
```

- [ ] **Step 2: Run tests and confirm they fail**

Run: `rtk cargo test --locked runtime::tests::schema_1_3`

Run: `rtk cargo test --locked run::tests::rejected_capability_preflight`

Expected: missing capability types and injected run entrypoint.

- [ ] **Step 3: Implement the read-only probe with strict parsers**

Use only these argv forms, each with existing five-second / 64-KiB bounds and
the discovery environment:

```text
docker info --format {{json .}}
docker context show
docker image inspect --format {{json .}} <configured-pinned-image>
```

Require JSON booleans `MemoryLimit` and `SwapLimit` equal `true`, a bounded
non-control context string, an image ID matching `sha256:<64 lowercase hex>`,
and an image repo-digest array containing the exact configured image reference.
Digest the context literal immediately with `canonical_digest`; never return it.

- [ ] **Step 4: Integrate the probe before journal/snapshot and in the library**

In `print_run`, call the probe after storage preflight and before
`RunJournalStore::initialize`. In the library path, call the injectable probe
before Git inspection. Reuse captured evidence through the run; do not issue a
second probe. Map every failure to typed `RuntimeError`/`RunError` and preserve
existing admission release behavior.

- [ ] **Step 5: Run focused tests**

Run: `rtk cargo test --locked runtime::tests`

Run: `rtk cargo test --locked run::tests`

Expected: all pass with fake supervisors/probes only.

- [ ] **Step 6: Commit Task 2**

```bash
git add -- src/runtime.rs src/run.rs src/main.rs
git commit -m "feat: preflight Docker runtime capabilities"
```

### Task 3: Bind runtime evidence to receipt v2

**Files:**
- Modify: `src/receipt.rs`
- Modify: `src/run.rs`
- Modify: `src/schema_contract.rs`
- Modify: `tests/receipt_contract.rs`
- Modify: `schema/receipt-v2.schema.json`
- Modify: `docs/RECEIPT_SPEC.md`

**Interfaces:**
- Produces `RuntimeCapabilityEvidenceV1` receipt validation.
- Consumes `ExecutionPlanV1.schema_version`, `NormalizedRuntime`, and the capability probe result from Task 2.

- [ ] **Step 1: Write failing receipt tests**

```rust
#[test]
fn schema_1_3_receipt_requires_matching_runtime_capability_evidence() {
    let mut receipt = passing_schema_1_3_receipt();
    receipt.runtime_capability_evidence = None;
    assert!(matches!(receipt.validate(), Err(ReceiptError::MissingRuntimeCapabilityEvidence)));
}

#[test]
fn historical_receipt_rejects_runtime_capability_evidence() {
    let mut receipt = passing_receipt_v2();
    receipt.runtime_capability_evidence = Some(valid_evidence());
    assert!(matches!(receipt.validate(), Err(ReceiptError::UnexpectedRuntimeCapabilityEvidence)));
}
```

- [ ] **Step 2: Run tests and confirm failure**

Run: `rtk cargo test --locked receipt::tests::runtime_capability`

Expected: missing receipt field and validation errors.

- [ ] **Step 3: Add canonical, schema-gated evidence validation**

Add an optional `runtime_capability_evidence` field to `ReceiptV2`. Validate
schema `1.0`, two true capability booleans, SHA-256 format for context and
image ID, and exact equality of `resolved_image_reference` and
`execution_plan.runtime.image`. Require it only for schema `1.3` and forbid it
otherwise.

- [ ] **Step 4: Carry evidence from Task 2 into v2 sealing**

Extend the private run outcome plumbing so receipt sealing receives the single
captured optional evidence object. Keep the public v1 receipt shape unchanged.

- [ ] **Step 5: Regenerate and run receipt tests**

Run: `rtk cargo run --locked --quiet --example generate_contract`

Run: `rtk cargo test --locked receipt::tests`

Run: `rtk cargo test --locked --test receipt_contract`

Expected: pinned schemas and fixtures are byte-stable or intentionally updated
only through the generator.

- [ ] **Step 6: Commit Task 3**

```bash
git add -- src/receipt.rs src/run.rs src/schema_contract.rs tests/receipt_contract.rs \
  schema/receipt-v2.schema.json docs/RECEIPT_SPEC.md
git commit -m "feat: bind runtime capability evidence to receipts"
```

### Task 4: Mount grammar regression coverage and handoff documentation

**Files:**
- Modify: `src/runtime.rs`
- Modify: `src/workspace.rs`
- Modify: `docs/RUNTIME.md`
- Modify: `docs/LOCAL_RUN.md`
- Modify: `docs/RELIABILITY_HARDENING_PLAN.md`
- Modify: `CHANGELOG.md`
- Test: `src/runtime.rs`
- Test: `src/workspace.rs`

**Interfaces:**
- Consumes `docker_mount_argument`, `validate_host_path`, `WorkspacePlanV1`, and the schema-`1.3` contract.
- Produces deterministic accept/reject coverage; it adds no mount syntax or new public API.

- [ ] **Step 1: Write table-driven failing mount tests**

```rust
for hostile in ["/safe,comma", "/safe\nnewline", "/safe\0nul"] {
    assert!(matches!(docker_mount_argument(&mount(hostile)), Err(RuntimeError::Workspace(_))));
}
```

Add target/path cases for traversal, nested overlap, empty segments and control
characters through the configuration/workspace validators.

- [ ] **Step 2: Run tests and confirm expected current gaps**

Run: `rtk cargo test --locked runtime::tests::mount`

Run: `rtk cargo test --locked workspace::tests`

Expected: tests identify any delimiter case not yet rejected. If all reject
already, retain them as regression tests and document that no renderer change
was necessary.

- [ ] **Step 3: Apply the smallest renderer or validator correction**

Keep `--mount` structured and shell-free. Reject unsafe input before argv
rendering; do not escape, quote, shell-parse, or accept ambiguous syntax.

- [ ] **Step 4: Update the runbook and hardening ledger**

State the exact three probe commands, evidence privacy boundary, failure
ordering, historical-schema compatibility and remaining native qualification.
Do not claim a Docker/OrbStack/Linux/Windows PASS without a corresponding
exact-commit receipt.

- [ ] **Step 5: Run final deterministic gates**

Run:

```bash
rtk cargo fmt --all -- --check
rtk cargo run --locked --quiet --example generate_contract
rtk cargo test --locked --all-targets --all-features
rtk git diff --check
```

Expected: all tests pass; generated schemas are current; no heavy runtime is
started.

- [ ] **Step 6: Inspect scope and commit Task 4**

```bash
git diff --name-only bf07d8c..HEAD
git diff --check
git add -- src/runtime.rs src/workspace.rs docs/RUNTIME.md docs/LOCAL_RUN.md \
  docs/RELIABILITY_HARDENING_PLAN.md CHANGELOG.md
git commit -m "test: harden Docker mount grammar"
```

### Task 5: Publication and qualification boundary

**Files:**
- Modify: `docs/RELIABILITY_HARDENING_PLAN.md`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes all completed Task 1–4 evidence and the existing exact-head receipt policy.
- Produces a truthful draft-PR body; no release or merge action.

- [ ] **Step 1: Review the aggregate diff against the spec**

Run:

```bash
git diff --check bf07d8c..HEAD
git diff --stat bf07d8c..HEAD
git status --short --branch
```

Expected: only configuration, runtime, receipt, schema, tests and directly
supporting docs from Tasks 1–4 changed.

- [ ] **Step 2: Record completion boundaries**

Mark T8-C as source-verified only. Leave initial artifact state, inode reserve,
owned cache-total enforcement and exact native qualification explicitly pending
where they remain unimplemented.

- [ ] **Step 3: Request review and publication authority**

Do not stage, commit the final documentation, push, open a PR, merge, or run
OrbStack qualification without the owner’s explicit authorization.
