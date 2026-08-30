# M1 Run Application Seam Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the private telescoping run orchestration boundary with one coherent dependency object while retaining every existing public wrapper and byte-visible behavior.

**Architecture:** `RunRequest` remains the immutable use-case input. A private `RunDependencies` object groups existing effect ports and travels through the run and receipt orchestration paths; no new trait or public API is introduced. `main.rs`, `matrix.rs`, `runtime.rs`, and `process.rs` remain unchanged composition roots or adapters.

**Tech Stack:** Rust 2024, existing `RuntimePort`, `SupervisorPort`, `StorageProbe`, `RuntimeCapabilityProbe`, `CompletionBarrier`, `RunLifecycleObserver`, unit fakes, and the M0 compatibility corpus.

**Spec:** `docs/superpowers/specs/2026-08-30-capability-packs-clean-architecture-design.md`

## Global Constraints

- M0 must be terminal before this plan starts.
- Every shell command begins with `rtk`.
- Preserve all public functions at `src/run.rs:139-301` and `src/run.rs:378-402` with exact signatures and semantics.
- Do not modify `src/main.rs`, `src/matrix.rs`, `src/runtime.rs`, `src/process.rs`, schemas, fixtures, CLI text, receipt formats, or policy formats.
- `RunDependencies` remains private to `run.rs`; no new analyzer or pack concept enters this milestone.
- Preserve run ID inputs/order, timestamps, receipt sealing, atomic no-overwrite publication, capability evidence, artifact observation, cache promotion, cancellation, lifecycle, and barrier ordering.
- Strict RED/GREEN TDD; one production file owner; no Docker, network, CCP heavy command, publication, or external mutation.

---

### Task 1: Introduce the private dependency object at the outer run seam

**Files:**
- Modify: `src/run.rs:55-424`
- Test: `src/run.rs` test module

**Interfaces:**
- Consumes: existing `RunRequest<'a>` and all existing run effect ports.
- Produces: private `RunDependencies<'a>` and `execute_local_run_with_dependencies`.
- Preserves: every existing public run wrapper.

- [ ] **Step 1: Write the failing seam equivalence test**

Reuse the smallest existing passing run fixture in the `run.rs` test module.
Add a test that constructs the same request, runtime, supervisor, cancellation,
fixed clock, storage probe, capability probe, noop barrier, and noop lifecycle
as that fixture, then calls this not-yet-existing function:

```rust
let mut barrier = NoopCompletionBarrier;
let mut lifecycle = NoopRunLifecycleObserver;
let mut dependencies = RunDependencies {
    runtime: &runtime,
    supervisor: &supervisor,
    cancellation: &cancellation,
    clock: &clock,
    barrier: &mut barrier,
    lifecycle: &mut lifecycle,
    storage_probe: &storage_probe,
    capability_probe: &capability_probe,
    runtime_preflight: None,
};
let outcome = execute_local_run_with_dependencies(&request, &mut dependencies)
    .expect("execute through dependency seam");
assert_eq!(outcome.exit_code(), 0);
assert_eq!(
    outcome.published_canonical_bytes().expect("canonical bytes"),
    fs::read(&outcome.receipt_path).expect("published receipt")
);
```

- [ ] **Step 2: Run the focused test and verify RED**

```console
rtk cargo test --locked --offline --lib run_dependencies_execute_the_existing_run_path
```

Expected: compile failure because `RunDependencies` and
`execute_local_run_with_dependencies` do not exist.

- [ ] **Step 3: Add the minimal private dependency object**

Add immediately after `RunRequest`:

```rust
struct RunDependencies<'a> {
    runtime: &'a dyn RuntimePort,
    supervisor: &'a dyn SupervisorPort,
    cancellation: &'a CancellationToken,
    clock: &'a dyn Clock,
    barrier: &'a mut dyn CompletionBarrier,
    lifecycle: &'a mut dyn RunLifecycleObserver,
    storage_probe: &'a dyn StorageProbe,
    capability_probe: &'a dyn RuntimeCapabilityProbe,
    runtime_preflight: Option<RuntimePreflight>,
}
```

Replace the private
`execute_local_run_with_barrier_and_lifecycle_and_storage_probe_and_capability_probe`
signature with:

```rust
fn execute_local_run_with_dependencies(
    request: &RunRequest<'_>,
    dependencies: &mut RunDependencies<'_>,
) -> Result<RunOutcome, RunError>
```

Keep its receipt-v2 sealing, canonical byte selection, atomic write, and outcome
construction byte-for-byte. Pass `dependencies` to the receipt/artifact core;
do not reorder any operation.

- [ ] **Step 4: Verify focused GREEN**

```console
rtk cargo test --locked --offline --lib run_dependencies_execute_the_existing_run_path
```

Expected: PASS.

- [ ] **Step 5: Commit Task 1**

```console
rtk git add src/run.rs
rtk git commit -m "refactor: add private run dependency seam"
```

### Task 2: Route every compatibility wrapper through the seam

**Files:**
- Modify: `src/run.rs:139-424`
- Test: `src/run.rs` test module

**Interfaces:**
- Consumes: `RunDependencies` and `execute_local_run_with_dependencies` from Task 1.
- Produces: thin adapters for every existing public wrapper.
- Preserves: wrapper names, visibility, argument order, return types, defaults, and fault injection.

- [ ] **Step 1: Write the wrapper equivalence test before migration**

Using two independently created copies of the same fixture and fixed clock,
characterize the default `execute_local_run` wrapper only: execute one copy
through that public wrapper and one through a direct `RunDependencies` call.
Compare:

