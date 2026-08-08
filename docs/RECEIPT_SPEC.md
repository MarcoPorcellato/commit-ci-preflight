# Receipt specification v1

## Status

This document specifies the implemented receipt contract for schema version
`1.0`. A receipt is deterministic integrity evidence. It is **not** an
identity-bound attestation and does not prove that the producer or host was
trusted.

Normative terms **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are interpreted
as requirements within this project.

## Envelope

A receipt document is a UTF-8 JSON object containing:

- `receipt_id`: lowercase `sha256:` digest of the canonical `receipt` payload;
- `receipt`: a `ReceiptV1` payload conforming to
  [`../schema/receipt-v1.schema.json`](../schema/receipt-v1.schema.json).

Unknown fields are rejected at every typed object boundary.

## Canonical JSON profile

The v1 canonicalizer:

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

The committed pass fixture is the normative golden vector:
[`../tests/fixtures/receipt-v1-pass.json`](../tests/fixtures/receipt-v1-pass.json).

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

The typed v1 schema intentionally has no fields for environment values, raw
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

Policy verification for freshness, accepted platforms, exact pull-request SHA,
required check sets, and trusted identities belongs to later phases and must
remain distinguishable from structural integrity.

## Regeneration

From the repository root:

```console
cargo run --example generate_contract
cargo test --test receipt_contract
```

Regeneration is accepted only when the schema/fixture diff is intentionally
reviewed. Tests require generated and pinned bytes to remain identical.

