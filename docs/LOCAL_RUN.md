# Local run and receipt workflow v1

## Scope

PR 05 connects the validated configuration, process supervisor, Docker-
compatible runtime, managed workspace, cache, and receipt contracts. It runs
only explicit argv from a clean Git checkout and never inserts a shell.

```console
commit-ci-preflight run \
  --config .commit-ci-preflight.toml \
  --repository . \
  --generation 1
```

The repository configuration uses schema `1.1` runtime-internal bindings for
the three Cargo/Rustup cache variables. CCP derives their container values from
the reviewed managed-cache mounts, so the operator must not export host paths
before the run. `dry-run --json` reports names and mounts, never fixed or secret
values.

Use `--json` to print the canonical receipt. Raw stdout and stderr remain local
bounded process state and are not emitted by default or stored in the receipt;
only their canonical digest, truncation state, exit status, and duration are
recorded. Use `dry-run` to inspect the exact container argv and reproduce a
failing command deliberately when deeper local diagnostics are required.

Heavy commands use a default-on host-wide single-slot admission queue shared by
independent repositories, worktrees, agent activities, and cache roots. Override
the bounded wait with `--admission-timeout-seconds <seconds>` on `run` or
`benchmark`. Inspect the bounded operational state with
`admission status --json`; schema `2.0` distinguishes `slot_lock` from
`queue_lock` and reports the slot's opaque owner/run identifier, acquisition time,
heartbeat, and lease state. Missing, malformed, legacy, or contradictory lease
metadata is `unknown` and remains blocking. The status explicitly warns that
absence of a process in the local shell does not prove global inactivity. Use
the [cross-activity coordination runbook](COORDINATION_RUNBOOK.md) before
starting or handing off a heavy command. Status accepts
`--timeout-seconds <seconds>` and fails closed if it cannot obtain a coherent
snapshot within that bound.

For long-running local workflows that should not go through receipt creation,
use `commit-ci-preflight guard exec -- <program> [args...]`. It is shell-free,
inherits the caller environment and current working directory, keeps
stdout/stderr separate, waits for the same admission slot with a bounded queue
timeout, and uses a separate bounded
child runtime timeout. Both guard timeouts default to six hours, are capped at
24 hours, and can be selected independently with
`--admission-timeout-seconds` and `--timeout-seconds`.

## Execution sequence

1. Parse and normalize schema `1.0`, the explicit-environment schema `1.1`,
   schema `1.2` with its explicit storage-capacity policy, or opt-in schema
   `1.3` with its explicit no-pull and disabled-swap policy.
2. Perform bounded non-heavy setup required by the command: `run` resolves and
   initializes its selected cache, while `benchmark` may perform its optional
   runtime probe.
3. For schema `1.2` `run`, fail closed if the selected CCP-owned cache-root
   filesystem cannot retain the declared free-space reserve after receipt/journal,
   maximum cache-growth, and declared artifact allowances. This check never
   deletes data and does not expose the host free-space sample in a receipt.
4. Acquire the host-wide admission slot for `run`, `benchmark`, or `guard exec`,
   immediately before heavy execution, waiting cooperatively with cancellation
   until the selected timeout.
5. On macOS, take a fresh strict `macos-v4` host-memory sample after slot
   acquisition. Denied, malformed, contradictory, timed-out, or uncertain
   samples release the slot and stop without starting heavy work. Linux and
   Windows report resource protection as unsupported and not enforced.
   Admission requires at least 20% available memory, 3 GiB reclaimable
   uncompressed memory, and swap no higher than the smaller of 8 GiB and 30%
   of physical RAM. Boundaries are inclusive and independently mandatory.
   Compression alone is advisory; it denies admission only at 70% or more
   together with another pressure signal.
6. For `run`, require a valid 40-hex Git commit and a clean checkout. The configured
   receipt output itself is excluded from this dirty check.
7. For schema `1.3` `run`, before any journal, source snapshot, workspace, or
   container mutation, invoke exactly `docker info --format {{json .}}`,
   `docker context show`, and `docker image inspect --format {{json .}}
   <configured-pinned-image>`. Each has the bounded discovery environment,
   five-second deadline, and 64 KiB capture limit. Require memory/swap
   capability and exact local image resolution; digest the context then discard
   its literal. A failure stops with no receipt.
8. For historical `run`, probe the Docker-compatible runtime with bounded output
   and deadline at the established historical point.
9. For `run`, acquire the plan-generation workspace lock.
10. For `run`, prepare only declared cache directories and artifact files.
11. For `run`, render shell-free `docker run` argv with a pinned image and explicit limits.
12. For `run`, start the macOS watchdog before local check execution. It samples
    every two seconds, cancels through the existing process supervisor on a
    hard trip or 15 consecutive compound soft samples (about 30 seconds), and
    joins before slot release. Compressor occupancy alone never cancels an
    otherwise healthy in-progress run.
    `benchmark` has no mid-workload watchdog in this tranche.
    The watchdog joins before admission release, and admission release is
    attempted exactly once. A release failure overrides the primary result.
13. Execute the `run` checks or benchmark workload with timeout, cancellation, and
    stale-generation guards.
14. Mark cache entries complete only when every check passes.
15. Seal and atomically write the canonical receipt. For a schema `1.3`
    snapshot-backed receipt v2, sealing requires the single captured runtime
    capability evidence. It contains only two booleans, a context digest, image
    ID, and exact configured image reference; no raw daemon/context output or
    host identity is retained. Historical plans forbid this evidence.

