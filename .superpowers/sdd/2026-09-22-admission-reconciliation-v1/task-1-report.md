# Task 1 report

## RED evidence

Added `reconciliation_preview_is_read_only_and_classifies_an_absent_lease` in `src/admission.rs`. Initial focused command failed at compile time because `AdmissionReconciliationModeV1`, `AdmissionReconciliationCandidateV1`, and `reconcile_preview_with_timeout` were absent. GitNexus attempt was blocked: `.gitnexus/run.cjs` is absent in this worktree.

## Implementation

Added preview report/mode/candidate types and `AdmissionCoordinator::reconcile_preview_with_timeout`. Preview returns empty for an absent root without initialization; validates owned layout; uses existing queue/slot locking and strict ticket/lease readers; classifies eligible absent leases and blocked ticket/slot/live-lease cases; releases locks; does not call reclamation or mutation helpers.

## Tests/output

`rtk env CARGO_TARGET_DIR=/tmp/admission-reconcile-target cargo test --locked reconciliation_preview -- --nocapture`: PASS (1 test).

`rtk env CARGO_TARGET_DIR=/tmp/admission-reconcile-target cargo test --locked`: FAIL, 185 passed, 101 failed, 1 ignored. Failures are pre-existing sandbox permission errors in cache, cache_payload, matrix, run, and workspace tests (`Operation not permitted` while creating cache mount targets).

`cargo fmt -- src/admission.rs`: blocked by worktree filesystem permission (`Operation not permitted`); no formatting changes applied.

## Commit

Pending commit after parent review.

## Concerns

Task 1 scope only covers preview API and one focused fixture. Full reconciliation apply behavior remains for later tasks. Full-suite failures are environmental and unrelated to admission preview; no claim of full-suite pass.
