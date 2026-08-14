# ADR 0002: Immutable Git-object snapshots for T2

- Status: Proposed
- Tranche: T2 in progress
- Date: 2026-08-14
- Decision owner: Marco Porcellato

## Context

The current implementation mounts the repository path read-only at
`/workspace`, while still reading whatever bytes exist in the user's working
tree at runtime. `src/workspace.rs` canonicalizes the repository path,
validates that it is a directory, and binds it into the container as a
read-only repository mount. `src/runtime.rs` renders explicit `docker run`
argv with `--read-only` and separate writable cache and artifact mounts.

That design is a deliberate containment contract, but it is not yet immutable
source materialization. The hardening plan names this gap as P0-2 and assigns
it to PR T2:

- a CCP-owned exact-commit snapshot outside the user's working tree;
- a canonical manifest over path, mode, blob OID, and submodule OID;
- `source_snapshot_digest` in the execution contract and receipt v2;
- explicit policy for submodules, Git LFS, executable bits, symlinks, and
  sparse-checkout;
- read-only runtime mount of the snapshot only;
- bounded snapshot cleanup and recovery through the journal.

The same plan also records the current failure mode: a clean checkout can still
expose ignored files, generated state, local symlinks, LFS differences, or
submodule drift through the live source mount.

## Decision

T2 should materialize a CCP-owned immutable source snapshot for attestable
execution and mount only that snapshot at runtime.

The preferred implementation direction is:

1. Materialize the exact commit from Git objects or tree metadata, not from the
   mutable working tree.
2. Record a canonical source manifest that includes the path, mode, blob OID,
   and submodule OID for every admissible entry.
3. Bind the execution contract and receipt v2 to a `source_snapshot_digest`.
4. Revalidate source identity after execution and before receipt sealing.
5. Treat unsupported sparse-checkout, submodule, LFS, executable-bit, or
   symlink states as fail-closed conditions before admission.

This ADR does not accept a live-worktree mount as sufficient evidence for T2.
It also does not claim that GitHub, package publication, or release qualification
depend on this work.

## Consequences

Benefits:

- ignored files, `.env`, generated files, IDE state, and local cache noise stop
  influencing attestable execution;
- the exact supported Git tree has a stable source identity that can be
  compared across runs;
- source identity becomes a first-class input to verification instead of an
  implicit property of the checkout;
- cleanup and recovery can be made journaled and bounded rather than ad hoc.

Costs:

- an additional source-materialization step must run before execution;
- the snapshot policy becomes stricter for unsupported Git states;
- receipt v2 and the run journal need new fields and regression coverage;
- snapshot cleanup must be bounded and ownership-aware.

## Rejected alternatives

- **Live working tree mount:** preserves the current behavior, but does not
  close the commit-to-byte fidelity gap described by T2.
- **Whole-tree copy without object identity:** isolates the bytes, but still
  lacks a canonical path/mode/OID manifest and weakens provenance.
- **Silent fallback for unsupported Git states:** reduces immediate failures,
  but would reintroduce ambiguous evidence and non-deterministic source
  identity.
- **Treating `git status` cleanliness as proof:** the current docs already show
  that a clean checkout is not enough for commit-to-byte fidelity.

## Guardrails

- No attestable run without a source snapshot digest.
- No PASS if source identity is uncertain, unsupported, or stale.
- No silent reinterpretation of unsupported LFS or submodule states.
- No claim that T2 is implemented or qualified until the snapshot path is
  covered by exact-commit regression evidence.
