# Terminal Owned-Resource Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give `run`, matrix `run`, `benchmark`, and `guard exec` one deterministic terminal barrier that completes owned execution cleanup before exactly one admission-release attempt and fails closed when release is uncertain.

**Architecture:** Add a private generic finalizer in a focused binary module, then keep command-specific classification and journaling in thin adapters in `src/main.rs`. The generic finalizer owns only ordering and precedence; existing admission, watchdog, journal, cache-pin, process-supervisor, Docker, snapshot, workspace, and receipt types retain their current ownership and schemas.

**Tech Stack:** Rust 1.96.0, Cargo locked resolution, built-in unit and integration test harnesses, Clippy, rustfmt.

**Spec:** `docs/superpowers/specs/2026-08-26-terminal-owned-resource-release-design.md`

## Global Constraints

- Work only in `/private/tmp/ccp-terminal-resource-release-v1` on branch `feat/terminal-resource-release-v1` based on `2b4b55ce1a4be0a2b610656ae4a56a7641b29f26`.
- Preserve `/Users/marco1/Documents/CODICE con VS CODE/commit-ci-preflight` and the public-documentation and static-analysis worktrees unchanged.
- Start every shell command with `rtk`.
- Use TDD: add the named failing test, observe the expected RED failure, implement the minimum production change, then observe GREEN.
- Use only deterministic fake closures or existing local fixtures for the new terminal contract. Do not invoke real admission coordination, child processes, Docker, network access, CCP heavy commands, or host resource probes in the new unit tests.
- Do not manually terminate processes, manipulate swap/compressor state, or edit/remove admission tickets, leases, locks, journals, cache ownership, or receipts.
- Preserve existing CLI exit codes, serialized schemas, resource-history behavior, receipt behavior, source-snapshot ordering, and cache-pin lifetime.
- Admission release failure overrides a completed primary result. If the subsequent `run` cleanup-pending journal transition also fails, `CliError::RunJournal` retains precedence and no release claim is allowed.
- `benchmark` retains pre-start admission only; this tranche does not add a mid-workload benchmark watchdog.
- Do not push, open a pull request, run CCP, publish evidence, merge, or clean branches/evidence.

## File Structure

- Create `src/terminal.rs`: private family-neutral `TerminalFailure` and `finalize_owned_terminal`, plus its deterministic unit tests.
- Modify `src/main.rs`: declare the private module; add thin benchmark and run adapters; route guard, historical run, and matrix run through the shared primitive; add adapter and cache-pin lifetime tests.
- Modify `docs/LOCAL_RUN.md`: operator-facing terminal order and benchmark exception.
- Modify `docs/COORDINATION_RUNBOOK.md`: exact criteria for claiming slot release and cleanup uncertainty.
- Modify `docs/ARCHITECTURE.md`: shared finalization architecture and ownership boundary.
- Modify `docs/TESTING_AND_FAULT_INJECTION.md`: deterministic fault matrix and separation from real process/runtime tests.

---

### Task 1: Private family-neutral terminal primitive

**Files:**
- Create: `src/terminal.rs`
- Modify: `src/main.rs:14-70`

**Interfaces:**
- Consumes: a primary `Result<T, P>`, one `FnOnce` completion closure, and one `FnOnce` release closure.
- Produces: `terminal::finalize_owned_terminal<T, P, R>` and `terminal::TerminalFailure<P, R>`, both visible only to the parent binary module.

- [ ] **Step 1: Verify the exact implementation worktree before editing**

Run:

```bash
rtk git status --short --branch
rtk git rev-parse HEAD
rtk git rev-parse HEAD^
rtk git rev-parse origin/main
```

Expected: clean `feat/terminal-resource-release-v1`, HEAD
is the reviewed plan commit supplied in the execution handoff, its first parent
is design commit `056ceea228178c6df5cadb1c843f16cfe7fd63d2`, and `origin/main` is
`2b4b55ce1a4be0a2b610656ae4a56a7641b29f26`.

- [ ] **Step 2: Add the module declaration and failing primitive tests**

Add this declaration after the standard header and before the imports in
`src/main.rs`:

```rust
mod terminal;
```

Create `src/terminal.rs` with tests that name the not-yet-implemented API:

