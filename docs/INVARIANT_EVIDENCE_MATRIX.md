# T2 invariant evidence matrix

## Status

This matrix is for the proposed T2 snapshot tranche only. It separates current
evidence from the evidence that T2 still needs. It does not claim that T2 is
implemented or qualified.

## Reading guide

- **Current evidence**: what the current code or docs already show.
- **Gap**: what is still missing for T2.
- **T2 proof artifact**: the evidence T2 should produce.
- **Gate**: the measurable check that would close the gap.

## Matrix

| Invariant | Current evidence | Gap | T2 proof artifact | Gate |
| --- | --- | --- | --- | --- |
| Exact commit bytes are isolated from the user's mutable working tree | `src/workspace.rs` mounts the canonicalized repository path read-only at `/workspace`; `src/runtime.rs` renders explicit `docker run` argv with `--read-only`; `docs/CACHE_AND_WORKSPACE.md` calls this a containment contract, not a complete sandbox | The current mount still comes from the live checkout, so ignored files, generated state, IDE files, local symlinks, LFS differences, or submodule drift can still influence execution | CCP-owned immutable source snapshot materialized outside the working tree | Changing only ignored or generated state does not change the snapshot digest or execution outcome |
| Source identity is canonical and reproducible | The plan already uses deterministic config and receipt hashing, but the source side still lacks a manifest and digest | There is no canonical path/mode/blob OID/submodule OID manifest yet | Canonical source manifest plus `source_snapshot_digest` | The same supported Git tree yields the same digest on repeated runs |
| Unsupported Git states fail closed | Current docs and code validate mount targets and directory shape, but not the full T2 Git-state policy | No explicit policy yet for sparse-checkout, submodules, LFS, executable bits, or symlinks at the source-snapshot layer | Source policy table and failure cases for unsupported states | Any unsupported source state stops before admission |
| Receipt evidence binds source identity | Receipt v1 and the current architecture track plan and runtime evidence, not source snapshot identity | `source_snapshot_digest` is absent from the current receipt contract | Receipt v2 with source snapshot binding | Verifier rejects mismatched source identity even if other fields match |
| Source identity is revalidated before sealing | Current `run` flow seals receipts after execution and cache completion, but without immutable source evidence | No post-execution source revalidation step exists for T2 | Post-execution source revalidation record in the journal | Receipt sealing does not occur unless the bound snapshot still matches |
| Snapshot cleanup is bounded and recoverable | The current workspace layer already uses a `.run-lock-v1` lock and managed cache ownership rules | There is no T2 snapshot journal entry or cleanup/recovery classification yet | Journaled snapshot lifecycle with explicit ownership markers | Cleanup is reproducible, bounded, and does not depend on ad hoc operator action |
| T2 remains proposed, not implemented | The hardening plan marks T2 as a deliverable tranche and defines its exit gate | No implementation or qualification receipt exists for T2 | ADR, TRIZ analysis, and this matrix | No document in this tranche may claim PASS or qualification |

## Current boundary

The current codebase is already explicit about a few useful boundaries:

- the repository mount is read-only;
- writable state is limited to declared cache and artifact bindings;
- the runtime renders explicit argv instead of using an implicit shell;
- the hardening plan already identifies commit-to-byte fidelity as the T2 gap.

Those are the starting point, not the finish line. T2 is the tranche that
turns those boundaries into immutable source evidence.
