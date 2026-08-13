# ADR 0002: Physically independent verifier workspace split

## Status

Proposed implementation plan for PR 2 only. This document is an architecture
study, not an authorization to implement signing, publication, or distribution
architecture.

## Context and decision boundary

The repository is currently a single Rust package, `commit-ci-preflight`, at
the reviewed base `86b32bbef1ca3f75361d139fa27163e6f3427b24`. Its library
exports every production module from [`src/lib.rs`](../../src/lib.rs), and its
binary in [`src/main.rs`](../../src/main.rs) dispatches runner, resource,
cache, migration, benchmark, and verification commands from one CLI.

The product roadmap defines PR 2 as the introduction of `ccp-core` and
`ccp-verifier`, preservation of receipt/policy/report/schema/canonical-byte
behavior, a verifier-only binary, and a dependency audit proving that runner
and host-execution code is absent from the verifier graph. This plan narrows
that objective to the smallest behavior-preserving workspace migration. It
does not choose signing, identity, packaging, installer, release, or
distribution architecture.

## Source evidence at the reviewed base

### Current public contract

- [`src/receipt.rs`](../../src/receipt.rs) defines `ReceiptEnvelopeV1`,
  `ReceiptV1`, `ProducerEvidence`, `RepositoryEvidence`, `RunEvidence`,
  `PlatformEvidence`, `CheckEvidence`, `EvidenceStatus`, and `ReceiptError`.
- `ReceiptEnvelopeV1::seal`, `ReceiptEnvelopeV1::verify`, and
  `ReceiptEnvelopeV1::canonical_bytes` are the construction, integrity, and
  canonical-output seam. `ReceiptV1::validate` and `CheckEvidence::validate`
  enforce semantic invariants. `canonical_json`, `receipt_schema_json`, and
  `canonical_digest` define the byte and schema contracts.
- [`src/verify.rs`](../../src/verify.rs) defines `VerificationPolicyV1`,
  `AcceptedPlatformV1`, `VerificationStatus`, `VerificationDecision`,
  `VerificationFindingV1`, and `VerificationReportV1`. The public execution
  seam is `verify_receipt_document`; `receipt_input_failure_report`,
  `VerificationReportV1::canonical_bytes`, and
  `VerificationReportV1::exit_code` are also externally observable.
- `verify_receipt_document` validates the policy and caller-supplied commit and
  evaluation time, bounds receipt bytes, parses strict JSON, calls
  `ReceiptEnvelopeV1::verify`, evaluates policy, and derives the final report.
  It does not import `run`, `runtime`, `process`, `cache`, `admission`,
  `resource`, or `workspace`.
- [`src/main.rs`](../../src/main.rs) currently exposes `Command::Verify` with
  `--receipt`, `--policy`, `--expected-commit`, optional
  `--evaluated-at-utc`, and `--json`. `print_verify` loads the policy, reads
  the receipt, calls `verify_receipt_document` or
  `receipt_input_failure_report`, emits canonical JSON or human output, and
  maps PASS to 0 and a verification outcome to 3. CLI/usage errors remain 2;
  internal serialization remains 70 through `CliError`.

### Tests and golden contracts

- [`tests/receipt_contract.rs`](../../tests/receipt_contract.rs) proves that
  `tests/fixtures/receipt-v1-pass.json` round-trips byte-for-byte and that
  `receipt_schema_json()` equals `schema/receipt-v1.schema.json`.
- [`tests/verification_contract.rs`](../../tests/verification_contract.rs)
  proves deterministic replay, every-leaf tamper rejection, each covered
  policy mismatch, strict malformed/unknown/unsupported/oversized input,
  policy parser strictness, schema equality, and source-boundary expectations.
- [`tests/verify_cli.rs`](../../tests/verify_cli.rs) proves the command's JSON
  report, stable 0/3 outcomes, missing-receipt behavior, and usage handling.
- Inline tests in `receipt.rs`, `verify.rs`, and `main.rs` cover canonical
  digesting, semantic rejection, UTC conversion, CLI definition, exit-code
  separation, and command argument validation.
- [`tests/github_gate_contract.rs`](../../tests/github_gate_contract.rs) and
  [`tests/benchmark_contract.rs`](../../tests/benchmark_contract.rs) are
  downstream consumers of the existing `verify`/receipt API and must remain
  source-compatible through the first migration.

## Decision: smallest workspace split

Introduce a Cargo workspace while retaining the current root package as a
compatibility/runner package for this PR. Add these two workspace members:

```text
crates/
  ccp-core/       receipt domain, canonical JSON, digest, schema, common policy-domain types
  ccp-verifier/   bounded input parsing, policy evaluation, reports, verifier-only binary
```