For `guard exec` on macOS, pass `--resource-profile <class>` and a stable
`--resource-workload-family <cohort>` to classify an admitted workload without
exposing repository or command identity. Official indirect launchers should
also declare executor, cache state, execution mode, target platform and known
requested CPU/memory limits. The watchdog summarizes baseline and extrema into
the persistent local file
`~/Library/Application Support/commit-ci-preflight/resource-history-v2.jsonl`.
At most 500 records are retained. History is observation-only, never enters a
receipt, never changes a policy decision and can be disabled with
`--no-resource-history`. See
[RESOURCE_OBSERVATION_HISTORY.md](RESOURCE_OBSERVATION_HISTORY.md) for the
schema, privacy boundary, cleanup and forecast gate.
Tests may isolate storage with an absolute `CCP_RESOURCE_HISTORY_DIR`; this
changes only the history location.
Admission is intentionally non-reentrant. Do not use `guard exec` around a
workflow that invokes CCP guarded commands internally; split the outer workflow
or run CCP's own integration suite directly after a successful resource probe.
Only commands supervised by `guard exec` are covered. A direct Docker or
OrbStack invocation that bypasses CCP remains outside this telemetry contract.

A command failure or timeout is `FAIL`. A dependency skip, cancellation before
execution, or uncertain runtime execution is `NOT_RUN` and makes required
evidence `PENDING`. Cleanup uncertainty is an internal error and can never be
converted into PASS.

## Mount and runtime boundary

- repository: `/workspace`, read-only;
- declared cache paths: nested read-write bindings under `/workspace`;
- declared artifact files: nested read-write bindings under `/workspace`;
- temporary files: private 64 MiB `/tmp` tmpfs with `noexec,nosuid,nodev`;
- container root: read-only;
- network: `none` unless explicitly enabled;
- environment: fixed `TMPDIR=/tmp`, runtime-discovery fields for the Docker
  client, and only values admitted by the normalized environment contract;
- no Docker socket mount, privileged mode, host networking, or implicit shell.

This is strong containment for trusted project checks, not a security sandbox
for hostile code.

## Cache and interrupted runs

Cache contents are mutable acceleration state and are not attested. Completion
markers are written atomically only after all checks pass. Interrupted marker
writes are cleaned up best-effort and never make an entry complete.

The workspace lock is `.run-lock-v1`. Normal drop removes it. If the process is
forcibly killed, a later run reports the exact lock path and stops. Verify that
no runner uses that cache root before manually removing only the reported lock
file. There is intentionally no automatic stale-lock deletion.

Admission tickets are different: each ticket is protected by its own advisory
lock, so a crashed or rebooted waiter becomes reclaimable when that lock is
released and its valid lease is definitely expired. Malformed, foreign, missing,
legacy, or uncertain coordinator state fails closed and is never deleted
automatically. Admission and resource evidence are not included in receipts
yet. The complete owner/lease handoff and recovery procedure is in the
[cross-activity coordination runbook](COORDINATION_RUNBOOK.md).

See [troubleshooting and safe recovery](TROUBLESHOOTING.md) before interpreting
an `active: true` status, diagnosing an absent receipt, or touching any exact
workspace lock reported after a forced stop.

`resource status --json` is read-only. It never reports usernames, absolute
paths, commands, repository names, process inventory, or secrets.
`resource history --json` is likewise read-only and returns only the strict
privacy-minimized v2 records already written by admitted `guard exec` runs.

## Local evidence captured on 2026-08-09

The following clean-room fixtures ran on Apple Silicon macOS through OrbStack
29.4.0. Each used the official image digest committed in its sample config.

| Fixture | Command | Result |
|---|---|---|
| Rust | `cargo test --locked` | PASS |
| Python | `python -m unittest discover -v` | PASS |
| Node | `node --test` | PASS |
| Rust cache replay | second run with pre-existing ignored receipt | PASS |

The first Rust qualification correctly failed when no writable `target` cache
was declared. A second diagnostic correctly failed when the read-only container
had no writable temporary directory. The final contract added the declared
`target` cache and bounded private `/tmp`; it did not make the repository
writable.

These are macOS-hosted OrbStack results. They are not Windows-native,
Linux-host-native, hosted-runner, identity-attestation, or GitHub policy
evidence. PR 06 adds an independent verifier; later plan tranches add the small
remote gate and cross-platform qualification.

For a cooperative child using a completed CCP cache entry, declare one
`--managed-cache-root` and repeatable `--managed-cache-source` options:

```console
commit-ci-preflight guard exec \
  --managed-cache-root /absolute/owned/cache-root \
  --managed-cache-source /absolute/owned/cache-root/entries/sha256-<64>/data \
  -- <program> [args...]
```

The source must be the exact completed `entries/sha256-<64>/data` directory
under that root. Sources must already be canonical; aliases are rejected.
Accepted declarations are deduplicated and locked in a stable order, then
revalidated immediately before child spawn. The
existing advisory entry lock stays held through child cleanup and guard-session
release. This declaration is cooperative and non-attesting: undeclared paths
are not pinned, and manual or non-cooperative replacement is unsupported. The
flags do not initialize, repair, delete, quarantine, publish, or attest cache
contents. Legacy `guard exec` without these flags remains unchanged.
