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

Use `--json` to print the canonical receipt. Raw stdout and stderr remain local
bounded process state and are not emitted by default or stored in the receipt;
only their canonical digest, truncation state, exit status, and duration are
recorded. Use `dry-run` to inspect the exact container argv and reproduce a
failing command deliberately when deeper local diagnostics are required.

Heavy commands use a default-on host-wide single-slot admission queue shared by
independent repositories and cache roots. Override the bounded wait with
`--admission-timeout-seconds <seconds>` on `run` or `benchmark`. Inspect the
bounded operational state with `admission status --json`; it reports only the
schema, busy state, queue count, and opaque ticket identifiers.

For long-running local workflows that should not go through receipt creation,
use `commit-ci-preflight guard exec -- <program> [args...]`. It is shell-free,
inherits the caller environment, keeps stdout/stderr separate, waits for the
same admission slot with a bounded queue timeout, and uses a separate bounded
child runtime timeout. Both guard timeouts default to six hours, are capped at
24 hours, and can be selected independently with
`--admission-timeout-seconds` and `--timeout-seconds`.

## Execution sequence

1. Parse and normalize schema `1.0`.
2. Perform bounded non-heavy setup required by the command: `run` resolves and
   initializes its selected cache, while `benchmark` may perform its optional
   runtime probe.
3. Acquire the host-wide admission slot for `run`, `benchmark`, or `guard exec`,
   immediately before heavy execution, waiting cooperatively with cancellation
   until the selected timeout.
4. On macOS, take a fresh strict `macos-v2` host-memory sample after slot
   acquisition. Denied, malformed, contradictory, timed-out, or uncertain
   samples release the slot and stop without starting heavy work. Linux and
   Windows report resource protection as unsupported and not enforced.
   The swap-only pre-start ceiling is the smaller of 10 GiB and 30% of
   physical RAM, inclusive. Available memory, reclaimable uncompressed memory
   and compressor limits remain independent mandatory gates.
5. For `run`, require a valid 40-hex Git commit and a clean checkout. The configured
   receipt output itself is excluded from this dirty check.
6. For `run`, probe the Docker-compatible runtime with bounded output and deadline.
7. For `run`, acquire the plan-generation workspace lock.
8. For `run`, prepare only declared cache directories and artifact files.
9. For `run`, render shell-free `docker run` argv with a pinned image and explicit limits.
10. For `run`, start the macOS watchdog before local check execution. It samples
    every two seconds, cancels through the existing process supervisor on a
    hard trip or three consecutive soft trips, and joins before slot release.
    `benchmark` has no mid-workload watchdog in this tranche.
11. Execute the `run` checks or benchmark workload with timeout, cancellation, and
    stale-generation guards.
12. Mark cache entries complete only when every check passes.
13. Seal and atomically write the canonical receipt.

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
  client, and only user names declared in `environment.allow`;
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
released. Malformed, foreign, or uncertain coordinator state fails closed and
is never deleted automatically. Admission and resource evidence are not
included in receipts yet. The next tranche must integrate truthful evidence and
host telemetry without changing this tranche's receipt contract.

See [troubleshooting and safe recovery](TROUBLESHOOTING.md) before interpreting
an `active: true` status, diagnosing an absent receipt, or touching any exact
workspace lock reported after a forced stop.

`resource status --json` is read-only. It never reports usernames, absolute
paths, commands, repository names, process inventory, or secrets.

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