```rust
#[cfg(test)]
mod tests {
    use super::{TerminalFailure, finalize_owned_terminal};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    struct FakeGuard {
        releases: Rc<Cell<usize>>,
        released: bool,
    }

    impl FakeGuard {
        fn release(mut self) {
            if !self.released {
                self.releases.set(self.releases.get() + 1);
                self.released = true;
            }
        }
    }

    impl Drop for FakeGuard {
        fn drop(&mut self) {
            if !self.released {
                self.releases.set(self.releases.get() + 1);
                self.released = true;
            }
        }
    }

    #[test]
    fn completion_precedes_exactly_one_release() {
        let events = RefCell::new(Vec::new());
        let releases = Cell::new(0);

        let result = finalize_owned_terminal(
            Ok::<_, &'static str>(7_u8),
            |primary| {
                events.borrow_mut().push("complete");
                primary
            },
            || {
                events.borrow_mut().push("release");
                releases.set(releases.get() + 1);
                Ok::<_, &'static str>(())
            },
        );

        assert_eq!(result, Ok(7));
        assert_eq!(&*events.borrow(), &["complete", "release"]);
        assert_eq!(releases.get(), 1);
    }

    #[test]
    fn primary_failure_survives_successful_release() {
        let result = finalize_owned_terminal(
            Err::<(), _>("workload"),
            |primary| primary,
            || Ok::<_, &'static str>(()),
        );

        assert_eq!(result, Err(TerminalFailure::Primary("workload")));
    }

    #[test]
    fn release_failure_overrides_success_or_primary_failure() {
        for primary in [Ok(()), Err("workload")] {
            let result = finalize_owned_terminal(
                primary,
                |primary| primary,
                || Err::<(), _>("release"),
            );

            assert_eq!(result, Err(TerminalFailure::Release("release")));
        }
    }

    #[test]
    fn completion_failure_still_releases_once() {
        let releases = Cell::new(0);
        let result = finalize_owned_terminal(
            Ok::<_, &'static str>(()),
            |_| Err("watchdog"),
            || {
                releases.set(releases.get() + 1);
                Ok::<_, &'static str>(())
            },
        );

        assert_eq!(result, Err(TerminalFailure::Primary("watchdog")));
        assert_eq!(releases.get(), 1);
    }

    #[test]
    fn explicit_release_consumes_guard_without_drop_release() {
        let releases = Rc::new(Cell::new(0));
        let guard = FakeGuard {
            releases: Rc::clone(&releases),
            released: false,
        };

        let result = finalize_owned_terminal(
            Ok::<_, &'static str>(()),
            |primary| primary,
            || {
                guard.release();
                Ok::<_, &'static str>(())
            },
        );

        assert_eq!(result, Ok(()));
        assert_eq!(releases.get(), 1);
    }
}
```

- [ ] **Step 3: Run the focused test module and confirm RED**

Run:

```bash
rtk cargo test --offline --locked --bin commit-ci-preflight terminal::tests::
```

Expected: compilation fails because `TerminalFailure` and
`finalize_owned_terminal` do not yet exist. A failure caused by toolchain,
dependency, or filesystem access is inconclusive and must not be accepted as
the RED result.

- [ ] **Step 4: Implement the minimum pure primitive above the tests**

Add to `src/terminal.rs`:

```rust
#[derive(Debug, PartialEq, Eq)]
pub(super) enum TerminalFailure<P, R> {
    Primary(P),
    Release(R),
}

pub(super) fn finalize_owned_terminal<T, P, R>(
    primary: Result<T, P>,
    complete_owned: impl FnOnce(Result<T, P>) -> Result<T, P>,
    release: impl FnOnce() -> Result<(), R>,
) -> Result<T, TerminalFailure<P, R>> {
    let completed = complete_owned(primary);
    match release() {
        Ok(()) => completed.map_err(TerminalFailure::Primary),
        Err(error) => Err(TerminalFailure::Release(error)),
    }
}
```

- [ ] **Step 5: Run focused GREEN and formatting**

Run:

```bash
rtk cargo test --offline --locked --bin commit-ci-preflight terminal::tests::
rtk cargo fmt --check
rtk git diff --check
```

Expected: five primitive tests pass; formatting and diff checks exit 0.

- [ ] **Step 6: Commit the independently green primitive**

Run:

```bash
rtk git add src/main.rs src/terminal.rs
rtk git commit -m "refactor: add terminal finalization primitive"
```

Expected: one local commit containing only the private module and declaration.

---

### Task 2: Benchmark and guard-exec adapters

**Files:**
- Modify: `src/main.rs:532-574`
- Modify: `src/main.rs:681-751`
- Modify: `src/main.rs:1804-1810`
- Modify: `src/main.rs:1919-1944`
- Modify: `src/main.rs:2045-2071`
- Test: `src/main.rs:2551-3285`

