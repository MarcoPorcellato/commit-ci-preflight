# T8 Runtime Capability Contract Design

## Purpose

Extend the single-runtime CCP contract so a schema `1.3` attestation proves
more than the requested Docker argv: before execution, it establishes that the
selected Docker-compatible daemon can apply the declared RAM and disabled-swap
mode and that the exact pinned image is already locally available. The result
is still an A0 integrity assertion; it is not a claim about hostile daemons or
remote producer identity.

## Scope

This design covers four linked behaviours:

1. schema `1.3` runtime policy declares `pull_policy = "never"` and
   `swap_mode = "disabled"`;
2. the Docker argv renders `--pull=never` and an explicit
   `--memory-swap=<memory>` limit;
3. a bounded read-only runtime preflight checks daemon capabilities, context,
   and the locally resolved image before workspace or container mutation;
4. snapshot-backed receipt v2 contains privacy-bounded runtime capability
   evidence.

It also adds deterministic property-style tests for hostile delimiter and
control-character mount inputs already rejected by the workspace contract.

## Non-goals

- No Docker Engine API, daemon plugin, new network service, or new dependency.
- No image pull, image build, container start, host-resource admission change,
  cache cleanup, or automatic remediation.
- No hostname, Docker root path, raw `docker info`, raw context value, registry
  credential, command output, or absolute host path in a receipt.
- No claim of native Linux, Windows, or OrbStack qualification without a real
  exact-commit receipt for that host.

## Configuration and compatibility

Schemas `1.0` through `1.2` retain their exact historical runtime contract.
Schema `1.3` builds on `1.2`: it requires the explicit environment and storage
rules already required by strong-plan configurations, plus two runtime fields:

```toml
schema_version = "1.3"

[runtime]
pull_policy = "never"
swap_mode = "disabled"
```

`pull_policy` has one supported value in this tranche. `swap_mode` has one
supported value. Restricting each to a single value avoids an unreviewed matrix
of platform-specific Docker semantics. Both values are normalized plan fields,
so trusted-plan reconstruction rejects a changed policy.

For `swap_mode = "disabled"`, CCP renders Docker's documented relationship:
`--memory-swap` equals the already declared `--memory` value. This makes swap
capacity unavailable to the container instead of inheriting an engine default.

## Preflight protocol

The preflight runs after CCP has selected and capacity-checked its owned cache
root, but before journal creation, source snapshot materialization, workspace
creation, or container lifecycle commands.

1. `docker info --format {{json .}}` remains bounded to five seconds and
   64 KiB. The strict parser requires `MemoryLimit == true` and
   `SwapLimit == true` for schema `1.3`.
2. `docker context show` runs with the same bounded discovery environment.
   CCP accepts a short non-control textual value, computes its canonical
   SHA-256 digest, then discards the literal.
3. `docker image inspect --format {{json .}} <pinned-image>` runs without
   any pull. The parser requires a bounded canonical image ID and a repo-digest
   list containing the exact configured `name@sha256:...` reference.

Any command timeout, parse failure, missing capability, unavailable image, or
mismatch stops the run with a typed non-PASS error. It does not create a
container, mutate the source checkout, or seal a receipt.

## Receipt v2 evidence

Schema `1.3` snapshot-backed receipt v2 adds one optional
`runtime_capability_evidence` object:

```json
{
  "schema_version": "1.0",
  "memory_limit_supported": true,
  "swap_limit_supported": true,
  "context_digest": "sha256:<64 lowercase hex>",
  "resolved_image_id": "sha256:<64 lowercase hex>",
  "resolved_image_reference": "name@sha256:<64 lowercase hex>"
}
```

The object is required by receipt validation only when its embedded normalized
plan uses schema `1.3`; it is forbidden for historical plan schemas. Its image
reference must exactly equal the normalized configured image. It does not make
the daemon trustworthy: it merely binds the successful bounded observations to
the receipt integrity envelope.

## Failure and ordering rules

The CLI performs schema-`1.3` runtime preflight before creating an owned run
journal or source snapshot. The library repeats the preflight before its own
Git/runtime/workspace flow so non-CLI callers cannot bypass it. Both locations
use the same injected `RuntimeCapabilityProbe` contract in tests.

Runtime evidence is captured once and passed forward. It is never recomputed
after checks; a changed or unavailable runtime therefore cannot be masked by a
later successful process result. Existing cancellation, admission release and
resource-watchdog behavior is unchanged.

## Mount grammar hardening

The existing `--mount type=bind,src=...,dst=...` renderer stays shell-free.
Tests cover source and target strings containing commas, equals signs, newline,
NUL, control bytes, path traversal and ambiguous nesting. The result must
either be a canonical explicit binding or a pre-render typed error; no test
permits an ambiguous Docker argument.

## Test strategy

Deterministic unit and contract tests must cover:

- schema `1.3` acceptance, required values, historical rejection and plan
  digest changes;
- exact Docker argv for pull-never and disabled-swap;
- capability, context and image parser success and every rejection class;
- preflight ordering: no journal, Git, snapshot, workspace, Docker create, or
  receipt after a rejected preflight;
- receipt v2 evidence presence, absence, mutation and plan-schema mismatch;
- mount delimiter/control/path property cases and existing deterministic
  fixtures where applicable.

Live qualification remains a later T11 gate. It needs real macOS arm64 with
OrbStack, Linux x86_64 with Docker Engine, and Windows x86_64 evidence; local
unit success is never substituted for any of those claims.

## Acceptance criteria

1. Schema `1.3` cannot silently inherit Docker pull or swap defaults.
2. A passing schema-`1.3` receipt binds a verified daemon capability, opaque
   context digest and exact locally resolved image reference/ID.
3. A missing image, unsupported RAM/swap capability, malformed output or
   unsafe mount cannot start a container or emit PASS evidence.
4. Schemas `1.0`–`1.2`, matrix v2 and their fixtures preserve their historical
   behavior byte-for-byte except for generated optional receipt-schema support.
5. No new dependency, secret, host path, raw Docker metadata or claim of
   unperformed native qualification enters the repository.
