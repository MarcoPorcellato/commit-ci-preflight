# Multi-runtime receipts v2

## Status and scope

Receipt/configuration/policy v2 provides one local, exact-head qualification
for checks that must run in more than one independently pinned runtime. It is
designed for compatibility matrices such as Python 3.11 and Python 3.12. It
does not change v1 parsing, receipt IDs, verification, schemas, or historical
evidence.

The local runner still requires a clean Git checkout, a successful host
admission decision, and the normal macOS resource watchdog. It executes no
shell implicitly and does not turn an unsigned receipt into producer identity
or a substitute for review, permissions, secrets, or uncovered native checks.

## Why v2 is a new contract

A v1 receipt has exactly one platform/image and its checks carry no runtime
identity. Appending a second check would not prove which image ran it. V2
therefore contains:

- a normalized outer configuration digest covering every named runtime and
  check assignment;
- one sealed v1 receipt per named runtime, held inside a single outer receipt;
- an outer v2 digest over the complete matrix;
- a v2 repository policy mapping every required check to exactly one named
  runtime and exact image/platform/per-runtime-plan expectation.

The verifier rejects missing, duplicate, unknown, or substituted runtime IDs;
an image mismatch; a required-check set mismatch; a check executed by the
wrong runtime; stale/future evidence; or any inner or outer integrity failure.
All such cases fail closed.

## Configuration v2

`schema_version = "2.0"` selects the matrix contract. Runtime IDs are stable
ASCII identifiers, are canonicalized in lexical order, and every runtime must
own at least one required check. Cross-runtime dependencies are rejected rather
than guessed; split the checks into independent validation stages when such a
dependency would otherwise be needed.

```toml
schema_version = "2.0"
project = "example/project"

[receipt]
output = ".ccp/receipt.json"
freshness_seconds = 3600

[[runtimes]]
id = "python311"
kind = "docker_compatible"
image = "registry.example/python311@sha256:<64 lowercase hex>"
cpu_count = 1
memory_mib = 512
pids_limit = 64
network = false

[[runtimes]]
id = "python312"
kind = "docker_compatible"
image = "registry.example/python312@sha256:<64 lowercase hex>"
cpu_count = 1
memory_mib = 512
pids_limit = 64
network = false

[[checks]]
id = "compat-py311"
runtime_id = "python311"
required = true
argv = ["python", "-m", "pytest", "-q"]
working_directory = "."
timeout_seconds = 180

[[checks]]
id = "repository-check"
runtime_id = "python312"
required = true
argv = ["python", "scripts/repository_check.py"]
working_directory = "."
timeout_seconds = 180
```

`commit-ci-preflight run --config .commit-ci-preflight.toml` detects v2 and
executes the named runtime groups sequentially under one admission/watchdog
session. Inner receipts remain in memory until one create-new outer receipt is
atomically written. No intermediate receipt dirties the source tree observed
by a later runtime.

## Policy v2

Use `schema/policy-v2.schema.json` and explicitly bind both dimensions:

```toml
schema_version = "2.0"
project = "example/project"
configuration_digest = "sha256:<64 lowercase hex>"
max_age_seconds = 3600

[[required_checks]]
id = "compat-py311"
runtime_id = "python311"

[[required_checks]]
id = "repository-check"
runtime_id = "python312"

[[runtimes]]
id = "python311"
image_reference = "registry.example/python311@sha256:<64 lowercase hex>"
configuration_digest = "sha256:<python311 normalized v1 plan digest>"
platforms = [{ host_os = "macos", host_arch = "aarch64", runtime_kind = "docker_compatible" }]

[[runtimes]]
id = "python312"
image_reference = "registry.example/python312@sha256:<64 lowercase hex>"
configuration_digest = "sha256:<python312 normalized v1 plan digest>"
platforms = [{ host_os = "macos", host_arch = "aarch64", runtime_kind = "docker_compatible" }]
```

The existing `verify` command selects v1 or v2 from the trusted policy file:

```console
commit-ci-preflight verify --receipt .ccp/receipt.json \
  --policy .commit-ci-policy.toml --expected-commit <exact-sha>
```

Obtain the outer and per-runtime normalized plan digests without starting a
runtime:

```console
commit-ci-preflight plan --config .commit-ci-preflight.toml
```

The `--json` form includes the same named per-runtime digests bound by the
outer matrix digest. Copy those reviewed values into the v2 policy before a
qualification; never derive them from a completed receipt.

The GitHub receipt gate continues to run only the verifier against trusted base
policy and the event head. It does not check out, build, or execute candidate
project code.

## Generated schemas and compatibility

- `schema/config-v2.schema.json`
- `schema/receipt-v2.schema.json`
- `schema/policy-v2.schema.json`

Regenerate all pinned contracts with `cargo run --locked --example
generate_contract`. The v1 schema/fixture tests require historical v1 output
to remain byte-for-byte stable.