**Interfaces:**
- Consumes: `terminal::finalize_owned_terminal` and
  `terminal::TerminalFailure` from Task 1.
- Produces: private `finalize_benchmark_terminal<T>` and a
  `finalize_guard_exec_result` adapter that delegates release precedence to the
  shared primitive.

- [ ] **Step 1: Import the shared primitive and add failing adapter tests**

Add to the `src/main.rs` imports:

```rust
use terminal::{TerminalFailure, finalize_owned_terminal};
```

Add `finalize_benchmark_terminal` to the test module's `use super::{...}` list,
import `AdmissionError`, then add:

```rust
#[test]
fn benchmark_terminal_preserves_primary_and_release_precedence() {
    let releases = AtomicUsize::new(0);
    let primary = finalize_benchmark_terminal(Err::<(), _>(CliError::Benchmark(
        commit_ci_preflight::benchmark::BenchmarkError::NoSamples,
    )), || {
        releases.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    assert!(matches!(primary, Err(CliError::Benchmark(_))));

    let release = finalize_benchmark_terminal(Ok(()), || {
        releases.fetch_add(1, Ordering::SeqCst);
        Err(AdmissionError::Clock)
    });
    assert!(matches!(release, Err(CliError::Admission(AdmissionError::Clock))));
    assert_eq!(releases.load(Ordering::SeqCst), 2);
}
```

Extend `guard_exec_finalization_releases_once_for_success_error_and_resource_pressure`
with a release-failure case:

```rust
let release_failure = finalize_guard_exec_result(
    Err(GuardExecError::InternalFailure),
    &cancellation,
    None,
    None,
    || {
        release_count.fetch_add(1, Ordering::SeqCst);
        Err(GuardExecError::Admission(AdmissionError::Clock))
    },
);
assert!(matches!(
    release_failure,
    Err(GuardExecError::Admission(AdmissionError::Clock))
));
assert_eq!(release_count.load(Ordering::SeqCst), 4);
```

Add a cache-pin test that calls the real guard adapter inside the existing pin
scope:

```rust
#[test]
fn guard_cache_pin_remains_live_through_terminal_release() {
    let (cache, source, base) = guard_cache_fixture("terminal-release");
    let cancellation = CancellationToken::default();

    let result = super::with_guard_cache_pins(
        Some(&cache),
        std::slice::from_ref(&source),
        || {
            finalize_guard_exec_result(
                Ok(completed_process_result()),
                &cancellation,
                None,
                None,
                || {
                    assert!(matches!(
                        cache.pin_completed_sources(std::slice::from_ref(&source)),
                        Err(CacheError::LockBusy(_))
                    ));
                    Ok(())
                },
            )
        },
    );

    assert!(result.is_ok());
    assert!(
        cache
            .pin_completed_sources(std::slice::from_ref(&source))
            .is_ok()
    );
    cleanup_guard_cache_fixture(&base);
}
```

- [ ] **Step 2: Run the exact adapter tests and confirm RED**

Run:

```bash
rtk cargo test --offline --locked --bin commit-ci-preflight tests::benchmark_terminal_preserves_primary_and_release_precedence
rtk cargo test --offline --locked --bin commit-ci-preflight tests::guard_exec_finalization_releases_once_for_success_error_and_resource_pressure
rtk cargo test --offline --locked --bin commit-ci-preflight tests::guard_cache_pin_remains_live_through_terminal_release
```

Expected: the benchmark test fails to compile because
`finalize_benchmark_terminal` is missing. The extended guard and cache-pin
tests characterize behavior that may already pass once the compiler error is
resolved; they are required regression locks before refactoring.

- [ ] **Step 3: Add the benchmark adapter and replace `release_admission`**

Replace `release_admission` with:

```rust
fn finalize_benchmark_terminal<T>(
    primary: Result<T, CliError>,
    release: impl FnOnce() -> Result<(), AdmissionError>,
) -> Result<T, CliError> {
    match finalize_owned_terminal(primary, std::convert::identity, release) {
        Ok(value) => Ok(value),
        Err(TerminalFailure::Primary(error)) => Err(error),
        Err(TerminalFailure::Release(error)) => Err(CliError::Admission(error)),
    }
}
```

Change `print_benchmark` to:

```rust
let envelope = finalize_benchmark_terminal(result, || guard.release())?;
```

- [ ] **Step 4: Make guard classification the completion closure**

Refactor `finalize_guard_exec_result` without changing its signature:

