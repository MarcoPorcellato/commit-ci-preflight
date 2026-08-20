# Runtime and process contract v1

## Status and scope

This contract implements the process-supervision boundary, a Docker-compatible
runtime adapter, `doctor`, `dry-run`, and the PR 05 local execution path. Live
runs mount the repository read-only, expose only declared writable paths, and
emit a canonical receipt after supervised completion.

The implemented evidence is:

| Capability | Current status |
|---|---|
| Pure runtime trait and structured process result | Implemented and unit-tested |
| Docker-compatible read-only capability probe | Implemented |
| OrbStack identification from bounded Docker metadata | Implemented and locally exercised |
| Explicit non-shell container argv rendering | Implemented and deterministic |
| Unix process-group timeout and descendant cleanup | PASS on macOS arm64 |
| Windows Job Object compilation and cleanup | PENDING genuine Windows-native execution |
| Linux-native cleanup behavior | PENDING genuine Linux-native execution |
| Workspace/cache mount contract | Implemented and deterministic |
| Live container execution | PASS on macOS arm64 with OrbStack 29.4.0 |
| Rust, Python, and Node clean-room fixtures | PASS on macOS arm64 |

No macOS test is represented as Windows or Linux evidence.

## Commands

```console
commit-ci-preflight doctor --config .commit-ci-preflight.toml
commit-ci-preflight doctor --config .commit-ci-preflight.toml --json
commit-ci-preflight dry-run --config .commit-ci-preflight.toml
commit-ci-preflight dry-run --config .commit-ci-preflight.toml --json
commit-ci-preflight run --config .commit-ci-preflight.toml --repository .
```

`doctor` validates the configuration, then invokes exactly:

```text
docker info --format {{json .}}
```

The command has a five-second deadline and a 64 KiB output bound. It inherits
only the minimum runtime-discovery environment (`PATH`, platform home fields,
and documented Docker context/config fields). Raw stderr, machine name, and
environment values are never included in the structured probe.

`dry-run` starts no process and creates no cache directory. It renders a
deterministic `docker run` argv with a read-only container root filesystem,
explicit CPU/memory/PID limits, explicit network mode, a pinned image digest,
and the declared check argv. The repository binding is read-only. Only declared
cache and artifact bindings are read-write, and every writable source is
contained by the resolved managed cache root. The output labels this policy
`explicit_bindings`.

For opt-in configuration schema `1.3`, the rendered argv also contains
`--pull never` and `--memory-swap <memory_mib>m`. The latter equals the
declared `--memory` value and expresses disabled container swap; schemas
`1.0`–`1.2` retain their historical argv unchanged. Rendering does not itself
claim that the selected daemon supports those controls or that the pinned image
is locally available; later T8-C preflight and receipt work must establish that
evidence before a run starts.

Live runs preserve that argv. They add a private, non-host-backed tmpfs at
`/tmp`, bounded to 64 MiB with `noexec`, `nosuid`, and `nodev`, and set the
fixed container value `TMPDIR=/tmp`. This permits compiler temporary files
without making the repository or container root writable.

Before creating a managed run workspace, `run` checks the repository-side
destination of every nested writable binding. Cache destinations must already
be real directories and artifact destinations real files; missing paths,
symlinks, wrong object types, and paths resolving outside the repository fail
closed before Docker starts. The runner does not modify source merely to make
a container mount possible.

Absolute host paths are shown only in local dry-run output because an operator
must be able to audit the exact mount. They must not be copied into receipts or
telemetry. See [`CACHE_AND_WORKSPACE.md`](CACHE_AND_WORKSPACE.md).

## Lifecycle and fail-closed rules

Each process request carries a run identity containing project, optional commit,
configuration digest, and generation. The supervisor accepts completion only
while that exact identity remains current.

1. Validate the request before spawn.
2. Reject a stale generation or pre-existing cancellation before spawn.
3. Spawn with cleared environment and bounded concurrent stdout/stderr readers.
4. Poll for completion, cancellation, stale generation, or timeout.
5. On cancellation/timeout, request graceful termination where supported.
6. After the grace period, force-stop the complete containment unit.
7. Verify descendant cleanup and collect bounded output.
8. Recheck generation before accepting a completed result.

Uncertain process monitoring, output collection, timeout handling, or cleanup
returns an error. A later receipt layer must never convert such an error into
PASS.

## Platform containment

On Unix, each child becomes the leader of a new process group. Graceful stop
sends SIGTERM to the group; forced cleanup sends SIGKILL and checks that the
group no longer exists. A native macOS integration test starts a real
descendant and verifies its removal after timeout.

On Windows, `process-wrap` creates the child suspended, assigns it to a Job
Object, and resumes it. Forced stop terminates the Job Object. This source path
is deliberately documented as PENDING until a real Windows runner proves the
compiled behavior and descendant cleanup.

## Cancellation and stale generations

The CLI installs one Ctrl+C/termination handler that flips a thread-safe token.
The polling supervisor observes that token and performs the same verified
cleanup path used by a timeout. Changing the active run identity invalidates an
older result; the older process is cleaned up before `StaleGeneration` is
returned.

This guards in-process replacement. Crash recovery and cleanup of processes
left by a forcibly terminated supervisor require a later ownership/journal
contract and are not claimed here.

## Stable failure classes

| Exit code | Meaning in this tranche |
|---:|---|
| 2 | Invalid configuration or CLI usage |
| 4 | Runtime unavailable, unsupported, or invalid probe |
| 1 | One or more required checks completed with FAIL |
| 5 | PENDING evidence, cancellation, stale generation, or deadline exceeded |
| 70 | Internal invariant or uncertain cleanup/result collection |

The CLI prints sanitized classifications. It does not echo raw runtime stderr.

## Security boundary

Container execution is not a complete sandbox against hostile code. The live
runner does not mount the Docker socket into a job, enable privileged mode,
insert a shell, or expose undeclared host paths. Network remains disabled unless
the configuration explicitly enables it. See `docs/LOCAL_RUN.md` for evidence
and remaining platform limitations.