The root package remains the existing `commit-ci-preflight` package and
continues to own the current all-purpose `commit-ci-preflight` binary during
this tranche. This avoids changing the existing runner CLI while creating a
separately compilable verifier target. Later runner/CLI decomposition is not
part of PR 2.

### `ccp-core` contents

Move the receipt contract as a unit, preserving names, derives, serde field
names, schema output, and error behavior:

- the public receipt types and `EvidenceStatus` from `src/receipt.rs`;
- `ReceiptEnvelopeV1::{seal, verify, canonical_bytes}`;
- `ReceiptV1::validate` and `CheckEvidence::validate`;
- `canonical_json`, `receipt_schema_json`, `canonical_digest`, and the
  receipt-only validation helpers;
- the receipt schema version and receipt ID prefix constants.

If policy input types are moved into core for the roadmap's common-domain
boundary, move only the data contracts (`VerificationPolicyV1` and
`AcceptedPlatformV1`) and their serde/schema definitions. Keep file I/O,
strict TOML parsing, policy validation, report construction, and policy
evaluation in `ccp-verifier`; this keeps core independent of the verifier's
input transport. The implementation should choose one ownership location and
re-export it, rather than defining duplicate structs.

The first migration may retain the root package's old module paths as
compatibility re-exports:

```rust
pub use ccp_core::receipt;
// Existing root paths remain valid for downstream users and current tests.
pub use ccp_core::receipt::{ReceiptEnvelopeV1, ReceiptV1, EvidenceStatus};
```

The exact re-export form must preserve both `commit_ci_preflight::receipt::*`
and any root-level paths that currently exist. No fixture or schema byte should
need a compatibility shim.

### `ccp-verifier` contents

Move the verifier implementation as a unit, changing only crate-qualified
imports from `crate::receipt` to `ccp_core::receipt` (or the selected core
re-export):

- `VerificationPolicyV1::{load, parse, validate}`;
- `VerificationStatus`, `VerificationDecision`,
  `VerificationFindingV1`, and `VerificationReportV1`;
- `VerificationReportV1::{canonical_bytes, exit_code}`;
- `verify_receipt_document` and `receipt_input_failure_report`;
- `verification_policy_schema_json` and
  `verification_report_schema_json`;
- the verifier error types and bounded validation/time helpers used only by
  this surface.

Add a dedicated `ccp-verifier` binary with only these user-facing surfaces:

- `verify` with the existing receipt, policy, expected-commit,
  evaluated-at, and JSON options;
- schema output for policy/report/receipt contracts, with explicit stable
  output names and no runner command dispatch.

The verifier binary must read only caller-provided policy/receipt inputs,
never execute project checks, invoke Docker, inspect the runner cache, acquire
admission, start a process supervisor, or run a resource watchdog. Its
human-readable and JSON output should be copied from the existing
`print_verify` behavior only where that is necessary for compatibility; the
existing root binary remains the compatibility surface for all other commands.

## Dependency graph constraints

The graph is a hard acceptance contract, not a feature flag or a source-code
convention.

### Allowed graph

```text
ccp-verifier binary
  -> ccp-verifier
       -> ccp-core
       -> serde / serde_json / schemars / toml / std
       -> clap (binary argument parsing only)

ccp-core
  -> serde / serde_json / schemars / sha2 / std
```

The exact dependency list must be confirmed against the moved code. `toml` is
allowed only if policy parsing remains in `ccp-verifier`; `clap` is not a core
dependency. `fs` and `std::path` use are acceptable for bounded caller input,
but they must not pull runner services into the crate.

### Forbidden verifier edges

The resolved normal dependency graph for the dedicated binary must contain no
dependency on the root runner package and no modules or crates providing
`admission`, `benchmark`, `cache`, `config` execution, `github_actions`,
`process`, `resource`, `run`, `runtime`, or `workspace`. It must also contain
no `ctrlc`, `fs2`, `process-wrap`, or `nix` dependency. A textual absence check
is useful evidence, but `cargo tree -e normal -p ccp-verifier` and a clean
standalone build are the authoritative checks.

The root compatibility package may still depend on all existing runner
dependencies. That does not satisfy the gate by itself: the dedicated binary's
resolved graph is the scoped object under review.

## Compatibility and equivalence test plan

Keep the existing fixtures and tests in place first. Add tests in the smallest
new verifier package and use the same `include_bytes!`/`include_str!` golden
inputs, not regenerated copies.

1. **Receipt golden parity.** Parse and verify
   `tests/fixtures/receipt-v1-pass.json` with `ccp-core`; assert
   `canonical_bytes()` is byte-identical to the fixture, the generated receipt
   schema equals the pinned schema, and the receipt ID is unchanged.