```rust
fn finalize_guard_exec_result(
    result: Result<ProcessResult, GuardExecError>,
    cancellation: &CancellationToken,
    join_error: Option<ResourceProbeError>,
    trip: Option<WatchdogTripReason>,
    release: impl FnOnce() -> Result<(), GuardExecError>,
) -> Result<ProcessResult, GuardExecError> {
    match finalize_owned_terminal(
        result,
        |result| {
            if let Some(error) = join_error {
                Err(GuardExecError::Resource(ResourceGuardError::Watchdog(error)))
            } else if let Some(reason) = trip {
                Err(GuardExecError::Resource(
                    ResourceGuardError::WatchdogTripped(reason),
                ))
            } else if cancellation.reason() == Some(CancellationReason::ResourcePressure) {
                Err(GuardExecError::ResourcePressure)
            } else {
                result
            }
        },
        release,
    ) {
        Ok(value) => Ok(value),
        Err(TerminalFailure::Primary(error) | TerminalFailure::Release(error)) => Err(error),
    }
}
```

Keep `GuardExecSession::finish` as the sole watchdog-join owner: it joins once,
captures `trip` and `take_join_error()`, persists the resource observation,
takes the admission guard, then calls this adapter. The adapter's completion
closure only classifies the already-captured terminal state; it must not join
or read the barrier again. Keep the lexical cache-pin scope around
`session.finish` unchanged.

- [ ] **Step 5: Run focused GREEN and existing guard regressions**

Run:

```bash
rtk cargo test --offline --locked --bin commit-ci-preflight tests::benchmark_terminal_preserves_primary_and_release_precedence
rtk cargo test --offline --locked --bin commit-ci-preflight tests::guard_exec_finalization_releases_once_for_success_error_and_resource_pressure
rtk cargo test --offline --locked --bin commit-ci-preflight tests::guard_cache_pin_remains_live_through_terminal_release
rtk cargo test --offline --locked --bin commit-ci-preflight tests::guard_cache_pins_
rtk cargo test --offline --locked --test guard_exec_cli
rtk cargo fmt --check
rtk git diff --check
```

Expected: the named deterministic tests pass; `guard_exec_cli` compiles and
exits 0, but its native opt-in test remains ignored (0 passed, 1 ignored) and
is NOT_RUN, not PASS. No pin is released before the terminal release closure
returns.

- [ ] **Step 6: Commit the benchmark and guard adapters**

Run:

```bash
rtk git add src/main.rs
rtk git commit -m "refactor: unify benchmark and guard finalization"
```

Expected: one local commit with no documentation or unrelated formatting.

---

### Task 3: Historical and matrix run terminal adapter

**Files:**
- Modify: `src/main.rs:1064-1149`
- Modify: `src/main.rs:1215-1297`
- Modify: `src/main.rs:1354-1417`
- Test: `src/main.rs:2551-3285`

**Interfaces:**
- Consumes: `finalize_owned_terminal`, `TerminalFailure`, `CliError`,
  `AdmissionError`, `RunFailureKindV1`, and `RunJournalStateV1`.
- Produces: private `RunTerminalJournalEvent` and
  `finalize_run_terminal<T>`. Both historical and matrix run call this adapter
  after admission acquisition.

- [ ] **Step 1: Add failing tests for run release and journal precedence**

Add `finalize_run_terminal` and `RunTerminalJournalEvent` to the test module's
`use super::{...}` list, then add:

```rust
#[test]
fn run_terminal_orders_watchdog_release_and_primary_journal() {
    use std::cell::RefCell;

    let events = RefCell::new(Vec::new());
    let result = finalize_run_terminal(
        Err::<(), _>(CliError::Resource(ResourceGuardError::PreStartDenied)),
        |primary| {
            events.borrow_mut().push("complete");
            primary
        },
        || {
            events.borrow_mut().push("release");
            Ok(())
        },
        |event| {
            events.borrow_mut().push(match event {
                RunTerminalJournalEvent::PrimaryFailure(
                    RunFailureKindV1::ResourcePressure,
                ) => "journal-primary",
                _ => "unexpected-journal",
            });
            Ok(())
        },
    );

    assert!(matches!(result, Err(CliError::Resource(_))));
    assert_eq!(
        &*events.borrow(),
        &["complete", "release", "journal-primary"]
    );
}

#[test]
fn run_terminal_release_failure_journals_cleanup_pending() {
    use std::cell::RefCell;

    let events = RefCell::new(Vec::new());
    let result = finalize_run_terminal(
        Ok(()),
        |primary| primary,
        || Err(AdmissionError::Clock),
        |event| {
            events.borrow_mut().push(event);
            Ok(())
        },
    );

    assert!(matches!(result, Err(CliError::Admission(AdmissionError::Clock))));
    assert_eq!(
        &*events.borrow(),
        &[RunTerminalJournalEvent::ReleaseFailure]
    );
}

#[test]
fn run_terminal_journal_failure_overrides_release_failure() {
    let result = finalize_run_terminal(
        Ok(()),
        |primary| primary,
        || Err(AdmissionError::Clock),
        |_| Err(CliError::RunJournal(RunJournalError::InvalidTransition)),
    );

    assert!(matches!(
        result,
        Err(CliError::RunJournal(RunJournalError::InvalidTransition))
    ));
}
```

