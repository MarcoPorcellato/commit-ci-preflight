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

The M2 contract test will verify each declared path against the Git object at
that recorded commit, not against the current checkout. It will use a bounded,
fixed-path Git read rooted at `CARGO_MANIFEST_DIR`: an entry declaring more
than 64 KiB is rejected before the read, and captured object output is capped
at 64 KiB. It will fail closed when:

- the checkout has no usable Git repository;
- the declared base commit does not resolve to a commit object;
- a declared path is absent, non-blob, unreadable, or differs in byte length
  or SHA-256 from the manifest; or
- Git reports an error or exceeds the test's bounded capture contract.

No fallback reads the current working tree. No digest, file list, schema field,
or base commit in the historical manifest is changed.

## Scope

Modify only:

- `tests/capability_pack_contract.rs` — replace the live-tree assertion with
  historical-object verification and deterministic failure coverage;
- `docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/progress.md`
  — record the corrected historical semantics; and
- `CHANGELOG.md` only for the already-approved `macos-v5` user-visible record.

The production capability-pack library, CLI, schemas, receipt formats,
configuration, policies, and historical M2 manifest remain unchanged.

## Test strategy

1. Preserve the currently observed RED: a live-tree comparison fails after the
   `macos-v5` changelog entry.
2. Add a focused test that expects historical M2 verification to pass from the
   recorded Git object.
3. Add a focused negative case for an unresolvable base commit or missing
   object; it must fail closed without reading the live tree.
4. Run the focused capability-pack contract, then the offline workspace suite,
   formatting, and strict Clippy checks.

## Non-goals

- No mutable "current M2" manifest.
- No update to M2 hashes, file list, schema version, or base commit.
- No GitHub, Docker, CCP guarded command, candidate installation, push, or PR.
