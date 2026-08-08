# Configuration contract v1

## Status

`.commit-ci-preflight.toml` schema `1.0` is implemented for read-only planning.
No command execution is active in this phase.

The generated structural schema is pinned at
[`../schema/config-v1.schema.json`](../schema/config-v1.schema.json). Semantic
rules such as DAG validity, path isolation, limits, and image pinning are
enforced by the Rust validator in addition to that schema.

The parser rejects unknown fields and inputs larger than 1 MiB before TOML
deserialization. Configuration strings are literal: Commit CI Preflight does
not perform shell expansion, environment interpolation, command substitution,
or template evaluation.

## Inspect a plan

```console
commit-ci-preflight plan --config examples/config/rust-project.toml
commit-ci-preflight plan --config examples/config/rust-project.toml --json
```

Both forms only parse, validate, normalize, and display the plan. The JSON form
is canonical and includes a SHA-256 digest of the normalized plan.

## Top-level fields

| Field | Requirement |
|---|---|
| `schema_version` | Exactly `1.0` |
| `project` | Logical `owner/name`; never a URL or credential-bearing remote |
| `runtime` | Required runtime type, pinned image, and resource limits |
| `receipt` | Optional output/freshness table with fail-safe defaults |
| `environment` | Optional allowlist of variable names only |
| `caches` | Up to 32 unique logical caches |
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

The current phase validates these values but does not start a runtime.

## Checks

Every check declares:

- a unique stable `id`;
- whether it is `required`;
- an explicit `argv` array with 1–64 non-empty parts;
- a repository-relative `working_directory`;
- `timeout_seconds` between 1 and 86400;
- optional `depends_on` IDs;
- optional repository-relative `artifacts`.

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
artifacts. Artifact paths are globally unique. These restrictions prevent two
steps from silently assigning different meanings to the same writable path.

## Environment privacy

Only valid environment-variable names are stored in a plan. Values are never
read by `ccp plan` and never serialized. A later execution phase will copy only
explicitly allowed names into a bounded runtime environment while receipts
continue to exclude values.

## Plan digest

The digest covers the normalized schema version, project, runtime, receipt
policy, environment names, caches, and ordered checks. It uses the same CCP
canonical JSON v1 profile described in `docs/RECEIPT_SPEC.md`.

The digest is integrity evidence, not a signature or identity attestation.
