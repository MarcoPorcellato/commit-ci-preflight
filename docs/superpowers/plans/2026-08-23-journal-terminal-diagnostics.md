# Journal Terminal Diagnostics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve an actionable, redacted diagnostic for future top-level CCP failures that otherwise end in a terminal `unknown` journal record.

**Architecture:** Keep `RunJournalEntryV1` and all v1 markers unchanged. A strict owned `terminal-diagnostic-v1.json` sidecar is written before a failed transition only for unknown failures, then exposed as an optional safe value in read-only recovery status. The top-level classifier supplies only closed enum codes, never a raw error value.

**Tech Stack:** Rust, serde, schemars, existing durable filesystem and run-journal tests, Cargo integration tests.

**Spec:** `docs/superpowers/specs/2026-08-23-journal-terminal-diagnostics-design.md`

## Global Constraints

- Do not alter `RunJournalEntryV1`, `RUN_JOURNAL_SCHEMA_VERSION`, ownership-marker semantics, or source-binding token derivation.
- Persist only bounded enums: no error `Display`/`Debug`, path, environment, command, check output, or secret.
- Historic journals without a diagnostic sidecar remain readable and terminal failures remain non-actionable.
- A malformed or unknown sidecar is fail-closed and must surface as operator-required / a recovery error, not as a trusted diagnosis.
- Tests must be synthetic; do not invoke CCP `run`, Docker, network, R5, receipt publication, or GitHub mutation.

---

### Task 1: Strict terminal-diagnostic sidecar and recovery projection

**Files:**
- Modify: `src/run_journal.rs:22-147, 267-285, 342-494, 671-685, 780-1055`
- Test: `src/run_journal.rs:780-1055`
- Test: `tests/recover_cli.rs:104-169`

**Interfaces:**
- Consumes: existing `RunJournalStore`, `RunFailureKindV1`, `RunJournalStateV1`, `RecoveryRunStatusV1`.
- Produces: `RunFailureDiagnosticCodeV1`, `RunFailureDiagnosticV1`, `RunJournalStore::fail`, and optional `RecoveryRunStatusV1::failure_diagnostic` for Task 2.

- [ ] **Step 1: Write the failing unit tests for a safe terminal diagnostic**

Add tests that construct a created journal, reach `executing`, and expect an absent `RunJournalStore::fail` API to write a `failed/unknown` record carrying `internal_command_failure` in `status()`. Add a legacy journal test that still writes `transition(... Failed, Some(Unknown))` with no sidecar and remains terminal. Add a tampered sidecar test that expects `status()` to classify the run as `operator_required`.

```rust
store.transition(RUN_ID, RunJournalStateV1::Executing, AT, None)?;
store.fail(
    RUN_ID,
    AT,
    RunFailureKindV1::Unknown,
    Some(RunFailureDiagnosticCodeV1::InternalCommandFailure),
)?;
let status = store.status()?;
assert_eq!(status.runs[0].failure_diagnostic.as_ref().unwrap().diagnostic_code,
    RunFailureDiagnosticCodeV1::InternalCommandFailure);
```

- [ ] **Step 2: Run the focused test target and observe RED**

Run: `cargo test run_journal::tests::terminal_unknown_diagnostic_is_projected --lib`

Expected: compilation failure because `RunJournalStore::fail`, `RunFailureDiagnosticCodeV1`, and `failure_diagnostic` do not exist yet.

- [ ] **Step 3: Implement the smallest strict sidecar contract**

In `src/run_journal.rs`, add the private filename constant `terminal-diagnostic-v1.json`; public serde/schemars `RunFailureDiagnosticCodeV1` and `RunFailureDiagnosticV1`; and optional `failure_diagnostic` on `RecoveryRunStatusV1`. Implement `RunJournalStore::fail(run_id, at_utc, failure_kind, diagnostic_code)` so a code is allowed only with `Unknown`, serializes a strict sidecar before delegating to `transition(... Failed, ...)`, and refuses duplicate or invalid writes. Filter only this exact filename from journal entries.

Implement a strict sidecar reader that checks regular-file type, small size, exact schema version, run ID, `failure_kind == Unknown`, and the closed enum. `status()` and terminal `apply()` must read it when it is present; malformed data must not be trusted. Old failed entries with no file remain valid.

- [ ] **Step 4: Run focused unit tests and observe GREEN**

Run: `cargo test run_journal::tests --lib`

