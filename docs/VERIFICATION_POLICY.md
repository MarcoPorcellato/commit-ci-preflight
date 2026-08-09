# Independent verification and repository policy v1

## Scope

`commit-ci-preflight verify` is a read-only path separate from local execution.
It parses a receipt, proves structural and digest integrity, then evaluates a
strict repository policy. It does not import or invoke the run orchestrator,
Docker adapter, cache, or workspace code.

```console
commit-ci-preflight verify \
  --receipt .ccp/receipt.json \
  --policy .commit-ci-policy.toml \
  --expected-commit 0123456789abcdef0123456789abcdef01234567 \
  --json
```

The default evaluation instant is the verifier's system clock. An orchestrator
may provide `--evaluated-at-utc YYYY-MM-DDTHH:MM:SSZ` for deterministic replay.
Both the expected commit and an explicit evaluation instant are caller trust
inputs and are copied into the report. A future remote gate must derive them
from its trusted event and clock, never from the uploaded receipt.

## Policy file

The strict TOML schema is pinned at
[`../schema/policy-v1.schema.json`](../schema/policy-v1.schema.json). Unknown
fields, duplicate values, unsupported versions, unpinned images, malformed
digests, and oversized inputs fail before receipt evaluation.

```toml
schema_version = "1.0"
project = "example/project"
configuration_digest = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
required_checks = ["rust-test"]
image_reference = "example.invalid/ci@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
max_age_seconds = 3600

[[platforms]]
host_os = "macos"
host_arch = "aarch64"
runtime_kind = "docker_compatible"
```

Policy requires:

- exact logical project identity;
- exact externally supplied Git SHA;
- clean repository evidence;
- exact normalized configuration digest;
- exact immutable image reference;
- one accepted `(host_os, host_arch, runtime_kind)` tuple;
- exact required-check set and PASS result for every required check;
- overall PASS;
- completion no later than evaluation time and no older than
  `max_age_seconds`.

`runtime_kind` matches the receipt adapter contract. The current local runner
emits `docker_compatible`; engine flavor such as OrbStack is evidence reported
by runtime probing, not a portable policy alias.

The configuration digest intentionally binds policy to the full normalized
execution plan. Cache bytes are mutable acceleration state and are not part of
this assertion.

## Machine report

The report schema is pinned at
[`../schema/verification-report-v1.schema.json`](../schema/verification-report-v1.schema.json).
Canonical JSON uses stable fields and finding order:

- `integrity_status`: receipt shape, semantics, and digest;
- `policy_status`: repository policy, or `NOT_RUN` if integrity failed;
- `decision`: `PASS` only when both preceding statuses pass;
- `findings`: stable code, field, and non-sensitive explanation;
- `assurance_scope`: always `integrity_and_repository_policy_only` in v1.

Exit code 0 means verification PASS. Exit code 3 means receipt integrity or
policy failure. Invalid CLI/policy inputs use 2; internal serialization failure
uses 70.

## Fail-closed and assurance limits

Malformed JSON, unknown receipt fields, unsupported schemas, semantic
violations, digest mismatch, future timestamps, stale evidence, and every
covered policy mismatch fail closed. The verifier never repairs, reseals, or
executes a receipt.

A digest-valid receipt can still be false evidence. This tranche verifies
integrity and repository policy only. It does not establish producer identity,
host trust, signature validity, GitHub event identity, reviewer state,
permissions, or native evidence for a platform that was not executed. Those
claims remain outside v1 and must not be inferred from a PASS report.
