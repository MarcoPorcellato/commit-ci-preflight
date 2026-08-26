# Configuration contract v1

## Status

`.commit-ci-preflight.toml` schema `1.0` remains supported for legacy planning,
runtime diagnosis, deterministic container argv rendering, and local execution.
Schema `1.1` adds explicit environment classes. Schema `1.2` additionally
requires an explicit storage-capacity policy. Schema `1.3` is an opt-in
extension of the same single-runtime contract: it requires the `1.2` storage
policy plus an explicit no-pull, disabled-swap runtime declaration; none of
these v1 schemas changes the separate v2 matrix schema.
`doctor` performs only the bounded runtime probe described in `docs/RUNTIME.md`;
`run` performs the separately documented execution flow in `docs/LOCAL_RUN.md`.

The generated structural schema is pinned at
[`../schema/config-v1.schema.json`](../schema/config-v1.schema.json). Semantic
rules such as DAG validity, path isolation, limits, and image pinning are
enforced by the Rust validator in addition to that schema.

The parser rejects unknown fields and inputs larger than 1 MiB before TOML
deserialization. Configuration strings are literal: Commit CI Preflight does
not perform shell expansion, environment interpolation, command substitution,
or template evaluation.

For one exact-head qualification across independently pinned runtimes, use the
separate v2 matrix contract in
[`MULTI_RUNTIME_RECEIPTS.md`](MULTI_RUNTIME_RECEIPTS.md). V1 remains the
single-runtime contract described on this page and is not widened implicitly.

## Inspect a plan

```console
commit-ci-preflight plan --config examples/config/rust-project.toml
commit-ci-preflight plan --config examples/config/rust-project.toml --json
```

Both forms only parse, validate, normalize, and display the plan. The JSON form
is canonical and includes a SHA-256 digest of the normalized plan. Use
`commit-ci-preflight dry-run` to inspect non-shell container argv without
starting the runtime.

## Top-level fields

| Field | Requirement |
|---|---|
| `schema_version` | `1.0`; `1.1` for explicit environment classes; `1.2` for explicit storage; or opt-in `1.3` for explicit storage and runtime capability policy |
| `project` | Logical `owner/name`; never a URL or credential-bearing remote |
| `runtime` | Required runtime type, pinned image, and resource limits |
| `receipt` | Optional output/freshness table with fail-safe defaults |
| `environment` | Optional environment contract; `1.0` permits legacy `allow`, while `1.1` uses `fixed`, `runtime_internal`, and `remote_secret_only` |
| `caches` | Up to 32 unique logical caches |
| `storage` | Required by `1.2` and `1.3`; preflight reserve and bounded owned-growth declaration |
| `checks` | 1–128 explicit checks |

## Runtime

`kind` is `docker_compatible` or the reserved `host` adapter. An image is
always pinned as `name@sha256:<64 lowercase hexadecimal digits>`. Tags such as
`latest` are rejected.

Required limits:

- `cpu_count`: 1–256;
- `memory_mib`: 64–262144;
- `pids_limit`: 1–65536;
- `network`: defaults to `false` and must be explicitly enabled.

The current phase validates and enforces these values. `doctor` starts the
Docker CLI only for a read-only `docker info` probe; `dry-run` and `plan` start
no project process.
`dry-run` resolves a persistent cache path and renders the exact repository,
cache, and artifact mount bindings without creating them. No project check or
container is started yet.

### Schema 1.3 runtime policy

Schema `1.3` is opt-in and accepts exactly these two declarations:

```toml
schema_version = "1.3"

[runtime]
pull_policy = "never"
swap_mode = "disabled"
```

The same configuration must include the explicit environment-class and
`[storage]` rules used by schema `1.2`. Historical schemas reject both runtime
policy fields, preserving their normalized plan and argv shape. A `1.3`
`dry-run` renders `--pull never`; with the declared `memory_mib`, it also
renders `--memory-swap <same-memory>m`, so the container does not inherit a
daemon swap default. The declaration changes the plan digest, but it alone is
not runtime capability evidence; the bounded read-only preflight and receipt
binding are subsequent T8-C work.

## Checks

Every check declares:

- a unique stable `id`;
- whether it is `required`;
- an explicit `argv` array with 1–64 non-empty parts;
- a repository-relative `working_directory`;
- `timeout_seconds` between 1 and 86400;
- optional `depends_on` IDs;
- optional repository-relative `artifacts`;
- optional `artifact_contracts` nested tables for artifacts that must become
  receipt evidence.

There is no implicit shell. An operator who explicitly places a shell binary in
`argv[0]` has deliberately selected it; the tool never inserts one.

Dependencies must exist, cannot reference the same check, and must form a DAG.
Normalization uses a stable topological order and sorts independent checks by
ID. Dependency, environment, artifact, and cache ordering is canonical. Thus
semantically equivalent declaration orders produce identical plan bytes and
digest.

## Paths

Logical paths use `/`, are repository-relative, and are already normalized.
Absolute Unix paths, Windows drive paths, backslashes, `~`, empty segments,
`.` segments inside a path, and `..` traversal are rejected. The single value
`.` is allowed only where it means repository root; it cannot be a cache mount,
receipt output, or artifact.

