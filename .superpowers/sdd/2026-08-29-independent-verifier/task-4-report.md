# Task 4B1 report: pure Matrix plan core

## Slice A implementation attempt (2026-08-29)

- Scope: replaced the duplicate root Matrix config/plan block in `src/matrix.rs` with core re-exports and the compatibility `build_matrix_plan` wrapper; retained execution constants and removed now-unused imports.
- Initial focused tests compiled the library but failed at root integration: core plan accessors now return `ccp_core::matrix::MatrixContractError`, while existing tests and CLI call sites still match/pass `MatrixError`; `prepare_source_snapshot_overlay` is also no longer an inherent method on the re-exported core envelope.
- Exact command: `rtk env CARGO_TARGET_DIR=/private/tmp/ccp-independent-task4-slice-a cargo test --locked --test independent_verifier_contract --test matrix_contract --test receipt_contract`
- Result: FAIL, 18 compile errors (8 test nominal error-type mismatches; 10 binary call-site/method integration errors). No heavy commands run.
- `rtk cargo fmt --all` completed after escalation; `rtk git diff --check` passed. No commit created because focused tests are not green.
- Blocker: root execution/CLI adaptation and/or test contract migration is required before this slice can be green; scope is beyond the mechanical re-export-only edit as currently staged.

### Fix round 1 (2026-08-29)

- Added core `LegacyPlanNotRepresentable` error and root adapter mapping; restored root free-function source-overlay preparation and updated CLI/internal callers; updated pure-method assertions to core error variants.
- Focused command reached execution: 11 passed, 5 failed. Failures are legacy profile digest/validation parity (4 tests) plus pinned schema naming (fixed by restoring `EnvironmentConfig` schemars rename; rerun still required).
- Exact command: `rtk cargo fmt --all && rtk env CARGO_TARGET_DIR=/private/tmp/ccp-independent-task4-slice-a cargo test --locked --test independent_verifier_contract --test matrix_contract --test receipt_contract && rtk git diff --check`
- No commit created; required GREEN gates remain outstanding.

### Packaging correction (2026-08-29)

- The previously omitted eight-type Matrix identity test was verified independently: 1 passed; `git diff --check` passed. It is packaged in the follow-up commit.
Implemented the first bounded extraction slice in `ccp-core::matrix`.

## Scope

- Added Matrix V2 configuration, runtime/check declarations, plan envelope,
  normalization, canonical digest binding, and validation.
- Added `MatrixContractError` with source propagation for I/O, TOML, config,
  and receipt failures.
- Kept execution, runtime, cache, process, Docker, source-snapshot, receipt
  publication, and root `src/matrix.rs` untouched.
- Exposed the module through `ccp_core::matrix`.

## Verification

- `rtk env CARGO_TARGET_DIR=/private/tmp/ccp-independent-target cargo check -p ccp-core --locked`: PASS.
- `rtk env CARGO_TARGET_DIR=/private/tmp/ccp-independent-target cargo test -p ccp-core --locked`: blocked by a pre-existing unrelated test compile error in `crates/ccp-core/src/receipt.rs` (`Value` is not imported at line 986).
- `rtk git diff --check`: PASS.
- `cargo fmt --all`: blocked by the environment returning `Operation not permitted` when writing this worktree; source was kept formatted manually.

## Commit

Local commit: `refactor: add pure matrix plan core`.

# Task 4B2 report: pure Matrix receipt/policy core

## Task 4B1 parity completion (2026-08-29)

RED fixture (exact pre-B1 HEAD `7bcaeb12d39cf36c5880f7063d9b90df7d88f4f7`): corrected parity used a valid resealed runtime-image mismatch and failed because old core omitted `policy.runtime_image`; invalid-policy and adapter tests also failed as intended. Active GREEN: all three new tests passed.

Final gate counts: matrix 19/19, verification 21 passed plus 1 explicit ignored, receipt 11/11, independent 7/7, core 46/46, fmt check PASS, diff check PASS.

