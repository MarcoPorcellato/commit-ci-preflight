# T2 TRIZ contradictions

## Status

This is a proposed analysis for T2 only. It is based on the current code paths
in `src/workspace.rs` and `src/runtime.rs`, plus
[`docs/RELIABILITY_HARDENING_PLAN.md`](RELIABILITY_HARDENING_PLAN.md). It does
not claim that immutable snapshots, source digests, or receipt v2 are already
implemented or qualified.

## Ideal final result

The ideal result is an attestable run that uses the exact commit bytes, proves
which bytes were used, and never depends on the user's mutable working tree or
on undocumented local state.

The ideal system would:

- mount only CCP-owned immutable source bytes;
- carry a canonical source digest into execution and verification;
- fail closed on unsupported Git states;
- keep cleanup bounded and recoverable through the journal;
- preserve the current explicit argv and read-only runtime boundary.

## Engineering contradictions

| Need | Conflict | Proposed resolution |
| --- | --- | --- |
| Exact commit fidelity | The current live repository mount can still expose ignored files, generated state, local symlinks, LFS differences, or submodule drift | Materialize a CCP-owned immutable snapshot before admission |
| Strong provenance | A plain checkout or whole-tree copy does not identify every admissible object | Record a canonical manifest over path, mode, blob OID, and submodule OID |
| Tight failure handling | Unsupported Git states should not sneak through as partial support | Fail closed before admission and after source revalidation |
| Bounded recovery | Snapshot cleanup must not become an untracked side effect of execution | Journal snapshot identity and cleanup state separately from the run phase |

## Physical contradictions

| Object | Must be true | Must also be true |
| --- | --- | --- |
| Source bytes | Immutable for attestable execution | Recoverable for cleanup and replay |
| Snapshot location | CCP-owned and outside the user's mutable working tree | Cheap enough to create and inspect for every run |
| Policy coverage | Strict for sparse-checkout, LFS, submodules, symlinks, and executable bits | Narrow enough to keep the supported cases simple and deterministic |
| Evidence timing | Stable before sealing | Revalidated after execution, before receipt sealing |

## Separation principles

- Separation in space: keep the live checkout, the CCP-owned snapshot, and the
  run journal in distinct roles and paths.
- Separation in time: materialize and validate before admission; revalidate
  after execution; clean up only after the journal records a terminal state.
- Separation by condition: supported Git states proceed; unsupported states
  fail closed.
- Separation by part: source manifest fields, journal state, and receipt
  evidence should be separate records with clear ownership.

## Resources

The T2 design can reuse existing resources already present in the repository:

- exact Git commit identity;
- tree/blob/submodule object IDs;
- path and mode metadata;
- the managed cache root and workspace lock discipline;
- canonical JSON and SHA-256 receipt machinery;
- explicit argv rendering and the current read-only container boundary;
- the run journal and cleanup classification from earlier hardening work.

These resources should be sufficient for T2 without adding a new runtime or
shell layer.

## Inventive principles

| Principle | T2 use |
| --- | --- |
| Segmentation | Materialize the source as object-level snapshot data instead of treating the checkout as one mutable whole |
| Taking out | Exclude ignored files, generated files, IDE state, and other non-source noise from attestable execution |
| Prior action | Build and validate the snapshot before admission and execution |
| Local quality | Apply separate policy to submodules, LFS, symlinks, sparse-checkout, and executable bits |
| Intermediary | Use a canonical manifest and `source_snapshot_digest` between source materialization and receipt sealing |
| Copying | Prefer Git objects and tree metadata over ad hoc file reconstruction |
| Partial or excessive action | Fail closed on ambiguous states rather than partially supporting them |

## Anti-patterns

- Treating a clean checkout as proof of source identity.
- Mounting the user's live working tree for attestable execution.
- Silently accepting unsupported LFS or submodule states.
- Letting generated files, IDE state, or local caches influence attestation.
- Recomputing source identity only after receipt sealing.
- Describing T2 as implemented or qualified before snapshot evidence exists.

## Measurable gates

| Gate | Evidence needed |
| --- | --- |
| Same supported Git tree produces the same source digest | Deterministic snapshot manifest and digest tests |
| Ignored files and local noise cannot affect attestation | Regression coverage that changes only ignored or generated state |
| Unsupported LFS or submodule states fail closed | Explicit failure tests before admission |
| Snapshot identity reaches the verifier | `source_snapshot_digest` in the execution contract and receipt v2 |
| Source is revalidated before sealing | A post-execution check that compares the bound snapshot identity |
| Cleanup is bounded and recoverable | Journaled snapshot lifecycle with exact ownership markers |

## Non-claim

T2 is still proposed and in progress. Nothing in this document should be read
as implementation evidence, release qualification, or a claim that the live
workspace mount is already replaced.