Expected: journal transition, legacy compatibility, sidecar projection, redaction, and tamper tests pass.

- [ ] **Step 5: Add recovery CLI coverage and verify it**

In `tests/recover_cli.rs`, create a terminal unknown failure through `store.fail` and assert `recover status --json` exposes `failure_diagnostic.diagnostic_code`; retain the existing assertion that `recover apply` exits `5` for a terminal run. Run: `cargo test --test recover_cli`.

- [ ] **Step 6: Commit the independently testable store change**

```bash
git add src/run_journal.rs tests/recover_cli.rs
git commit -m "feat: persist bounded terminal diagnostics"
```

### Task 2: Top-level classifier and synthetic `executing` regression

**Files:**
- Modify: `src/main.rs:1392-1454, 1995-2707`
- Test: `src/main.rs` unit-test module

**Interfaces:**
- Consumes: `RunFailureDiagnosticCodeV1` and `RunJournalStore::fail` from Task 1.
- Produces: `cli_failure_diagnostic` and a `JournalLifecycleObserver::fail` path that stores a safe diagnostic before the terminal transition.

- [ ] **Step 1: Write the failing synthetic top-level failure test**

In the `src/main.rs` test module, create an isolated journal, move it through `created → admitted → prepared → executing`, construct `CliError::Internal(std::io::Error::other("synthetic-top-level-secret"))`, and call the new lifecycle/classifier path. Assert the final entry has `Unknown`, status exposes `internal_command_failure`, its serialized JSON excludes `synthetic-top-level-secret`, and no receipt is created by the unit test.

```rust
let error = CliError::internal(std::io::Error::other("synthetic-top-level-secret"));
lifecycle.fail(cli_failure_kind(&error), cli_failure_diagnostic(&error))?;
assert_eq!(diagnostic.diagnostic_code,
    RunFailureDiagnosticCodeV1::InternalCommandFailure);
assert!(!serialized.contains("synthetic-top-level-secret"));
```

- [ ] **Step 2: Run the single test and observe RED**

Run: `cargo test synthetic_internal_failure_after_executing_is_redacted --bin commit-ci-preflight`

Expected: compilation failure because the lifecycle `fail` signature and `cli_failure_diagnostic` do not exist.

- [ ] **Step 3: Implement the minimal classifier wiring**

Extend `JournalLifecycleObserver::fail` to accept an optional diagnostic code and call `RunJournalStore::fail`. Add `cli_failure_diagnostic(&CliError) -> Option<RunFailureDiagnosticCodeV1>`: map `CliError::Internal(_)` to `InternalCommandFailure`; map every remaining unmatched branch that has coarse `Unknown` to `UnclassifiedTopLevel`; return `None` for known coarse kinds. Update only existing `lifecycle.fail(cli_failure_kind(&error))` call sites to pass the companion result.

- [ ] **Step 4: Run focused binary tests and observe GREEN**

Run: `cargo test --bin commit-ci-preflight`

Expected: the new synthetic regression and existing main unit tests pass without invoking a real target run.

- [ ] **Step 5: Commit the classifier integration**

```bash
git add src/main.rs
git commit -m "fix: classify unknown terminal failures"
```

### Task 3: Full regression and contract review

**Files:**
- Modify only if verification identifies a concrete defect: `src/run_journal.rs`, `src/main.rs`, `tests/recover_cli.rs`

**Interfaces:**
- Consumes: completed Task 1 and Task 2.
- Produces: verified implementation evidence; no receipt, plan, or external state.

- [ ] **Step 1: Run formatting and targeted complete regression**

Run: `cargo fmt --check && cargo test --lib && cargo test --test recover_cli && cargo test --bin commit-ci-preflight`

Expected: all selected tests pass and formatting reports no diff.

- [ ] **Step 2: Run the full repository suite**

Run: `cargo test`

Expected: exit code `0`; record the exact test count/output and any environment-bound skips.

- [ ] **Step 3: Inspect the final diff for safety boundaries**

Run: `git diff origin/main...HEAD -- src/run_journal.rs src/main.rs tests/recover_cli.rs docs/superpowers`

Verify: no raw error serialization; only exact sidecar is exempted from journal-entry discovery; historic no-sidecar records remain readable; failed runs remain terminal; no command starts a target run.

- [ ] **Step 4: Commit only an evidence-driven correction, if one was necessary**

If and only if a verification defect required a source correction, re-run the exact relevant RED/GREEN test and commit that correction separately. Otherwise make no empty commit.