- Core Matrix policy validation now enforces project/digest/image/freshness/platform constraints and complete runtime coverage, matching the root policy contract.
- Core receipt verification now preserves root finding order and semantics for repository, commit, dirty state, configuration, runtime set/configuration/image/platform, required checks, and freshness.
- The root adapter preserves the nominal core `VerificationError` directly (`Core::Verification(error) => Self::Verification(error)`).
- `rtk env CARGO_TARGET_DIR=/private/tmp/ccp-independent-task4-b1 cargo test -p ccp-core --locked`: PASS (46 passed, 0 failed).
- `rtk env CARGO_TARGET_DIR=/private/tmp/ccp-independent-task4-b1 cargo test --locked --test matrix_contract --test receipt_contract --test independent_verifier_contract`: PASS (34 passed, 0 failed; existing dead-code warnings only).
- `rtk git diff --check`: PASS. `rtk cargo fmt --all` remains blocked by worktree `Operation not permitted` while writing `crates/ccp-core/src/matrix.rs`; no formatter changes were applied.

Implemented the bounded receipt, policy, verification, historical digest, and
schema slice in `ccp-core`, retaining root compatibility definitions for B3.

## Task 4 B2 final integration RED (2026-08-29)

- Added `matrix_receipt_policy_and_required_check_paths_are_nominally_identical`
  to `tests/independent_verifier_contract.rs`, covering the six receipt/policy
  nominal identities and source-boundary assertions.
- Exact command: `rtk env CARGO_TARGET_DIR=/private/tmp/ccp-independent-task4-b2 cargo test --locked --test independent_verifier_contract matrix_receipt_policy_and_required_check_paths_are_nominally_identical`
- Result: RED as required. Compilation reported six nominal type mismatches;
  root duplicate definitions were identified at `src/matrix.rs` lines 288,
  295, 309, 401, 412, and 419. No production migration was completed in this
  slice yet.
- GitNexus attempt was inconclusive: checkout is not indexed and the CLI
  reported multiple unrelated indexed repositories. Bounded `rg`/line-number
  inspection was used instead.

## Scope

- Added `MatrixReceiptEnvelopeV2`, `MatrixReceiptV2`, and
  `MatrixRuntimeReceiptV2` with canonical seal/verify and semantic checks.
- Added `MatrixVerificationPolicyV2` and strict parsing/validation plus the
  pure `verify_matrix_receipt_document` path.
- Added `MatrixContractError` variants for JSON, receipt-ID mismatch,
  verification, and invalid evaluation time.
- Added `ccp_core::matrix_legacy` historical digest projection and
  `ccp_core::schema` schema entry points.
- Did not remove root Matrix definitions or alter execution integration.

## Verification

- `rtk env CARGO_TARGET_DIR=/private/tmp/ccp-independent-target cargo test -p ccp-core --locked`: PASS (46 passed, 0 failed).
- `rtk git diff --check`: PASS.
- `rtk cargo fmt --all`: blocked by `Operation not permitted` writing `src/lib.rs`; source was manually kept formatter-compatible.

## Commit

Local commit: `refactor: add pure matrix receipt policy core`.
## B2 cleanup and focused verification

- Removed the obsolete root legacy module and updated the source-boundary assertion to the core legacy implementation.
- Removed root-only imports and constants left unused after receipt/policy contract migration; test-only imports remain scoped to the unit-test module.
- Preserved the root schema compatibility bytes and mapped core receipt errors through the existing root adapter.
- Focused verification: identity 8/8, matrix contract 19/19, receipt contract 11/11, verification contract 21 passed/1 ignored, root matrix unit tests 5/5, ccp-core tests 46/46.
- cargo fmt all check and git diff check: PASS.

## B2 final integration and independent controller verification

- Replaced the six duplicate root receipt/policy definitions with nominal
  `ccp_core::matrix` re-exports. Root execution and `MatrixError` remain
  root-owned.
- Retained the public root schema and verification free functions as thin
  compatibility wrappers; core `MatrixContractError` is converted through the
  exhaustive root adapter. The policy-document dispatcher performs the same
  explicit conversion while preserving its public error variant.
- Removed the duplicated root policy evaluator and the dead root legacy module;
  the independent-verifier test proves all six cross-crate identities and the
  absence of root look-alike definitions.
- Fresh controller command:
  `rtk cargo test --locked --test independent_verifier_contract --test matrix_contract --test receipt_contract --test verification_contract && rtk cargo test --locked --lib matrix && rtk cargo test --locked -p ccp-core && rtk cargo fmt --all -- --check && rtk git diff --check`.
- Result: 59 passed plus 1 explicitly ignored across the four integration
  suites; 5/5 root Matrix unit tests; 46/46 core tests; formatting and diff
  checks PASS.
