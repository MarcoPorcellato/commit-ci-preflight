# Receipt specification v1 and v2

## Status

This document specifies the implemented receipt contracts for schema versions
`1.0` and `2.0`. A receipt is deterministic integrity evidence. It is **not** an
identity-bound attestation and does not prove that the producer or host was
trusted.

Normative terms **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are interpreted
as requirements within this project.

## Envelope

A receipt document is a UTF-8 JSON object containing:

- `receipt_id`: lowercase `sha256:` digest of the canonical `receipt` payload;
- `receipt`: either a `ReceiptV1` payload conforming to
  [`../schema/receipt-v1.schema.json`](../schema/receipt-v1.schema.json), or a
  `ReceiptV2` payload conforming to
  [`../schema/receipt-v2.schema.json`](../schema/receipt-v2.schema.json).

Unknown fields are rejected at every typed object boundary.

Schema v2 preserves the common v1 evidence and adds required
`source_snapshot` evidence plus a complete public `execution_plan`. The plan
contains normalized checks, runtime limits, caches and class-safe environment
metadata; it never contains a fixed literal or a secret value. Its canonical
digest MUST equal `configuration_digest`. New snapshot-backed runs publish v2;
historical v1 documents remain strictly readable and are never reinterpreted as
carrying snapshot or plan-binding assurance.
The pinned v2 schema and vector are `schema/receipt-v2.schema.json` and
`tests/fixtures/receipt-v2-pass.json`.

## Canonical JSON profile

The shared v1 canonical JSON profile used by both receipt schemas:

1. serializes the typed Rust value into a JSON value;
2. recursively sorts every object key in ascending string order;
3. preserves array order;
4. emits compact UTF-8 JSON with no trailing newline;
5. uses only integer numbers in the v1 contract;
6. computes SHA-256 over the exact canonical payload bytes;
7. renders the digest as `sha256:` followed by 64 lowercase hexadecimal digits.

This is the **CCP canonical JSON v1 profile**. It must not be described as RFC
8785 unless a later ADR establishes complete RFC conformance and cross-language
vectors.

The committed pass fixtures are the normative golden vectors:
[`../tests/fixtures/receipt-v1-pass.json`](../tests/fixtures/receipt-v1-pass.json)
and [`../tests/fixtures/receipt-v2-pass.json`](../tests/fixtures/receipt-v2-pass.json).

## Source snapshot evidence in v2

Receipt v2 requires `source_snapshot.schema_version = "1.0"`, strategy
`git_object`, a canonical `sha256:` manifest digest, and a positive entry
count. The receipt intentionally does not include source contents, host paths,
worktree paths or user identity. The verifier recomputes the envelope digest;
changing any snapshot field invalidates receipt integrity before policy is
evaluated.

The journal stores a separate private `source-snapshot-v1.json` binding with
the exact commit, manifest digest, entry count and fixed CCP-owned resource ID.
That record supports bounded recovery; it is not copied into the receipt and
does not add host-path disclosure.

## Execution-plan evidence in v2

The embedded normalized plan makes the execution claim inspectable and binds
every plan field into the receipt integrity identifier. A changed plan cannot
retain the declared configuration digest and remain semantically valid.

Receipt self-consistency alone is not trusted-plan verification. Policy `1.1`
implements the additional boundary: its verifier reconstructs the normalized
plan from the policy-relative trusted configuration and compares it field by
field with this public plan. It rejects a mismatch before a PASS decision, while
continuing to exclude fixed literal and secret values. This comparison does not
establish cryptographic producer identity or host attestation.

## Runtime-capability evidence in v2

`runtime_capability_evidence` is an optional JSON member with a
schema-dependent semantic requirement. It is **required** when the embedded
execution plan has `schema_version = "1.3"`; it is **forbidden** for every
historical plan schema. This keeps historical receipt bytes and meanings
unchanged while making the v1.3 runtime claim inspectable.

For a v1.3 plan, the evidence MUST contain `schema_version = "1.0"`,
`memory_limit_supported = true`, `swap_limit_supported = true`, a canonical
SHA-256 `context_digest`, a canonical SHA-256 `resolved_image_id`, and a
`resolved_image_reference` exactly equal to the runtime image reference in the
embedded plan. The receipt validator rejects a missing, malformed, mismatched,
or historically unexpected evidence member.

The evidence carries no Docker context literal, command output, daemon
identity, host path, credential, or assurance that the host itself is trusted.
It binds the declaration that the bounded preflight observed the required
capabilities and pinned image metadata before the local run began. It is
integrity evidence, not host attestation or a proof that a future runtime will
retain those capabilities.