2. **Verifier report parity.** For every positive and negative case in
   `verification_contract.rs` and `verify_cli.rs`, run the legacy root command
   and the dedicated verifier with the same receipt, policy, commit, and
   evaluation instant. Compare stdout bytes in JSON mode and process exit
   codes. Compare human output only after deciding that its current wording is
   intentionally part of the compatibility contract.
3. **Malformed and maximum-input parity.** Preserve the 1 MiB receipt bound
   (`MAX_RECEIPT_BYTES`), strict unknown-field rejection, unsupported-schema
   rejection, invalid JSON behavior, missing-file report, invalid commit usage
   behavior, and deterministic failure report ordering. Add exact boundary
   cases at maximum size and maximum-plus-one.
4. **Policy and report schema parity.** Assert the generated policy and report
   schema bytes remain equal to their pinned `schema/*.schema.json` files.
5. **Mutation matrix.** Retain the existing every-receipt-leaf mutation test
   and policy-dimension matrix. Add a subprocess matrix so the dedicated
   binary's 0/3/2 classifications match the legacy path.
6. **Dependency receipt.** Record the exact `cargo tree -e normal` output,
   target, compiler/toolchain, binary byte size, and SHA-256 in a reviewable
   local receipt or test artifact. This is evidence of the PR-2 candidate only,
   not a signing or release claim.

The implementation must not weaken tests by replacing byte equality with parsed
JSON equality. Parsed comparisons may be additional diagnostics, never a
replacement for canonical-byte assertions.

## Migration order

Each step should leave the workspace buildable and independently revertible.

1. Add workspace metadata and the two package skeletons without changing
   existing source behavior; verify the current root package and tests.
2. Move/copy the receipt implementation into `ccp-core` with one authoritative
   definition. Adapt the root library to re-export it and update internal
   imports without changing public paths, schemas, fixtures, or bytes.
3. Move/copy the verifier implementation into `ccp-verifier`, make its receipt
   dependency explicit, and expose compatibility re-exports from the root
   package. Remove duplicate definitions only after the parity tests pass.
4. Add the verifier-only binary and its minimal CLI/schema surfaces. Keep the
   existing root `Command` enum and `print_verify` path operational.
5. Add subprocess equivalence, boundary, malformed-input, dependency-tree,
   and binary-size evidence tests. Run the bounded PR-2 gate.
6. Only after all gates are green, update documentation references that need
   to name the new crates. README, workflows, schemas, and GitHub state are
   outside this architecture-study tranche and must not be altered here.

## Rollback

Rollback is a source-level revert of the PR-2 migration commits in reverse
order. First remove the verifier-only package/binary and its new tests; then
restore the root verifier implementation/re-exports; then restore the root
receipt module. Do not regenerate or rewrite fixtures, schemas, lockfiles, or
receipts during rollback. If a workspace metadata change prevents Cargo from
loading the old package, retain the smallest valid workspace declaration while
restoring the pre-split root package, or revert the workspace commit as a
separate atomic step. The existing root `commit-ci-preflight verify` command is
the operational fallback throughout the migration.

## Risks and mitigations

| Risk | Mitigation and exit condition |
|---|---|
| Duplicate receipt or policy types drift | One authoritative definition per contract; root paths re-export; compile current integration tests and compare schema/fixture bytes. |
| Cargo package graph accidentally includes runner code | Inspect `cargo tree -e normal -p ccp-verifier`; reject any forbidden crate/module; build the dedicated package independently. |
| Serde/schema output changes from crate relocation | Preserve derives, field attributes, type names, and schema settings; require byte equality against all pinned schemas and the pass fixture. |
| CLI reports differ at error boundaries | Subprocess-test missing, malformed, oversized, policy-fail, invalid-usage, and pass cases; compare exact JSON and exit codes. |
| Root compatibility re-export creates a dependency cycle | Core must not depend on root; verifier must depend only on core; root may depend on both. Cargo metadata must show an acyclic inward graph. |
| `include_str!`/fixture paths break after relocation | Keep test fixtures at repository-stable paths or use package-local includes with explicit parity tests; do not duplicate golden content silently. |
| Binary-size evidence is mistaken for distribution qualification | Record size/hash only as a PR-2 bounded receipt; leave signing, publication, installer, and release architecture undecided. |
| Local or remote consumers rely on undocumented root symbols | Search current tests and source for imports; preserve all observed public module paths in the first split and document any genuinely unobserved path as a follow-up. |

## Validation required for this study artifact

This plan is complete when the document's relative links resolve, the working
tree contains no Rust, manifest, lockfile, schema, workflow, README, or
GitHub-state changes, and `git diff --check` passes. Implementation validation
listed above belongs to PR 2 execution and is intentionally not claimed by
this document.
