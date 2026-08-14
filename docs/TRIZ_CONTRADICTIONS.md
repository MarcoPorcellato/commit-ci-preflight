# T2 TRIZ contradictions

## Status

This is the implemented decision ledger for T2. It is based on the code paths
in `src/source_snapshot.rs`, `src/run.rs`, and `src/runtime.rs`, plus
[`docs/RELIABILITY_HARDENING_PLAN.md`](RELIABILITY_HARDENING_PLAN.md). It
separates deterministic implementation evidence from native qualification.

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

| Need | Conflict | Implemented resolution |
| --- | --- | --- |
| Exact commit fidelity | Before T2, the live repository mount could expose ignored files, generated state, local symlinks, LFS differences, or submodule drift | Materialize a CCP-owned immutable snapshot before admission |
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
- Conflating deterministic T2 implementation evidence with native or release
  qualification.

## Measurable gates

| Gate | Evidence | Status |
| --- | --- | --- |
| Same supported Git tree produces the same source digest | Deterministic snapshot manifest and digest tests | PASS in source |
| Ignored files and local noise cannot affect attestation | Git-object materialization plus snapshot-backed run test | PASS in source |
| Unsupported LFS or submodule states fail closed | Explicit typed rejection tests before admission | PASS in source |
| Snapshot identity reaches the verifier | Receipt v2 publication and v1/v2 verifier dispatch | PASS in source |
| Source is revalidated before sealing | Post-execution file, mode, blob and manifest checks | PASS in source |
| Cleanup is bounded and recoverable | Strict journal source binding and exact run ownership | PASS in source; native crash proof PENDING |

## Non-claim

T2 is implemented in source and deterministically tested. Native platform,
crash/power-loss, and release qualification remain pending and require
separate evidence.