## Artifact-manifest evidence in v2

`artifact_manifest` is backward-compatible as an empty or omitted field for a
historical plan with no `artifact_contracts`. When a plan declares artifact
contracts, v2 validation requires an exact path-for-path manifest match. Each
record carries the contract path, producer check ID, declared kind, observed
entry count, observed total bytes, and a canonical `sha256:` digest of the
bounded final-state content manifest.

For a regular file, `entry_count` MUST be exactly one. For a directory, every
entry is traversed in deterministic path order. Missing output, a symlink or
unsafe object, an escaped ancestor, concurrent replacement, or an exceeded
size or entry limit prevents the run from sealing a receipt. The receipt
contains neither artifact contents nor local filesystem paths. This
final-state slice does not yet claim an initial-state manifest, artifact
retention, or cross-host artifact reconstruction; those remain separate
reliability work.

## Status semantics

| Status | Normative meaning |
|---|---|
| `PASS` | The check ran, returned exit code zero, and was neither timed out nor cancelled. |
| `FAIL` | The check ran unsuccessfully, timed out, or was cancelled. |
| `PENDING` | Required evidence is expected later and the check did not run. |
| `NOT_RUN` | The check deliberately did not run in this receipt. |

`PENDING` and `NOT_RUN` MUST carry a non-empty `incomplete_reason`. They MUST
NOT carry an exit code, duration, timeout/cancellation flag, or output digest.

The overall status is derived from required checks:

1. `FAIL` if any required check failed;
2. otherwise `PENDING` if any required check is `PENDING` or `NOT_RUN`;
3. otherwise `PASS`.

At least one check and at least one required check are mandatory. Duplicate
check IDs are invalid.

## Identity and input binding

- `repository.repository` is a logical `owner/name` identifier. URLs,
  credentials, extra path segments, and control characters are invalid.
- `repository.commit_sha` is exactly 40 or 64 lowercase hexadecimal digits.
- `configuration_digest`, image digest, optional output digests, and receipt ID
  use the lowercase `sha256:` representation.
- `platform.image_reference` MUST end in `@` plus the exact `image_digest`.
- Working directories are repository-relative and cannot contain parent
  traversal or absolute roots.

These constraints bind the receipt to stated inputs but do not establish who
created it.

## Time

Timestamps use exactly `YYYY-MM-DDTHH:MM:SSZ`, including Gregorian leap-year
validation. Fractional seconds and offsets other than `Z` are excluded from v1
to keep canonical fixtures simple. `finished_at_utc` cannot precede
`started_at_utc`.

Production builders MUST receive a clock through an explicit port. Tests and
fixtures MUST NOT read the wall clock.

## Privacy

The typed v1 and v2 schemas intentionally have no fields for environment values, raw
stdout/stderr, source contents, usernames, email addresses, machine names, IP
addresses, or absolute home paths. Output may be represented only by a digest.

Producers are still responsible for ensuring that free-text identifiers and
incomplete reasons do not contain secrets or personal data. A later builder PR
will add bounded redaction before these types are populated.

## Verification

Structural and integrity verification MUST:

- reject malformed JSON and unknown fields;
- reject unsupported schema versions;
- enforce all semantic invariants above;
- recompute the canonical payload digest;
- compare it exactly with `receipt_id`.

Policy verification for freshness, accepted platforms, exact externally
supplied commit SHA, required check sets, image, configuration, and—under
policy `1.1`—trusted-plan reconstruction is specified in
[`VERIFICATION_POLICY.md`](VERIFICATION_POLICY.md). It remains distinguishable
from structural integrity. Trusted identity and signatures belong to later
phases.

## Regeneration

From the repository root:

```console
cargo run --example generate_contract
cargo test --test receipt_contract
```

Regeneration is accepted only when the schema/fixture diff is intentionally
reviewed. Tests require generated and pinned bytes to remain identical.
## Compatibility-profile boundary

The producer version is `0.1.0+matrix-v2-legacy-v1`. The receipt boundary is
outer Matrix schema 2.0 with inner runtime schema 1.0. Reviewed digests are
never from a completed receipt.

Receipts produced under `matrix-v2-legacy-v1` are Matrix-only compatibility
evidence and retain the producer suffix. The profile applies to `plan`,
`doctor`, `dry-run`, and `run`; `verify` has no profile flag. A historical
verifier must accept the exact receipt before Matrix policy v2 migration. This
does not widen the schema, infer policy, or establish general trust.
