# Terminal owned-resource release design

Status: approved by owner on 2026-08-26  
Date: 2026-08-26  
Baseline: `2b4b55ce1a4be0a2b610656ae4a56a7641b29f26`  
Scope: make terminal cleanup and admission release ordering explicit and
uniform across `run`, `benchmark`, and `guard exec`

## Decision

CCP will add one private, family-neutral terminal finalization primitive and
three thin command-family adapters.

The shared primitive will enforce this order after admission has been
acquired:

1. finish the command family's owned workload and containment cleanup;
2. complete any owned terminal barrier, including watchdog join;
3. release admission exactly once;
4. return the primary outcome when release succeeds;
5. return the release failure when release fails, even if the primary outcome
   was already a failure.

The primitive will be pure and closure-driven. Tests can therefore prove the
ordering, release count, and failure precedence without admission state,
Docker, child processes, resource probes, network access, or host mutation.

The change consolidates existing behavior. It does not create a new process
manager, Docker cleanup mechanism, admission authority, or recovery path.

## Problem and evidence

The three heavy command families already release admission after their work,
but they encode the terminal contract separately.

- `benchmark` acquires admission and calls `release_admission` after its
  resource pre-start check and native workload (`src/main.rs:539-555`). It has
  no mid-workload resource watchdog.
- historical `run` joins and reconciles its watchdog before releasing
  admission (`src/main.rs:1110-1142`). Its pre-start and current-directory
  failures release through separate inline branches (`src/main.rs:1077-1097`).
- matrix `run` repeats the same terminal structure around
  `src/main.rs:1215-1290`.
- `guard exec` owns an admission guard, watchdog barrier, and optional resource
  observation in `GuardExecSession` (`src/main.rs:1812-1816`). Its explicit
  `finish` path joins the watchdog, records the observation, takes the guard,
  and releases it (`src/main.rs:1919-1944`).
- `finalize_guard_exec_result` already provides a pure release closure and
  makes release failure override watchdog or child outcomes
  (`src/main.rs:2045-2071`).
- `GuardExecSession::Drop`, `WatchdogCompletionBarrier::Drop`, and
  `AdmissionGuard::Drop` are best-effort safety fallbacks. They are not
  evidence-producing terminal paths (`src/main.rs:2073-2141` and
  `src/admission.rs:1093-1096`).

This duplication makes a later edit capable of releasing the host-wide slot
before a watchdog has joined, releasing twice on a fallback path, or applying
different failure precedence to different command families.

The defect is contract drift risk, not evidence of a currently leaked slot.
The design must not reinterpret a historical journal, ticket, lease, or
receipt.

## Goals

- Give `run`, matrix `run`, `benchmark`, and `guard exec` one explicit terminal
  ordering contract after admission acquisition.
- Prove that a watchdog or other terminal barrier completes before admission
  release.
- Prove that explicit admission release occurs exactly once on every covered
  terminal outcome.
- Preserve fail-closed precedence: a release failure overrides a successful or
  failed primary result because slot ownership is then uncertain.
- Cover success, workload or child failure, timeout, user cancellation,
  resource-pressure cancellation or watchdog trip, watchdog join failure, and
  release failure with deterministic tests.
- Preserve existing command-specific exit codes, journal transitions, resource
  history, receipts, and output schemas.

## Non-goals

- No real CCP heavy command, Docker invocation, child process, network request,
  or host resource probe is part of the unit contract.
- No manual process termination, swap manipulation, compressor manipulation,
  admission-root editing, lock removal, ticket removal, lease rewriting,
  journal rewriting, or receipt rewriting.
- No attempt to supervise processes or containers that the current CCP
  lifecycle did not create.
- No change to admission policy thresholds, queue ordering, heartbeat format,
  resource history schema, receipt schema, or public exit-code assignments.
- No new watchdog for the native `benchmark` loop in this tranche.
- No promise that an interrupted process can always publish terminal evidence;
  RAII remains a best-effort fallback for unwinding and process-local exits.

## Ownership boundary

The terminal barrier covers only active execution resources owned by the
current CCP command:

- its admission ticket, slot, lease, heartbeat, and related locks;
- its supervised child process tree;
- its watchdog thread and resource observation;
- Docker containers created by that command's `DockerLifecyclePlan`;

Managed-cache pins, source snapshots, and command-local workspaces remain
CCP-owned, but keep their existing independent RAII and lifecycle ordering.
In particular, `guard exec` cache pins remain held through admission release,
and successful `run` source-snapshot cleanup continues after admission release.
This tranche must not silently reorder either lifecycle.

It does not cover:

- unrelated host processes;
- swap, compressor, or other operating-system-wide memory state;
- Docker or OrbStack containers not created by the current lifecycle;
- undeclared paths embedded in opaque `guard exec` child arguments;
- a ticket or lease owned by another process merely because that owner is not
  visible in a local `ps` sample.

Ownership is determined by CCP state and the current lifecycle, not by process
name matching.

## Alternatives considered

### A. Keep the three implementations separate and add only more tests

Rejected. Tests would document the current duplication but would not prevent
future semantic drift between command families.

### B. Build a new RAII session abstraction for every heavy command

Rejected for this tranche. A broad ownership rewrite would touch admission,
journaling, process supervision, runtime cleanup, and receipt publication at
once. The existing types already own these resources correctly; the missing
piece is one terminal ordering seam.

### C. One pure finalizer with thin family adapters

Selected. It reuses current ownership types, makes the failure contract
testable without the host, and limits production edits to terminal call sites.

## Shared primitive

The implementation will introduce a private generic result that distinguishes
a primary command failure from an admission release failure. Its exact Rust
names may be refined during the TDD plan, but its semantics are fixed:

```rust
enum TerminalFailure<P, R> {
    Primary(P),
    Release(R),
}

fn finalize_owned_terminal<T, P, R>(
    primary: Result<T, P>,
    complete_owned: impl FnOnce(Result<T, P>) -> Result<T, P>,
    release: impl FnOnce() -> Result<(), R>,
) -> Result<T, TerminalFailure<P, R>>;
```

`complete_owned` runs exactly once before `release`. It is the family adapter's
terminal barrier:

- for historical `run`, it joins the watchdog and reconciles its join error or
  trip with the run outcome;
- for matrix `run`, it preserves the execution barrier's existing trip
  semantics, joins the watchdog, and promotes any join error over the matrix
  outcome;
- for `guard exec`, it joins the watchdog, preserves resource-observation
  ordering, and applies the existing watchdog, cancellation, and child-result
  classification;
- for `benchmark`, it is an identity completion because the current native
  workload has no watchdog.

`release` runs exactly once after completion. If it succeeds, the completed
primary result is returned. If it fails, `TerminalFailure::Release` is
returned regardless of the primary result.

Using distinct primary and release error types lets each command adapter retain
its current error mapping and side effects. In particular, `run` can mark its
journal `cleanup-pending` for a release failure in the shared terminal adapter
and can record the normal failure kind only after successful release. Existing
later cleanup paths may independently use `cleanup-pending`.

The primitive does not own or drop the real admission guard. Each adapter moves
its guard into the one-shot release closure. Rust ownership and `FnOnce`
prevent a second explicit release from the same terminal path.

## Command-family adapters

### `benchmark`

After admission acquisition, every primary result passes through the shared
primitive. The adapter uses identity completion and maps a release failure to
`CliError::Admission`. Existing output and optional receipt writing remain
after successful finalization.

Admission acquisition failure remains outside the finalizer because no guard
is owned.

### historical and matrix `run`

Every path after successful admission acquisition passes through the same run
adapter, including resource pre-start failure and failures that occur before a
watchdog is created. A barrier with no watchdog is a valid no-op completion.

The adapter performs this sequence:

1. ensure the completion barrier has joined at most once;
2. apply the path's existing watchdog semantics: historical `run` reconciles
   join error, trip, and resource-pressure evidence; matrix `run` preserves the
   barrier's trip result and promotes a join error over the matrix outcome;
3. release the admission guard once;
4. on release failure, transition the journal to `cleanup-pending` and return
   `CliError::Admission`;
5. on primary failure after successful release, record the existing terminal
   failure kind and return that primary error;
6. on success, continue existing source-snapshot cleanup, sealing, and receipt
   behavior.

Historical and matrix paths may share one adapter if their lifecycle types and
borrow rules permit it. If not, both adapters must delegate the ordering and
precedence to the same primitive; they must not duplicate the primitive.

The cleanup-pending transition is itself fallible and remains outside the pure
primitive. If admission release fails, the adapter first attempts that journal
transition. A transition failure retains the existing `CliError::RunJournal`
precedence because the durable terminal record is then also uncertain;
otherwise the adapter returns the admission release failure. Neither case may
claim that the slot was released. This rule applies uniformly to resource
pre-start, current-directory, workload, watchdog, and other failures after
admission acquisition.

### `guard exec`

`GuardExecSession::finish` remains the evidence-producing explicit terminal
path. Resource observation must still be persisted after watchdog join and
before admission release.

The current `finalize_guard_exec_result` classification becomes the guard
adapter's completion logic and delegates release precedence to the shared
primitive. The `Option::take` guard ownership remains, so `Drop` sees no guard
after explicit completion and cannot release it again.