Import `AdmissionError`, `ResourceGuardError`, `RunFailureKindV1`, and
`RunJournalError` into the test module from their existing crate paths.

- [ ] **Step 2: Run the exact run-adapter tests and confirm RED**

Run:

```bash
rtk cargo test --offline --locked --bin commit-ci-preflight tests::run_terminal_
```

Expected: compilation fails because `finalize_run_terminal` and
`RunTerminalJournalEvent` do not yet exist.

- [ ] **Step 3: Implement the run adapter**

Add near the current finalization helpers:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunTerminalJournalEvent {
    PrimaryFailure(RunFailureKindV1),
    ReleaseFailure,
}

fn finalize_run_terminal<T>(
    primary: Result<T, CliError>,
    complete_owned: impl FnOnce(Result<T, CliError>) -> Result<T, CliError>,
    release: impl FnOnce() -> Result<(), AdmissionError>,
    mut journal: impl FnMut(RunTerminalJournalEvent) -> Result<(), CliError>,
) -> Result<T, CliError> {
    match finalize_owned_terminal(primary, complete_owned, release) {
        Ok(value) => Ok(value),
        Err(TerminalFailure::Primary(error)) => {
            journal(RunTerminalJournalEvent::PrimaryFailure(cli_failure_kind(
                &error,
            )))?;
            Err(error)
        }
        Err(TerminalFailure::Release(error)) => {
            journal(RunTerminalJournalEvent::ReleaseFailure)?;
            Err(CliError::Admission(error))
        }
    }
}
```

Add one production mapping closure at each run call site:

```rust
|event| match event {
    RunTerminalJournalEvent::PrimaryFailure(kind) => lifecycle.fail(kind),
    RunTerminalJournalEvent::ReleaseFailure => {
        lifecycle.transition_state(RunJournalStateV1::CleanupPending, None)
    }
}
```

- [ ] **Step 4: Route historical run through the adapter**

For resource pre-start failure and current-directory failure after admission,
call `finalize_run_terminal` with identity completion, `|| guard.release()`,
and the production journal closure. Do not rely on `AdmissionGuard::Drop` for
these normal error paths.

Use this exact shape for the resource pre-start branch:

```rust
if let Err(error) = resource_pre_start(supervisor.clone(), &cancellation) {
    return finalize_run_terminal(
        Err::<(), _>(error),
        std::convert::identity,
        || guard.release(),
        |event| match event {
            RunTerminalJournalEvent::PrimaryFailure(kind) => lifecycle.fail(kind),
            RunTerminalJournalEvent::ReleaseFailure => {
                lifecycle.transition_state(RunJournalStateV1::CleanupPending, None)
            }
        },
    );
}
```

Use this exact early-return shape if `current_dir` cannot be resolved after
admission:

```rust
let current_dir = match std::env::current_dir() {
    Ok(path) => path,
    Err(error) => {
        return finalize_run_terminal(
            Err::<(), _>(CliError::internal(error)),
            std::convert::identity,
            || guard.release(),
            |event| match event {
                RunTerminalJournalEvent::PrimaryFailure(kind) => lifecycle.fail(kind),
                RunTerminalJournalEvent::ReleaseFailure => lifecycle
                    .transition_state(RunJournalStateV1::CleanupPending, None),
            },
        );
    }
};
```

For the executed historical run, replace the explicit join/reconcile/release
block with:

```rust
let outcome = finalize_run_terminal(
    run_result.map_err(CliError::Run),
    |outcome| {
        completion_barrier.ensure_joined();
        reconcile_watchdog_outcome(outcome, &mut completion_barrier)
    },
    || guard.release(),
    |event| match event {
        RunTerminalJournalEvent::PrimaryFailure(kind) => lifecycle.fail(kind),
        RunTerminalJournalEvent::ReleaseFailure => {
            lifecycle.transition_state(RunJournalStateV1::CleanupPending, None)
        }
    },
)?;
```

Leave successful source-snapshot cleanup, finalization, receipt writing, and
sealing after this block in their existing order.

Immediately compile the historical path before editing matrix run:

```bash
rtk cargo check --offline --locked --bin commit-ci-preflight
rtk cargo test --offline --locked --bin commit-ci-preflight tests::run_terminal_
rtk cargo test --offline --locked --bin commit-ci-preflight tests::watchdog_barrier_joins_once_after_early_run_error
```

Expected: the binary compiles and the historical terminal/watchdog tests pass;
this independently validates that the early-return closures consume `guard`
only on paths that return from the enclosing function.

- [ ] **Step 5: Route matrix run through the adapter**

For matrix resource pre-start and current-directory failures after admission,
use the same identity-completion adapter path and exact early-return snippets
from Step 4. The enclosing function also returns `Result<(), CliError>`, so the
annotated `Err::<(), _>` remains the correct type. Do not leave the current
post-admission `std::env::current_dir().map_err(CliError::internal)?` path in
place.

Replace the explicit matrix join/result/release block with:

```rust
let outcome = finalize_run_terminal(
    result,
    |result| {
        completion_barrier.ensure_joined();
        if let Some(error) = completion_barrier.take_join_error() {
            Err(CliError::Resource(ResourceGuardError::Watchdog(error)))
        } else {
            result
        }
    },
    || guard.release(),
    |event| match event {
        RunTerminalJournalEvent::PrimaryFailure(kind) => lifecycle.fail(kind),
        RunTerminalJournalEvent::ReleaseFailure => {
            lifecycle.transition_state(RunJournalStateV1::CleanupPending, None)
        }
    },
)?;
```

Preserve matrix execution-barrier trip semantics and the existing finalizing
and sealed transitions.

Immediately compile the matrix path before adding the cross-family tests:

```bash
rtk cargo check --offline --locked --bin commit-ci-preflight
rtk cargo test --offline --locked --test runtime_cli
rtk cargo test --offline --locked --test matrix_contract
```

Expected: the binary compiles and the existing matrix CLI/contract tests pass.

- [ ] **Step 6: Add one cross-family regression test**

Add this test; it supplements rather than replaces the primitive's event-order
test:

```rust
#[test]
fn all_heavy_family_adapters_release_once_and_fail_closed() {
    let releases = AtomicUsize::new(0);

    let benchmark = finalize_benchmark_terminal(Ok(()), || {
        releases.fetch_add(1, Ordering::SeqCst);
        Err(AdmissionError::Clock)
    });
    assert!(matches!(
        benchmark,
        Err(CliError::Admission(AdmissionError::Clock))
    ));

    let run = finalize_run_terminal(
        Ok(()),
        std::convert::identity,
        || {
            releases.fetch_add(1, Ordering::SeqCst);
            Err(AdmissionError::Clock)
        },
        |_| Ok(()),
    );
    assert!(matches!(
        run,
        Err(CliError::Admission(AdmissionError::Clock))
    ));

    let cancellation = CancellationToken::default();
    let guard = finalize_guard_exec_result(
        Ok(completed_process_result()),
        &cancellation,
        None,
        None,
        || {
            releases.fetch_add(1, Ordering::SeqCst);
            Err(GuardExecError::Admission(AdmissionError::Clock))
        },
    );
    assert!(matches!(
        guard,
        Err(GuardExecError::Admission(AdmissionError::Clock))
    ));

    assert_eq!(releases.load(Ordering::SeqCst), 3);
}
```

Also add an opaque-success preservation test for the two adapters whose
timeout/cancellation/resource state is already encoded inside their successful
domain outcome before terminal release:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpaqueTerminalOutcome {
    TimedOut,
    UserCancelled,
    ResourcePressure,
}

#[test]
fn benchmark_and_run_preserve_opaque_terminal_outcomes() {
    for expected in [
        OpaqueTerminalOutcome::TimedOut,
        OpaqueTerminalOutcome::UserCancelled,
        OpaqueTerminalOutcome::ResourcePressure,
    ] {
        assert_eq!(
            finalize_benchmark_terminal(Ok(expected), || Ok(()))
                .expect("benchmark terminal outcome"),
            expected
        );
        assert_eq!(
            finalize_run_terminal(
                Ok(expected),
                std::convert::identity,
                || Ok(()),
                |_| Ok(()),
            )
            .expect("run terminal outcome"),
            expected
        );
    }
}
```

