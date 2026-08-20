# T7/T8 TRIZ environment-contract ledger

## Ideal final result

An operator can run a declared preflight without remembering fragile shell
exports, while an independent verifier can still determine exactly which
environment contract applied and no secret or private path reaches evidence.

## Contradictions and resolution

| Contradiction | Naive compromise | T8 resolution |
| --- | --- | --- |
| Simple first run versus complete evidence | Copy whatever variables happen to be present | Separate fixed, runtime-internal, and remote-secret-only classes in the normalized plan |
| Reusable caches versus immutable source/runtime boundary | Use arbitrary host paths as environment values | Refer only to declared managed cache IDs and derive the container target |
| Verifiability versus privacy | Store values in the receipt | Reconstruct from trusted configuration and bind a canonical digest, without serializing values |
| Stronger evidence versus historical compatibility | Reinterpret all v1 receipts as trusted plans | Keep v1 policy semantics stable and make v1.1 require v2 plus an independent comparison |
| Full comparison versus bounded verifier output | Print the mismatched command or environment value | Emit bounded JSON-pointer findings only, with no compared values |
| Compatibility versus stronger semantics | Break all v1 configurations | Keep v1 parseable, require explicit deterministic migration for v2 attestation |
| Local speed versus remote-secret safety | Treat missing secret as empty or optional | Fail closed before admission and retain the remote protected-job path |

## TRIZ principles applied

| Principle | Applied decision |
| --- | --- |
| Segmentation | Split one overloaded allowlist into three independently governed classes |
| Taking out | Remove secrets and accidental host inheritance from local attestable execution |
| Prior action | Normalize and validate environment resources before admission or runtime start |
| Intermediary | Use a cache-ID binding as an auditable intermediate between configuration and container target |
| Copying | Carry a public normalized execution plan in receipt v2, then compare it with an independently reconstructed trusted copy |
| Composite action | Require self-integrity, trusted-plan comparison, source strategy, and producer tuple before one policy PASS |
| Local quality | Give cache-backed toolchains, fixed settings, and remote secrets different rules |
| Feedback | Expose class-safe dry-run diagnostics and fail before a costly container run |
| Parameter change | Make v2 strengthening opt-in and preserve historical v1 interpretation |

## Resources reused

- strict configuration parsing and canonical plan digests;
- declared managed cache IDs and normalized workspace mounts;
- explicit shell-free Docker argv rendering;
- receipt v2 dispatch and trusted-base policy verification;
- pre-admission validation and existing fail-closed evidence statuses.

No new runtime, shell, secret store, host path parser, or vendor integration is
needed for the initial cache-backed use case.

## Anti-patterns

- detecting Cargo names and silently changing an attestable plan;
- reading any unclassified host variable during a v2 attestable run;
- placing values or private paths in receipts, diagnostics, resource history, or
  public templates;
- calling an unexecuted remote-secret job locally qualified;
- treating a v1 migration as a harmless formatting rewrite.

## Measurable evidence

| Gate | Evidence required |
| --- | --- |
| Deterministic runtime target | Golden vector from cache ID to normalized container target |
| Host independence | Different ambient host values yield the same v2 runtime argv and receipt plan binding |
| Secret boundary | `remote_secret_only` rejects local attestation before admission |
| Verifier integrity | Any changed class, cache reference, or fixed-value digest fails independent verification |
| Trusted-plan comparison | A receipt v2 changed in argv, runtime, mounts, environment class, dependency, artifact, or limit emits a bounded non-PASS field finding |
| Ergonomic regression | Repository Rust preflight works with declared runtime-internal cache bindings and no legacy exports |

This ledger defines design and tests, not proof that a platform run or release
has completed.