```rust
assert_eq!(public.exit_code(), direct.exit_code());
assert_eq!(public.receipt.receipt, direct.receipt.receipt);
assert_eq!(public.receipt_v2, direct.receipt_v2);
assert_eq!(
    public.published_canonical_bytes().expect("public bytes"),
    direct.published_canonical_bytes().expect("direct bytes")
);
```

Give each fixture an isolated repository/cache root so atomic receipt output and
cache generations cannot collide.

- [ ] **Step 2: Establish the pre-migration characterization baseline**

Run the equivalence test before migrating the remaining wrappers:

```console
rtk cargo test --locked --offline --lib public_run_wrapper_matches_dependency_seam_bytes
```

Expected: PASS because Task 1's seam still delegates to the unchanged receipt
core. This passing characterization test is the safety net for the mechanical
adapter migration; Task 1 already supplied the compile-RED production seam.

- [ ] **Step 3: Migrate public wrappers mechanically**

Each existing wrapper creates only its current defaults plus a
`RunDependencies` value, then calls `execute_local_run_with_dependencies`.
For example, the storage-probe wrapper becomes structurally:

```rust
let mut barrier = NoopCompletionBarrier;
let mut lifecycle = NoopRunLifecycleObserver;
let capability_probe = DockerRuntimeCapabilityProbe;
let mut dependencies = RunDependencies {
    runtime,
    supervisor,
    cancellation,
    clock,
    barrier: &mut barrier,
    lifecycle: &mut lifecycle,
    storage_probe,
    capability_probe: &capability_probe,
    runtime_preflight: None,
};
execute_local_run_with_dependencies(request, &mut dependencies)
```

The runtime-preflight wrapper sets `runtime_preflight: Some(runtime_preflight)`.
The injected capability-probe wrapper uses the caller's probe. Do not change
which wrappers choose `SystemStorageProbe`, `DockerRuntimeCapabilityProbe`, or
noop barrier/lifecycle values. The remaining wrappers are covered by the named
runtime-preflight, resource-pressure, storage-preflight, and full `run::tests`
targets in Step 5; do not generalize the default-wrapper byte comparison beyond
that one adapter.

- [ ] **Step 4: Route receipt-only execution through the same dependencies**

Rename the private receipt/artifact function to:

```rust
fn execute_local_receipt_and_artifacts_with_dependencies(
    request: &RunRequest<'_>,
    dependencies: &mut RunDependencies<'_>,
) -> Result<(
    ReceiptEnvelopeV1,
    Vec<crate::receipt::ArtifactEvidence>,
    Option<crate::runtime::RuntimeCapabilityEvidenceV1>,
), RunError>
```

Inside it, replace only parameter references with the corresponding dependency
field. Consume preflight exactly once with
`dependencies.runtime_preflight.take()`. The public receipt-only wrapper builds
the same default dependencies as before and returns `.map(|(receipt, _, _)|
receipt)`. It must still avoid receipt publication.

- [ ] **Step 5: Verify wrapper and fault-path GREEN**

```console
rtk cargo test --locked --offline --lib public_run_wrapper_matches_dependency_seam_bytes
rtk cargo test --locked --offline --lib runtime_preflight
rtk cargo test --locked --offline --lib resource_pressure
rtk cargo test --locked --offline --lib storage_preflight
rtk cargo test --locked --offline --lib run::tests
```

Expected: all selected tests pass; no Docker or network.

- [ ] **Step 6: Commit Task 2**

```console
rtk git add src/run.rs
rtk git commit -m "refactor: route run wrappers through dependencies"
```

### Task 3: Prove strict compatibility and close M1

**Files:**
- Modify: `docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/progress.md`

**Interfaces:**
- Consumes: M0 compatibility manifest and all M1 commits.
- Produces: terminal local evidence that internal composition changed but public bytes and facade did not.

- [ ] **Step 1: Run formatting and lint for the changed production module**

```console
rtk cargo fmt --check
rtk cargo clippy --locked --offline --all-targets --all-features -- -D warnings
```

Expected: PASS. Clippy may compile but may not use network; missing cached
dependencies stop the task rather than authorizing network.

- [ ] **Step 2: Run the M0 compatibility contract**

```console
rtk cargo test --locked --offline --test compatibility_baseline
rtk cargo test --locked --offline --test plan_cli
rtk cargo test --locked --offline --test verify_cli
rtk cargo test --locked --offline --test receipt_contract
rtk cargo test --locked --offline --test matrix_contract
rtk cargo test --locked --offline --test verification_contract
```

Expected: all tests pass with no fixture update. Any golden change is a product
regression, not an automatic baseline refresh. A suspected false positive
stops M1 and requires an explicit amendment to the M0 plan and manifest before
any fixture can change.

- [ ] **Step 3: Run the complete offline local suite**

```console
rtk cargo test --locked --offline --workspace --all-targets --all-features
```

Expected: PASS. This is local deterministic evidence, not hosted/platform or
release qualification.

- [ ] **Step 4: Independent reviews**

Request one specification-compliance review and one code-quality review. Both
must inspect the exact diff from the M0 terminal commit to the current HEAD.
Critical or Important findings block closure; Minor findings are either fixed
with focused tests or recorded with owner-approved rationale.

- [ ] **Step 5: Update progress and commit closure**

Record exact HEAD, commands, counts, review verdicts, unchanged fixtures, and
remaining hosted-CI gate. Then:

```console
rtk git add docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/progress.md
rtk git commit -m "docs: close run application seam milestone"
```

M1 is locally complete only after the worktree is clean and every M0 manifest
hash still matches. Push and hosted CI remain external exact-head gates.