Early managed-cache, resource pre-start, pin acquisition, and child execution
failures continue to call `finish`. Cache pins remain alive through explicit
session finish and admission release.

The guard adapter does not move pin ownership into the shared primitive. A
focused adapter test must retain an instrumented pin owner around finalization
and prove that it drops only after the release closure has run.

## Terminal ordering

The normative order for the active terminal barrier of an admitted command is:

```text
owned workload terminates
  -> child/container containment cleanup completes or fails
  -> watchdog joins and terminal observation is captured
  -> admission release is attempted exactly once
  -> command-specific journal/exit/receipt handling continues
```

A command must not report the slot as released merely because its child has
exited or because no matching process appears in `ps`.

Docker lifecycle cleanup remains inside the runtime/process primary outcome.
If Docker cleanup fails, that uncertainty becomes the primary failure before
the terminal finalizer runs. If admission release then also fails, the release
failure wins because host-wide ownership is still uncertain.

Auxiliary RAII resources that intentionally outlive slot release are not part
of this ordering. Existing cache-pin, source-snapshot, workspace, and receipt
lifecycles remain unchanged.

## Failure precedence

The following table is normative:

| Completed primary outcome | Admission release | Final outcome |
| --- | --- | --- |
| success | success | success |
| workload or child failure | success | original failure |
| timeout | success | timeout |
| user cancellation | success | cancellation |
| resource-pressure cancellation or trip | success | resource failure |
| watchdog join failure | success | watchdog failure |
| any of the above | failure | admission release failure |

Within the completed primary outcome, existing family-specific precedence is
preserved. For `guard exec`, watchdog join error precedes trip, resource
pressure, and child outcome. For `run`, the existing reconciliation rules for
a receipt already containing resource-pressure `NOT_RUN` evidence remain
unchanged.

## Fallback behavior

RAII `Drop` implementations remain safety fallbacks:

- watchdog drop joins at most once;
- session or admission-guard drop attempts best-effort release only while it
  still owns the guard;
- explicit finalization consumes or takes the guard, so subsequent drop cannot
  perform a second release.

A suppressed error from `Drop` is not terminal proof. Normal command paths
must use explicit finalization so cleanup failure can be returned and, for
`run`, journaled as `cleanup-pending`.

## Deterministic TDD contract

Tests will be serial and use fake closures or small fake adapters only. An
event trace will assert `complete` occurs before `release`, and an atomic or
cell counter will assert release is called exactly once.

The table-driven contract must cover each adapter for:

- success;
- workload or child failure;
- timeout;
- user cancellation;
- resource-pressure cancellation or watchdog trip;
- watchdog join failure where the family has a watchdog;
- release failure overriding every representative primary outcome;
- explicit finalization followed by drop without a second release;
- for `guard exec`, cache-pin ownership remaining live through completion and
  admission release.

Existing process-tree and Docker lifecycle tests remain the source of truth for
real child timeout containment and container cleanup. This tranche will not add
a fake Docker implementation to the shared terminal primitive because Docker
cleanup must finish before the primitive is entered.

At least one cross-family contract test must exercise the same shared
primitive through all three adapters. Tests that independently duplicate the
expected ordering in three helpers are insufficient.

## Documentation changes

After tests and implementation are green, bounded updates will be made to:

- `docs/LOCAL_RUN.md`: terminal order, benchmark watchdog exception, and
  release-failure precedence;
- `docs/COORDINATION_RUNBOOK.md`: a terminal child result is not a release
  handoff; release uncertainty remains fail-closed;
- `docs/ARCHITECTURE.md`: shared terminal finalization contract and ownership
  boundary;
- `docs/TESTING_AND_FAULT_INJECTION.md`: deterministic fault matrix and the
  separation between fake terminal tests and real process/runtime tests.

No public documentation may claim qualification beyond the exact tests that
were executed.

## Verification boundary

The implementation tranche will require, at minimum:

1. focused RED/GREEN tests for the shared primitive and all adapters;
2. existing guard-exec, admission, process-supervisor, run-journal, runtime,
   and repository-hygiene tests relevant to the touched paths;
3. `cargo fmt --check`;
4. strict Clippy with the repository-pinned toolchain and all targets/features;
5. the complete native test suite and doctests;
6. `git diff --check` and an exact-head scoped review.

These are local source checks. They do not themselves authorize or constitute
a CCP `run`, Docker workload, evidence publication, push, pull request, or
merge.

## Stop boundaries

The written design must be reviewed and locally committed before the TDD
implementation plan is written. The implementation plan must then be reviewed
before production code changes begin.

No push, pull request, CCP heavy command, Docker workload, evidence
publication, or merge is authorized by this design.
