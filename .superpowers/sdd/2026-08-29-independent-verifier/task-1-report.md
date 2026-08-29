# Task 1 report: Freeze the M2 compatibility envelope

## Status

PASS. The pre-extraction root API and fixture bytes are frozen in commit
`9831208` (`test: freeze verifier compatibility envelope`).

## Implementation

- Added `tests/fixtures/m2-compatibility-envelope-v1.json` with the required
  source head and SHA-256 manifest.
- Added `tests/independent_verifier_contract.rs`, which parses the manifest,
  rejects unsafe/duplicate paths, and verifies every listed byte hash.
- Added `tests/public_api_compat_contract.rs`, compile-checking root receipt,
  normalized-plan, policy, report, and Matrix imports and representative public
  error Display/source/Send+Sync behavior.
- Extended receipt and verification contract tests with Send+Sync assertions.

## TDD / characterization evidence

This is a pure pre-extraction characterization slice: the required behavior
already exists in the root package, so a RED test would not be meaningful.
The new tests encode the observed baseline and passed before any crate
extraction or production-code change.

## Tests

Command (using an isolated target directory because the linked worktree target
path was sandbox-denied):

`rtk env CARGO_TARGET_DIR=/private/tmp/ccp-independent-target cargo test --locked --test independent_verifier_contract --test public_api_compat_contract --test receipt_contract --test verification_contract`

Result: PASS; 1 + 1 + 10 + 20 passed, 1 existing historical test ignored, 0
failed. `git diff --check` also passed before commit.

## Files changed

- `tests/fixtures/m2-compatibility-envelope-v1.json`
- `tests/independent_verifier_contract.rs`
- `tests/public_api_compat_contract.rs`
- `tests/receipt_contract.rs`
- `tests/verification_contract.rs`

## Self-review and concerns

The manifest test hashes only the explicitly listed repository-relative files;
negative fixtures remain behavior-pinned by existing tests as required. No
production code, dependency, schema, or unrelated test was changed. The
focused command intentionally did not run the ignored historical verifier
test because its external retained binary was not available.
