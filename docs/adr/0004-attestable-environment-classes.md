# ADR 0004: Attestable environment classes

- Status: Implemented for schema `1.1` normalization and local runtime
  enforcement; trusted verifier binding remains T7
- Tranche: T7 trusted-plan binding and T8 environment/resource contracts
- Date: 2026-08-20
- Decision owner: Marco Porcellato

## Context

Legacy configuration accepts `environment.allow`: names are normalized and
the Docker runtime copies the corresponding values from the host process. This
is explicit name allowlisting, but not a complete attestable environment
contract. In particular, the repository's own containerized Rust preflight uses
managed writable cache mounts while Cargo and Rustup need three matching
container paths. If those host values are not exported before the command,
Cargo can use a read-only image path and a format check can fail even though the
same check works locally.

Silently inventing host values would make the common path convenient but would
weaken the boundary: the declared plan would no longer identify which values
matter, and a verifier could not reconstruct them from trusted configuration.
The reliability plan therefore assigns environment values to T8, after T7
trusted-plan reconstruction.

## Decision

Schema `1.1` adds three explicit environment classes without reusing the
separate matrix schema version `2.0`. The normalized environment contract is a normative field of
the trusted execution plan; its canonical digest is bound to receipt v2 and
reconstructed by the trusted verifier.

| Class | Source | Container value | Attestable run behaviour |
| --- | --- | --- | --- |
| `fixed` | Literal non-secret configuration value | Exact declared value | Allowed after bounded validation |
| `runtime_internal` | Declared runtime resource, initially a managed cache ID | Deterministic target derived from the normalized mount | Allowed; no host environment read |
| `remote_secret_only` | Remote protected environment | Never materialized locally | Local attestable run fails closed before admission |

`fixed` values must be non-secret by contract. Their values never appear in a
receipt, dry-run output, resource history, error, or public evidence. The
trusted configuration is the sole verifier input that carries the literal.

`runtime_internal` initially supports a variable name bound to one declared
managed cache ID. CCP derives the container value from the normalized cache
mount under `/workspace`; an arbitrary host path or arbitrary literal is not a
runtime-internal value. The own Rust configuration will therefore bind
`CARGO_HOME`, `CARGO_TARGET_DIR`, and `RUSTUP_HOME` to their existing managed
cache IDs rather than requiring process exports.

`remote_secret_only` makes the limitation visible rather than treating an unset
host variable as a passing empty value. It preserves the remote protected-job
boundary and makes an attempted local receipt `PENDING`/non-PASS.

Configuration `1.0` remains parseable for compatibility. Its `environment.allow`
semantics are legacy host inheritance and cannot be promoted to a `1.1`
attestable-environment claim. Migration must be explicit, deterministic,
diff-first, and reversible; no automatic rewrite of a user configuration is
allowed.

## Verification and privacy

The future verifier loads the trusted config and policy from the trusted base,
normalizes the `1.1` environment contract independently, and compares its digest
and complete normative plan binding against the receipt. It does not read host
environment variables and does not need their values from untrusted evidence.

Receipts retain a bounded environment-contract digest and class-safe metadata
only. They do not serialize literal fixed values, secret values, home paths,
usernames, or inherited host values. Dry-run may show a class and a normalized
container target for a runtime-internal mapping, but never a fixed value.

## Consequences

Benefits:

- cache-backed toolchains become deterministic without a fragile shell export;
- plan verification can distinguish a fixed setting from a runtime resource or
  an intentionally remote secret;
- host variations become either forbidden, fixed by trusted configuration, or
  classified out of local attestation;
- the public A0 claim remains bounded and privacy-minimized.

Costs:

- configuration schema and plan compatibility require a deliberate `1.1` path;
- T7 must reconstruct the plan before T8 can claim a complete binding;
- legacy configurations remain supported but carry a weaker, explicit status;
- arbitrary dynamic environment injection remains unsupported.

## Rejected alternatives

- **Silently synthesize known Cargo variables from cache names:** convenient for
  one project, but hidden policy and impossible to generalize safely.
- **Continue host inheritance and document exports better:** preserves the
  failure mode and does not close the plan-binding gap.
- **Place secrets in fixed configuration or receipts:** violates the privacy and
  remote-trust boundary.
- **Allow arbitrary runtime-internal paths:** turns a bounded resource reference
  into an unreviewable host path injection mechanism.
- **Block all legacy v1 parsing immediately:** breaks compatibility without
  improving evidence for existing historical receipts.

## Implementation gates

1. Characterization tests show that changed host values cannot silently affect
   a `1.1` attestable plan.
2. Golden tests prove deterministic cache-ID-to-container-target resolution and
   rejection of missing, duplicate, or incompatible cache references.
3. Receipt v2 and verifier tests reject every changed environment class,
   normalized target, fixed digest, or secret-only misuse.
4. Migration tests preserve v1 parsing and require an explicit v2 proposal.
5. A clean exact-head CCP run verifies the repository Rust configuration without
   the three legacy host exports, then an independent verifier confirms receipt
   eligibility.

No native-platform, signing, or stable-release claim follows from this ADR.
