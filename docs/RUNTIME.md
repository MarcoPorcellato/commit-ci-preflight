# Runtime and process contract v1

## Status and scope

This tranche implements the process-supervision boundary, a Docker-compatible
runtime adapter, `doctor`, and `dry-run`. It does **not** execute a project
check, mount a repository, create a container, restore a cache, or emit a PASS
receipt. Those responsibilities begin in later tranches.

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
| Workspace/cache mounts and container execution | Deferred to PR 04/05 |

No macOS test is represented as Windows or Linux evidence.

## Commands

```console
commit-ci-preflight doctor --config .commit-ci-preflight.toml
commit-ci-preflight doctor --config .commit-ci-preflight.toml --json
commit-ci-preflight dry-run --config .commit-ci-preflight.toml
commit-ci-preflight dry-run --config .commit-ci-preflight.toml --json
```

`doctor` validates the configuration, then invokes exactly:

```text
docker info --format {{json .}}
```

The command has a five-second deadline and a 64 KiB output bound. It inherits
only the minimum runtime-discovery environment (`PATH`, platform home fields,
and documented Docker context/config fields). Raw stderr, machine name, and
environment values are never included in the structured probe.

`dry-run` starts no process. It renders a deterministic `docker run` argv with
read-only root filesystem, explicit CPU/memory/PID limits, explicit network
mode, a pinned image digest, and the declared check argv. It labels workspace
mount policy as `deferred_to_pr04` because showing a plausible but unenforced
mount would be misleading.

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
| 5 | Cancelled, stale generation, or deadline exceeded |
| 70 | Internal invariant or uncertain cleanup/result collection |

The CLI prints sanitized classifications. It does not echo raw runtime stderr.

## Security boundary

Container execution is not a complete sandbox against hostile code. This
tranche does not mount the Docker socket into a job, enable privileged mode,
insert a shell, or execute the declared check. Future mount and execution work
must preserve those boundaries and add its own native evidence.