For `guard exec`, extend the existing finalizer test with
`GuardExecError::ChildExit(7)`, `GuardExecError::TimedOut`, and
`GuardExecError::UserCancelled` cases, each with a successful release closure,
and assert each exact exit classification survives:

```rust
for (primary, expected_exit) in [
    (GuardExecError::ChildExit(7), 7),
    (GuardExecError::TimedOut, 124),
    (GuardExecError::UserCancelled, 130),
] {
    let result = finalize_guard_exec_result(
        Err(primary),
        &CancellationToken::default(),
        None,
        None,
        || Ok(()),
    );
    assert_eq!(result.expect_err("guard primary failure").exit_code(), expected_exit);
}
```

Keep the existing resource-pressure test and
`watchdog_barrier_joins_once_after_early_run_error` as the actual resource and
watchdog classification coverage.

- [ ] **Step 7: Run focused GREEN and existing run/watchdog regressions**

Run:

```bash
rtk cargo test --offline --locked --bin commit-ci-preflight tests::run_terminal_
rtk cargo test --offline --locked --bin commit-ci-preflight tests::all_heavy_family_adapters_release_once_and_fail_closed
rtk cargo test --offline --locked --bin commit-ci-preflight tests::benchmark_and_run_preserve_opaque_terminal_outcomes
rtk cargo test --offline --locked --bin commit-ci-preflight tests::watchdog_barrier_joins_once_after_early_run_error
rtk cargo test --offline --locked --lib run::tests::
rtk cargo test --offline --locked --lib resource::tests::watchdog_
rtk cargo test --offline --locked --test runtime_cli
rtk cargo test --offline --locked --test matrix_contract
rtk cargo test --offline --locked --test recover_cli
rtk cargo fmt --check
rtk git diff --check
```

