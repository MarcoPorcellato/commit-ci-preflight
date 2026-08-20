# Independent verification and repository policy v1 / v1.1

## Scope

`commit-ci-preflight verify` is a read-only path separate from local execution.
It parses a receipt, proves structural and digest integrity, then evaluates a
strict repository policy. Policy `1.1` additionally reconstructs the normalized
execution plan from a configuration stored beside the trusted policy and
compares it field by field with a receipt v2. The verifier does not import or
invoke the run orchestrator, Docker adapter, cache, or workspace code.

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

The implemented GitHub adapter follows that rule: it supplies the pull-request
head SHA as the expected commit and uses the trusted runner clock. See
[`GITHUB_GATE.md`](GITHUB_GATE.md).

## Policy files

The compatibility policy schema `1.0` is pinned at
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

Policy `1.1` is pinned separately at
[`../schema/policy-v1_1.schema.json`](../schema/policy-v1_1.schema.json). It
preserves every `1.0` field and adds a trusted, policy-relative configuration,
an accepted source-snapshot strategy, and exact supported/revoked producer
tuples:

```toml
schema_version = "1.1"
project = "example/project"
configuration_digest = "sha256:..."
required_checks = ["rust-test"]
image_reference = "example.invalid/ci@sha256:..."
max_age_seconds = 3600
trusted_config = ".commit-ci-preflight.toml"
source_snapshot_strategy = "git-object"

[[supported_producers]]
name = "commit-ci-preflight"
version = "0.1.0"
```

`trusted_config` MUST be a regular non-symlink file addressed by a safe
relative path below the policy directory. It is selected only by the trusted
policy; the receipt, command line, and evidence branch cannot redirect it.
Policy `1.1` requires receipt schema `2.0`. The verifier independently
normalizes the trusted configuration, confirms its digest equals the policy
digest, then reports bounded JSON-pointer field mismatches without revealing
the compared values. It compares runtime limits and network mode, receipt
settings, environment classes and fixed-value digests, caches, checks,
arguments, working directories, dependencies, artifacts, source strategy, and
producer identity. A signed receipt cannot mask any such mismatch.

## Machine report

The report schema is pinned at
[`../schema/verification-report-v1.schema.json`](../schema/verification-report-v1.schema.json).
Canonical JSON uses stable fields and finding order:

- `integrity_status`: receipt shape, semantics, and digest;
- `policy_status`: repository policy, or `NOT_RUN` if integrity failed;
- `decision`: `PASS` only when both preceding statuses pass;
- `findings`: stable code, field, and non-sensitive explanation;
- `assurance_scope`: `integrity_and_repository_policy_only` for policy `1.0`,
  or `integrity_and_trusted_plan_policy` for policy `1.1`.

Exit code 0 means verification PASS. Exit code 3 means receipt integrity or
policy failure. Invalid CLI/policy inputs use 2; internal serialization failure
uses 70.

## Fail-closed and assurance limits

Malformed JSON, unknown receipt fields, unsupported schemas, semantic
violations, digest mismatch, future timestamps, stale evidence, and every
covered policy mismatch fail closed. The verifier never repairs, reseals, or
executes a receipt.

A digest-valid receipt can still be false evidence. Policy `1.1` constrains the
declared producer name/version and trusted execution plan, but does not prove
the producer's cryptographic identity, host trust, signature validity, GitHub
event identity, reviewer state, permissions, or native evidence for a platform
that was not executed. Those claims remain outside this policy and must not be
inferred from a PASS report.

The GitHub gate adds event binding and a commit status, but does not upgrade the
unsigned receipt into producer identity or host attestation.

For the separate v2 policy that binds every required check to a named pinned
runtime, see [`MULTI_RUNTIME_RECEIPTS.md`](MULTI_RUNTIME_RECEIPTS.md). The v1
policy and its historical receipts remain unchanged.
