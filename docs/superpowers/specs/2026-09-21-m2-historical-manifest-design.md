---
type: evidence-contract-amendment
title: "M2 Historical Manifest Verification"
status: approved-direction
last_verified: 2026-09-21
---

# M2 Historical Manifest Verification

## Purpose

Keep the M2 capability-pack closure evidence immutable while allowing later,
unrelated user-visible changes to `CHANGELOG.md`.

The current M2 test reads every declared path from the checkout under test.
Because `CHANGELOG.md` is one of those paths, any later release note makes an
otherwise unrelated change fail the historical-evidence test. Updating the
stored digest would rewrite the M2 evidence and is forbidden.

## Decision

`m2-manifest.json` remains schema `1.0`, byte-for-byte unchanged, and stays
bound to commit `2e6286cc23584d5e82842aacf106c3bb5e7462df`.

The preserved legacy record is now known invalid: after acquiring its declared
commit object, three entries disagree with that commit's raw blobs. Its
existence and SHA-256 remain historical evidence of the faulty record, not a
claim that the record verified. A new sibling
`m2-manifest-v1.1.json` retains schema `1.0`, the same base commit, fixed path
list, and corrected byte/SHA-256 values. The filename version identifies the
corrected evidence record without mutating or relabelling v1.0. Only v1.1 is
the positive closure contract; v1.0 is exercised as a preserved fail-closed
negative record.

The M2 contract test will verify each declared path against the Git object at
that recorded commit, not against the current checkout. It will use the
existing CCP test process supervisor with a two-second wall-clock deadline,
64 KiB stdout/stderr ceiling, deterministic process seams, and cleanup
verification. The subprocess receives only an allowlisted Git environment:
`GIT_CONFIG_NOSYSTEM=1`, `GIT_NO_REPLACE_OBJECTS=1`,
`GIT_NO_LAZY_FETCH=1`, `GIT_OPTIONAL_LOCKS=0`, and
`GIT_LITERAL_PATHSPECS=1`; the supervisor clears every inherited variable.
Git also receives `--no-replace-objects` and `--no-lazy-fetch` explicitly.
An entry declaring more than 64 KiB is rejected before the read. The verifier
first requires the recorded object to have raw type `commit`, then requires
each declared path to resolve as a raw `blob` at that commit. It will fail
closed when:

- the checkout has no usable Git repository;
- the declared base commit does not resolve to a commit object;
- a declared path is absent, non-blob, unreadable, or differs in byte length
  or SHA-256 from the manifest; or
- Git reports an error, times out, fails cleanup, exceeds the bounded capture
  contract, or lacks local historical objects.

No fallback reads the current working tree, accepts replacement objects, or
fetches promisor objects. The Git executable and repository object database are
trusted test-environment prerequisites; source archives, shallow clones, and
partial clones without the recorded objects fail rather than degrade. No digest,
file list, schema field, or base commit in the historical manifest is changed.

## Scope

Modify only:

- `tests/capability_pack_contract.rs` — replace the live-tree assertion with
  historical-object verification, a preserved-invalid-v1.0 assertion, and
  deterministic failure coverage;
- `docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/m2-manifest-v1.1.json`
  — corrected, immutable historical record generated from the declared base;
- `.github/workflows/rust-ci.yml` — fetch sufficient history only in the Linux/
  macOS test matrix that executes this contract, then explicitly provision the
  fixed base object from canonical `${{ github.repository }}` because it is not
  reachable from `main`;
- `docs/TESTING_AND_FAULT_INJECTION.md` — declare the Git-history prerequisite
  for this deterministic historical-evidence test;
- `docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/progress.md`
  — record the corrected historical semantics; and
- `CHANGELOG.md` only for the already-approved `macos-v5` user-visible record.

The production capability-pack library, CLI, schemas, receipt formats,
configuration, policies, and historical M2 manifest remain unchanged.

## Test strategy

1. Preserve the currently observed RED: a live-tree comparison fails after the
   `macos-v5` changelog entry.
2. Preserve v1.0 exactly and prove it fails against its declared historical
   objects without falling back to the working tree.
3. Add a v1.1 corrected record and prove it passes against the same declared
   historical commit and raw blobs.
4. Add focused deterministic tests for malformed identity/path, missing and
   non-commit objects, missing blobs, bad bytes/digests, supervisor timeout,
   overflow, read failure, nonzero exit, cleanup failure, and zero fallback.
5. Run the focused capability-pack contract, then the offline workspace suite,
   formatting, and strict Clippy checks.

## Non-goals

- No mutable "current M2" manifest.
- No update to v1.0 M2 hashes, file list, schema version, base commit, or
  filename.
- No GitHub, Docker, CCP guarded command, candidate installation, push, or PR.
