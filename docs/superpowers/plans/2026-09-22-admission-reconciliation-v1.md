# Admission Reconciliation V1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Safely reconcile explicitly selected abandoned admission tickets without starting a workload.

**Architecture:** `AdmissionCoordinator` owns preview, lock acquisition, revalidation, and durable mutation. `main.rs` adds a thin CLI adapter. A new reconciliation report is versioned separately from frozen `AdmissionStatusV1`; acquisition and run-journal recovery remain unchanged.

**Tech Stack:** Rust 2024, `clap`, `serde`, `fs2`, standard filesystem APIs, deterministic unit and CLI tests.

**Spec:** `docs/superpowers/specs/2026-09-22-admission-reconciliation-v1-design.md`

## Global Constraints

- Preview is read-only and never initializes an absent coordinator root.
- Apply requires `--apply` plus one or more exact `--ticket-id` values; it never selects all tickets.
- Apply holds queue and slot exclusion, locks every selected ticket, then rereads every record before mutation.
- Only a valid CCP-owned unlocked selected ticket with an absent or definitely expired lease is eligible.
- Preserve status schema `2.0`, existing on-disk schemas, `recover`, receipt, cache, `run`, `benchmark`, and `guard exec` behavior.
- JSON contains only bounded opaque IDs and fixed categories, never paths, commands, environments, process inventory, raw errors, or logs.
- No network, Docker workload, CCP heavy command, release, installation, or manual coordinator cleanup belongs to this plan.

## Review Focus

1. Preview evidence changing before apply must block; Task 3 tests it.
2. Free slot plus lease residue remains blocking; Task 2 tests it.
3. Quarantine collision must preserve evidence; Task 3 tests it.
4. Lease/move fault must be truthful and release locks; Task 3 tests it.
5. CLI must not widen status/recover contracts; Task 4 tests it.

---

### Task 1: Read-only preview model

**Files:**
- Modify: `src/admission.rs:90-360`
- Test: `src/admission.rs` unit tests

**Interfaces:**
- Produces `AdmissionReconciliationReportV1`, `AdmissionReconciliationModeV1`, `AdmissionReconciliationCandidateV1`, and `AdmissionCoordinator::reconcile_preview_with_timeout`.

- [ ] **Step 1: Write RED test for an eligible absent lease and byte preservation.**

```rust
#[test]
fn reconciliation_preview_is_read_only_and_classifies_an_absent_lease() {
    let coordinator = coordinator("reconcile-preview");
    coordinator.initialize().expect("initialize");
    let id = write_unlocked_valid_ticket(&coordinator, None);
    let before = tree_bytes(coordinator.root());
    let report = coordinator.reconcile_preview_with_timeout(
        Duration::from_secs(1), &CancellationToken::default(),
    ).expect("preview");
    assert_eq!(report.mode, AdmissionReconciliationModeV1::Preview);
    assert_eq!(report.candidates, vec![candidate(&id, "eligible_absent_lease")]);
    assert_eq!(before, tree_bytes(coordinator.root()));
}
```

- [ ] **Step 2: Run RED.**

Run: `cargo test --locked reconciliation_preview_is_read_only_and_classifies_an_absent_lease -- --exact`

Expected: compile failure; the API does not exist.

- [ ] **Step 3: Implement the smallest preview surface.**

```rust
pub fn reconcile_preview_with_timeout(
    &self, timeout: Duration, cancellation: &CancellationToken,
) -> Result<AdmissionReconciliationReportV1, AdmissionReconciliationError>;
```

Use existing strict owner/layout/ticket/lease readers and queue deadline. Do not
call `initialize_until`, `scan_tickets(..., true)`, `remove_stale`,
`quarantine_file`, `remove_lease`, or `create_ticket`.

- [ ] **Step 4: Add RED tests for blocked shapes.**

Create three exact fixtures: a valid ticket retained under an exclusive lock
with a future lease; an absent root whose parent fingerprint is identical
before and after preview; and each of a symlinked ticket plus a valid-JSON
foreign ticket. Assert a blocked category or strict error as appropriate and
byte-identical coordinator state in every case.

- [ ] **Step 5: Run GREEN and commit.**

Run: `cargo test --locked reconciliation_preview -- --nocapture`