Cache mounts cannot overlap one another, the receipt output, or declared
artifacts. Artifact paths cannot duplicate or nest beneath one another. A
legacy artifact is a regular-file mount; a declared `artifact_contract` may
instead make an already-declared artifact a directory mount. These restrictions
prevent two steps from silently assigning different meanings to the same
writable path.
The repository root is mounted read-only at `/workspace`; only declared cache
and artifact paths receive nested read-write bindings. Host mount paths that
cannot be represented safely by the Docker `--mount` syntax are rejected.
Before a live `run`, every cache destination must already exist as a real
directory in the repository. A regular-file artifact destination must be a real
file and a directory artifact destination must be a real directory. They may
be ignored generated placeholders, but must not be symlinks. This is required
by nested Docker bindings beneath a read-only repository mount; the runner
fails before Docker rather than mutating the source checkout to create them.

### Artifact contracts

An artifact contract is optional only for legacy execution. If declared, it is
normative plan data and a snapshot-backed receipt v2 MUST contain exactly one
observed manifest record for it. The record is created only from the
CCP-owned writable mount after checks complete; it never reads the operator's
source checkout.

```toml
artifacts = ["results/report.json"]

[[checks.artifact_contracts]]
path = "results/report.json" # Must also appear in `artifacts` for this check.
kind = "regular-file"         # `regular-file` or `directory`.
max_bytes = 1048576           # 1 byte through 1 GiB.
max_entries = 1               # Exactly 1 for a regular file; 1 through 10000 for a directory.
```

`path` is producer-bound to its containing check. The observer rejects a
missing output, symlink, special object, pre-existing symlinked ancestor,
concurrent replacement, byte-limit breach, or directory-entry-limit breach.
It computes a canonical manifest digest from bounded final-state entries. It
does not copy artifact contents into the receipt or expose a host path.

See [`CACHE_AND_WORKSPACE.md`](CACHE_AND_WORKSPACE.md) for cache-root
precedence, ownership, persistence, inventory, and cleanup rules.

### Storage-capacity policy

Schemas `1.2` and `1.3` require one explicit policy. It is normalized into the plan and
therefore changes the plan digest and any snapshot-backed receipt v2.

```toml
schema_version = "1.2" # or "1.3" with the explicit runtime policy above

[storage]
min_free_bytes = 1073741824          # Keep at least 1 GiB free after the declared run allowance.
receipt_journal_reserve_bytes = 1048576 # Reserve 1 MiB for CCP receipt and journal writes.
max_cache_growth_bytes = 2147483648  # Bound new CCP-managed cache growth for this run.
```

The policy does not request deletion or make a prediction about arbitrary
project writes. Before Git inspection, runtime probing, workspace creation, or
container execution, CCP calculates the required free capacity as:

`min_free_bytes + receipt_journal_reserve_bytes + max_cache_growth_bytes + sum(artifact_contract.max_bytes)`.

The probe is performed only against the already-owned CCP cache root. A probe
failure, an arithmetic overflow, or insufficient free capacity fails closed;
CCP does not evict, delete, or inspect unowned files. The dynamic free-space
sample is deliberately not included in a receipt: it is host-specific and is
not execution evidence. Schema `1.0` and `1.1` reject a `[storage]` table so
they retain their historical contract unchanged.

## Environment classes and privacy

Schema `1.0` retains `environment.allow` as a legacy host-inheritance
allowlist. It is explicit but cannot make a complete attestable-environment
claim.

Schema `1.1` requires an explicit class for every non-runtime-discovery value:

| Class | Declaration | Local runtime behaviour | Evidence boundary |
| --- | --- | --- | --- |
| `fixed` | Non-secret literal | Injects the exact reviewed value only after its canonical digest matches the normalized plan | Value is never serialized or printed by `plan`/`dry-run` |
| `runtime_internal` | Variable name plus declared cache ID | Derives `/workspace/<cache mount>` without reading the host environment | Cache ID and derived container target are normative plan fields |
| `remote_secret_only` | Secret name | Rejects local receipt creation before admission | No secret value is read or stored locally |

`runtime_internal` is intentionally limited to declared managed caches in this
release. Arbitrary host paths, undeclared fixed values, and changed fixed values
fail closed. A plan stores a canonical digest of each fixed value, never the
literal. The private parser envelope carries the literal only until the runtime
checks that digest immediately before rendering the process environment.

## Plan digest

The digest covers the normalized schema version, project, runtime, receipt
policy, environment names, caches, schema-`1.2` storage policy, ordered checks,
and artifact contracts. It uses the same CCP canonical JSON v1 profile
described in `docs/RECEIPT_SPEC.md`.

The digest is integrity evidence, not a signature or identity attestation.
## Matrix V2 compatibility profile

The `matrix-v2-legacy-v1` profile is Matrix-only compatibility for the legacy
producer suffix. It does not widen the schema or infer policy. Review the plan
and copy its digests into Matrix policy v2; never copy them from a completed
receipt. Use the same command parity sequence for each selected source:

```console
commit-ci-preflight plan --matrix-plan-profile matrix-v2-legacy-v1 --json
commit-ci-preflight doctor --matrix-plan-profile matrix-v2-legacy-v1 --json
commit-ci-preflight dry-run --matrix-plan-profile matrix-v2-legacy-v1 --json
commit-ci-preflight run --matrix-plan-profile matrix-v2-legacy-v1 --generation N --json
```

Legacy and current cache identities are separate. `verify` has no profile flag.
An old trusted historical verifier must accept the exact receipt before policy
migration; this is evidence acceptance, not a general trust claim.