Expected: all focused and existing regression tests pass. No command starts a
real CCP run or Docker container.

- [ ] **Step 8: Commit the run adapters**

Run:

```bash
rtk git add src/main.rs
rtk git commit -m "refactor: unify run terminal resource release"
```

Expected: one local commit containing historical run, matrix run, adapter, and
their unit tests only.

---

### Task 4: Operator documentation

**Files:**
- Modify: `docs/LOCAL_RUN.md:84-117`
- Modify: `docs/COORDINATION_RUNBOOK.md:15-38,162-178`
- Modify: `docs/ARCHITECTURE.md:65-95`
- Modify: `docs/TESTING_AND_FAULT_INJECTION.md:1-110`

**Interfaces:**
- Consumes: the exact terminal ordering and failure behavior implemented in
  Tasks 1-3.
- Produces: a discoverable operator contract that cannot silently claim a slot
  release from child exit, `ps`, or an uncertain release result.

- [ ] **Step 1: Add bounded, consistent documentation**

Add the following exact claims once in the named canonical documents, with
surrounding prose that preserves existing command-family distinctions:

- `docs/LOCAL_RUN.md`: “The watchdog joins before admission release, and
  admission release is attempted exactly once. A release failure overrides
  the primary result.” Also state: “benchmark has no mid-workload watchdog.”
- `docs/COORDINATION_RUNBOOK.md`: “A child exit is not a slot-release handoff.”
  Require explicit terminal result plus fresh admission/runtime/resource status
  before handoff.
- `docs/ARCHITECTURE.md`: describe the shared private terminal primitive and
  state “release failure overrides the primary result.” Preserve cache-pin and
  source-snapshot lifecycle exceptions.
- `docs/TESTING_AND_FAULT_INJECTION.md`: state that deterministic fake closure
  tests prove ordering and precedence, while process-tree and Docker lifecycle
  tests prove their own containment boundaries. Include the exact phrase
  “process lists do not prove release.”

Do not claim that unit tests prove a real admission root, Docker cleanup, a
published receipt, or another platform.

- [ ] **Step 2: Review the semantic documentation diff**

Read the complete four-document diff and verify each claim against the exact
implemented call-site ordering from Tasks 1-3. Confirm that the prose
distinguishes facts, operator requirements, and non-claims; does not describe a
unit test as host qualification; and does not broaden CCP ownership to swap,
unrelated processes, foreign containers, or undeclared paths.

- [ ] **Step 3: Run related existing contracts**

Run:

```bash
rtk cargo test --offline --locked --test cache_pin_contract
rtk cargo test --offline --locked --test repository_hygiene_contract
rtk cargo test --offline --locked --test release_hardening_contract
rtk git diff --check
```

Expected: all three existing contract suites pass with no broken ownership or public
evidence claims.

- [ ] **Step 4: Commit documentation**