Then: `git add src/admission.rs && git commit -m "feat: add read-only admission reconciliation preview"`

### Task 2: Exact-target apply and lock discipline

**Files:**
- Modify: `src/admission.rs:236-310,620-810`
- Test: `src/admission.rs` unit tests

**Interfaces:**
- Consumes Task 1 report types.
- Produces `reconcile_apply_with_timeout(&[String], Duration, &CancellationToken)` and `AdmissionReconciliationOutcomeV1`.

- [ ] **Step 1: Write RED exact-target test.**

```rust
#[test]
fn reconciliation_apply_quarantines_only_the_selected_expired_ticket() {
    let coordinator = coordinator("reconcile-target");
    coordinator.initialize().expect("initialize");
    let selected = write_unlocked_valid_ticket(&coordinator, Some(expired_lease()));
    let untouched = write_unlocked_valid_ticket(&coordinator, Some(future_lease()));
    let counter = fs::read(coordinator.root().join(NEXT_TICKET)).expect("counter");
    let report = coordinator.reconcile_apply_with_timeout(
        &[selected.clone()], Duration::from_secs(1), &CancellationToken::default(),
    ).expect("apply");
    assert_eq!(report.outcomes, vec![outcome(&selected, "quarantined")]);
    assert!(!ticket_path(&coordinator, &selected).exists());
    assert!(ticket_path(&coordinator, &untouched).exists());
    assert_eq!(counter, fs::read(coordinator.root().join(NEXT_TICKET)).expect("counter"));
}
```

- [ ] **Step 2: Run RED.**

Run: `cargo test --locked reconciliation_apply_quarantines_only_the_selected_expired_ticket -- --exact`

Expected: compile failure; apply does not exist.

- [ ] **Step 3: Implement prevalidation before any mutation.**

Validate non-empty, unique canonical selected IDs. Hold bounded queue lock and
retain slot lock. Open/lock every requested ticket, reread ticket and lease
under locks, and reject the whole request for unknown, held, active/future,
foreign, malformed, unsafe, lease-only, or contradictory state.

- [ ] **Step 4: Write and run lock RED/GREEN tests.**

Create a held selected ticket fixture and capture its bytes before apply; create
a free slot lock plus a valid selected expired lease residue; and invoke apply
with duplicate, malformed, and omitted target IDs. Each must return a bounded
blocked or usage result and preserve every ticket, lease, and counter byte.

Run: `cargo test --locked reconciliation_apply -- --nocapture`

- [ ] **Step 5: Commit.**

`git add src/admission.rs && git commit -m "feat: serialize targeted admission reconciliation"`

### Task 3: Durable no-replace mutation and failure reporting

**Files:**
- Modify: reconciliation-only helpers in `src/admission.rs`
- Test: `src/admission.rs` unit tests

**Interfaces:**
- Produces a private no-replace quarantine helper and per-ticket success/blocked/partial result.

- [ ] **Step 1: Write RED collision test.**

```rust
#[test]
fn reconciliation_never_overwrites_an_existing_quarantine_record() {
    let coordinator = coordinator("reconcile-collision");
    let id = write_unlocked_valid_ticket(&coordinator, None);
    write_existing_quarantine_name(&coordinator, &id, b"preserve me");
    let result = coordinator.reconcile_apply_with_timeout(
        &[id], Duration::from_secs(1), &CancellationToken::default(),
    );
    assert!(matches!(result, Err(AdmissionReconciliationError::QuarantineCollision)));
    assert_eq!(read_existing_quarantine_name(&coordinator), b"preserve me");
}
```

- [ ] **Step 2: Run RED.**

Run: `cargo test --locked reconciliation_never_overwrites_an_existing_quarantine_record -- --exact`

Expected: failure; the existing broad rename helper is overwrite-capable.

- [ ] **Step 3: Implement the reconciliation-only durable sequence.**

Remove an eligible lease first, then move the ticket only through an atomic
no-replace destination. If the host lacks such a primitive, return fail-closed;
never fall back to overwrite-capable rename. If move fails after lease removal,
preserve the ticket with no lease and report partial.

- [ ] **Step 4: Add RED fault/race tests, then GREEN.**

