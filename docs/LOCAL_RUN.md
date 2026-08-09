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

## Execution sequence

1. Parse and normalize schema `1.0`.
2. Require a valid 40-hex Git commit and a clean checkout. The configured
   receipt output itself is excluded from this dirty check.
3. Probe the Docker-compatible runtime with bounded output and deadline.
4. Resolve and validate the persistent owned cache root.
5. Acquire the plan-generation workspace lock.
6. Prepare only declared cache directories and artifact files.
7. Render shell-free `docker run` argv with a pinned image and explicit limits.
8. Execute checks in deterministic DAG order with timeout, cancellation, and
   stale-generation guards.
9. Mark cache entries complete only when every check passes.
10. Seal and atomically write the canonical receipt.

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