Run:

```bash
rtk git add docs/LOCAL_RUN.md docs/COORDINATION_RUNBOOK.md docs/ARCHITECTURE.md docs/TESTING_AND_FAULT_INJECTION.md
rtk git commit -m "docs: define terminal resource release evidence"
```

Expected: one local documentation commit.

---

### Task 5: Exact-head verification and review package

**Files:**
- Modify only if a verification failure proves a scoped defect in Tasks 1-4.
- Record: `/private/tmp/ccp-terminal-resource-release-verification.md`

**Interfaces:**
- Consumes: all locally committed deliverables from Tasks 1-4.
- Produces: exact-head local verification evidence and a bounded review package;
  it does not produce a CCP receipt or remote evidence.

- [ ] **Step 1: Confirm exact local scope before verification**

Run:

```bash
rtk git status --short --branch
rtk git rev-parse HEAD
rtk git log --oneline origin/main..HEAD
rtk git diff --stat origin/main...HEAD
rtk git diff --check origin/main...HEAD
```

Expected: clean branch; only the approved spec, plan, private primitive,
terminal adapters/tests, and four bounded operator documents differ from
`origin/main`.

- [ ] **Step 2: Run formatting and strict static checks**

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy --offline --locked --all-targets --all-features -- -D warnings
```

Expected: both exit 0 under the pinned Rust 1.96.0 toolchain. Preserve complete
diagnostics for any failure; do not weaken `-D warnings`.

- [ ] **Step 3: Run focused terminal, guard, run, journal, and runtime tests**

Run:

```bash
rtk cargo test --offline --locked --bin commit-ci-preflight terminal::tests::
rtk cargo test --offline --locked --bin commit-ci-preflight tests::run_terminal_
rtk cargo test --offline --locked --bin commit-ci-preflight tests::all_heavy_family_adapters_release_once_and_fail_closed
rtk cargo test --offline --locked --bin commit-ci-preflight tests::guard_cache_pin_remains_live_through_terminal_release
rtk cargo test --offline --locked --bin commit-ci-preflight tests::watchdog_barrier_joins_once_after_early_run_error
rtk cargo test --offline --locked --lib run::tests::
rtk cargo test --offline --locked --lib resource::tests::watchdog_
rtk cargo test --offline --locked --test guard_exec_cli
rtk cargo test --offline --locked --test process_supervisor
rtk cargo test --offline --locked --test recover_cli
rtk cargo test --offline --locked --test runtime_cli
rtk cargo test --offline --locked --test matrix_contract
rtk cargo test --offline --locked --test cache_pin_contract
```

Expected: all named deterministic suites pass. `guard_exec_cli` compiles and
exits 0, but its native opt-in test remains ignored (0 passed, 1 ignored) and
is NOT_RUN, not PASS; this does not block deterministic task closure.
`process_supervisor` may start its existing local fixture processes, but no
test may acquire admission or invoke CCP/Docker.

- [ ] **Step 4: Run the complete native suite and doctests**

Run:

```bash
rtk cargo test --offline --locked --all-targets --all-features
rtk cargo test --offline --locked --doc
```

Expected: terminal PASS for the complete native suite and doctests. Record the
exact passed/ignored/failed counts; do not infer them from an interrupted or
truncated command.

- [ ] **Step 5: Write the local verification report**

Create `/private/tmp/ccp-terminal-resource-release-verification.md` containing:

- repository and worktree absolute path;
- branch, exact HEAD, exact `origin/main`, and clean/dirty state;
- ordered local commits;
- each exact command, exit code, and terminal test count;
- the distinction between deterministic fake terminal evidence and existing
  process/runtime integration evidence;
- confirmation that no CCP run, Docker workload, network action, push, PR,
  evidence publication, or merge occurred;
- any remaining unknowns or limitations.

- [ ] **Step 6: Request exact-head code review and fix only proven findings**

Give the reviewer the design, plan, `origin/main...HEAD` diff, verification
report, and exact head. Require findings by severity with file/line anchors.
For a proven finding, return to the smallest affected RED test, implement the
minimum fix, rerun the affected focused suite and full required gates, then
commit a scoped fix. Do not make speculative cleanup changes.

- [ ] **Step 7: Freeze the local handoff state**

Run:

```bash
rtk git status --short --branch
rtk git rev-parse HEAD
rtk shasum -a 256 /private/tmp/ccp-terminal-resource-release-verification.md
```

Expected: clean local branch and a hash-bound verification report. Stop before
push, PR, CCP, evidence publication, merge, or branch cleanup.