Inject one ticket-move failure after a successful lease removal and assert a
partial result plus residual no-lease ticket. Change selected ticket bytes after
preview but before apply and assert apply blocks. Hold reconciliation at a
deterministic barrier after queue/slot acquisition, start ordinary acquisition
on another thread, assert it cannot enter, then release the barrier and assert
both paths release their locks.

Run: `cargo test --locked reconciliation_ -- --nocapture`

Assert all owned locks release and counter, journals, cache, and unselected
tickets stay unchanged after every injected failure.

- [ ] **Step 5: Commit.**

`git add src/admission.rs && git commit -m "feat: preserve evidence during admission reconciliation"`

### Task 4: CLI, JSON privacy, and compatibility

**Files:**
- Modify: `src/main.rs:467-500,1992-2025`
- Create: `tests/admission_cli.rs`
- Test: `src/admission.rs` status schema test

**Interfaces:**
- Produces `admission reconcile [--json] [--timeout-seconds N]` and the apply-only `--ticket-id` shape.

- [ ] **Step 1: Write RED CLI tests.**

Invoke the test binary against an owned fixture root through an existing
test-only platform-root seam. Snapshot fixture bytes before/after preview and
assert JSON lacks its absolute root. Assert `--ticket-id` without `--apply`,
`--apply` without an ID, malformed IDs, and duplicate IDs exit `2`. Invoke
`admission status --json` and assert its object still contains exactly seven
fields.

- [ ] **Step 2: Run RED.**

Run: `cargo test --locked --test admission_cli -- --nocapture`

Expected: command/help failure because `reconcile` is not registered.

- [ ] **Step 3: Add narrow parser and renderer.**

```rust
Reconcile {
    apply: bool,
    ticket_ids: Vec<String>,
    json: bool,
    timeout_seconds: u64,
}
```

Reject ticket IDs without `--apply`; reject `--apply` without IDs. Route only
to Task 1 preview or Task 2 apply. Serialize the new report; preserve status
output and existing exit mappings.

- [ ] **Step 4: Run GREEN and compatibility suite.**

Run: `cargo test --locked --test admission_cli && cargo test --locked status_schema_contains_only_bounded_safe_fields -- --exact && cargo test --locked --test compatibility_baseline`

- [ ] **Step 5: Commit.**

`git add src/main.rs src/admission.rs tests/admission_cli.rs && git commit -m "feat: expose explicit admission reconciliation"`

### Task 5: Operator documentation and final gate

**Files:**
- Modify: `docs/COORDINATION_RUNBOOK.md`, `docs/TROUBLESHOOTING.md`, `docs/LOCAL_RUN.md`, `CHANGELOG.md`
- Review: all files changed by Tasks 1-4

- [ ] **Step 1: Write RED documentation contract.**

```rust
#[test]
fn reconciliation_docs_preserve_the_no_manual_deletion_boundary() {
    assert!(COORDINATION_RUNBOOK.contains("admission reconcile --ticket-id"));
    assert!(TROUBLESHOOTING.contains("active: false"));
    assert!(LOCAL_RUN.contains("recover status/apply"));
}
```

- [ ] **Step 2: Run RED, then update docs.**

Run: `cargo test --locked reconciliation_docs_preserve_the_no_manual_deletion_boundary -- --exact`

Document: preserve status; preview; obtain separate exact apply authorization;
inspect bounded result; rerun fresh status, Docker, and resource checks. State
that `recover` is journal-only, `active: false` never overrides `unknown`, and
manual deletion remains unsupported.

- [ ] **Step 3: Run GREEN and full deterministic verification.**

Run: `cargo fmt --check && cargo test --locked --workspace --all-targets --all-features && git diff --check origin/main...HEAD`

- [ ] **Step 4: Request a fresh read-only whole-branch review.**

Give the reviewer exact base/head, this plan, the spec, and diff. Fix every
Critical or Important finding through a new RED-GREEN cycle, then rerun the
full verification command.

- [ ] **Step 5: Commit and stop at external gates.**

`git add docs/COORDINATION_RUNBOOK.md docs/TROUBLESHOOTING.md docs/LOCAL_RUN.md CHANGELOG.md src/admission.rs && git commit -m "docs: explain explicit admission reconciliation"`

Do not create the public issue, push, open a PR, run CCP, install a producer,
publish evidence, or merge without separate exact authorization.
