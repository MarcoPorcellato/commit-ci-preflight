# Troubleshooting and safe recovery

## Start with bounded evidence

Record these outputs before changing state:

```console
commit-ci-preflight --version
git status --short --branch
git rev-parse HEAD
commit-ci-preflight admission status --json
commit-ci-preflight resource status --json
commit-ci-preflight cache path
commit-ci-preflight cache inventory --json
```

Do not publish secrets, environment values, raw project logs, personal paths,
or proprietary source when asking for help.

## Exit-code map

| Code | Meaning | First response |
|---:|---|---|
| `0` | Command-specific PASS | Continue to the next gate |
| `1` | One or more required project checks failed | Reproduce the explicit check locally |
| `2` | Invalid CLI, configuration, policy, or benchmark input | Correct the reviewed input; do not retry blindly |
| `3` | Receipt/benchmark verification rejected | Inspect bounded findings and exact commit/policy |
| `4` | Runtime unavailable or migration blocked | Check runtime capability or unsupported workflow features |
| `5` | PENDING, cancelled, stale generation, or admission timeout | Establish whether work was superseded or the queue is busy |
| `6` | `guard exec` resource guard rejection/pressure | Let host pressure subside; do not bypass the guard |
| `70` | Internal invariant, unsafe state, or uncertain cleanup | Stop; preserve evidence and inspect exact state |
| `124` | `guard exec` child runtime timeout | Inspect the child workflow and timeout budget |
| `130` | `guard exec` user cancellation | Confirm descendants stopped before retrying |

For `guard exec`, a child exit code from `1` through `255` is propagated when
the child completed normally. Code `70` is used instead if that result or
cleanup is uncertain.

## `admission status` says `active: true`

`active` reports that the platform-wide admission slot lock was busy at the
instant sampled. It is not a durable statement that a named process still
exists, and the privacy-bounded status intentionally omits process inventory,
commands, repository names, usernames, and absolute paths. Schema `2.0` now
separates `slot_lock` from `queue_lock` and exposes an opaque owner/run ID,
acquisition time, heartbeat, and lease state. These fields coordinate
activities; they do not turn a lease into the authority that the OS lock
already provides.

Safe diagnosis:

1. preserve the complete status JSON, including `owner_run_id`, heartbeat, and
   lease state;
2. check the terminal or orchestrator that launched the heavy command;
3. inspect the relevant Docker/OrbStack container list using the runtime's
   normal operator tooling;
4. poll `admission status --json` again after the suspected command exits;
5. confirm no `commit-ci-preflight run`, `benchmark`, or `guard exec` process
   owned by you is still executing;
6. retry the intended guarded command with a bounded admission timeout only
   after a fresh handoff and preflight.

Admission tickets use advisory locks. A crashed waiter or reboot releases its
lock, and a later coordinator pass reclaims that certainly stale ticket only
when the valid lease is definitely expired. Do not delete ticket files, lock
files, lease files, counters, ownership markers, or the admission root by hand
merely because one status snapshot showed `active: true` or because your local
shell cannot see a process. The absence of a process in one activity does not
prove global inactivity.

If repeated fresh status calls report active while no owned process/container
can be found, preserve the outputs and report the exact CCP version and host
platform. If CCP reports a foreign, malformed, unsafe, or uncertain coordinator
layout, it intentionally fails closed with code `70`; manual deletion is not a
supported recovery procedure.
Use the [cross-activity coordination runbook](COORDINATION_RUNBOOK.md) for the
owner handoff and safe-recovery matrix.

### Unsafe admission layout

Route only the exact `UnsafeLayout(.../agent-tickets)` case to the read-only
recovery status command:

```console
commit-ci-preflight admission layout-recovery status --json --timeout-seconds 5
```

If the result is eligible, preserve its lowercase `plan_sha256` and obtain
separate authorization for one exact hash-bound apply. Apply preserves the
deterministic quarantine entry; it does not authorize a later CCP run, Docker,
receipt/publication, or R5. A later heavy run needs fresh exact-head,
hash-bound authorization. Manual deletion, moving, or quarantine is not a
supported cleanup recipe.

`recovery_uncertain` is a hard stop and exit code `70`: preserve both paths and
the JSON, do not retry apply, and do not start a heavy run. `not_applied` means
no recovery occurred. Any non-empty, foreign, malformed, active, lock-timeout,
plan-mismatch, or unknown-child result remains code `70` and operator-required.

## Run ended but no receipt exists

A receipt is the final product of successful orchestration, not a start marker.
No receipt is expected when failure occurs before sealing, including:

- dirty or non-commit repository state;
- invalid configuration or missing writable mount placeholders;
- admission or resource rejection;
- runtime probe/spawn failure;
- required check failure, timeout, cancellation, or stale generation;
- uncertain descendant cleanup or output collection;
- invalid or occupied receipt destination.

Use the command exit code and bounded terminal classification. Then run:

```console
commit-ci-preflight plan --config .commit-ci-preflight.toml --json
commit-ci-preflight doctor --config .commit-ci-preflight.toml --json
commit-ci-preflight dry-run --config .commit-ci-preflight.toml --repository . --json
```

Reproduce only the failing explicit check in a deliberate diagnostic context.
Do not synthesize a receipt or reinterpret an absent receipt as PASS.

## First Rust check fails only inside the read-only container

The repository's own `.commit-ci-preflight.toml` uses schema `1.1` and derives
`CARGO_HOME`, `CARGO_TARGET_DIR`, and `RUSTUP_HOME` from declared managed-cache
mounts. Do not export host paths for these variables. First confirm the exact
normalized bindings without starting Docker:

```console
commit-ci-preflight plan --config .commit-ci-preflight.toml --json
commit-ci-preflight dry-run --config .commit-ci-preflight.toml --repository . --json
```

If a check still exits `1`, preserve the failed receipt if it exists, confirm
the cache destinations and image are the expected reviewed values, then start a
new monotonically higher generation only after correcting the configuration or
cache state. Do not make the repository mount writable or bypass the
environment contract with host exports.

## Workspace lock after a forced stop

The run workspace lock and the admission queue are different mechanisms. If a
process is forcibly killed, a later run may report an exact `.run-lock-v1`
path. CCP does not auto-delete this lock because it cannot prove another runner
is absent.

Before removing only the reported lock file, verify that no CCP process or
container uses that exact cache/workspace and preserve any uncertain work. Do
not recursively delete the cache root. If ownership is unclear, choose a new
explicit persistent cache root and retain the old one for review.

## macOS resource guard denies a run

The `macos-v4` guard evaluates swap, available/reclaimable memory, compressor
pressure, and sample certainty. Admission requires at least 20% available
memory and 3 GiB reclaimable memory, and permits swap through the smaller of
8 GiB and 30% of physical RAM. Those three limits are independently mandatory.
Compression alone is advisory; even extreme compression denies only when it
accompanies another pressure signal.

```console
commit-ci-preflight resource status --json
```

The in-run watchdog intentionally differs from admission. Compression alone is
not evidence that a running workload is unsafe. Soft cancellation requires at
least two signals among low available memory, low reclaimable memory, at least
55% compression, at least 4 GiB swap, or at least 1 GiB swap growth across the
30-second trend window. The compound condition must persist for 15 samples.
Immediate cancellation remains for critically low available/reclaimable
memory, 8 GiB swap, or at least 70% compression accompanied by another pressure
signal.

Close or finish memory-heavy work and retry later. Do not modify coordinator
files, disable the guard, or infer that a machine with free swap is safe. Linux
and Windows currently report `unsupported_not_enforced`; that is not a PASS for
equivalent host protection.

## Runtime or image failure

If `doctor` returns code `4`:

- confirm the Docker-compatible engine is running;
- inspect the selected Docker context and ordinary runtime connectivity;
- verify the pinned image digest exists for the required architecture;
- confirm network policy is truthful for first-time dependency/image fetches;
- rerun `doctor`, then `dry-run`, before `run`.

Do not replace an unavailable digest with a mutable tag. A macOS OrbStack PASS
does not establish Windows-native or Linux-host-native qualification.

## Configuration digest changed

Any normalized execution-plan change can change the digest, including command
order after dependency normalization, image, limits, caches, artifacts,
environment names, receipt policy, or network mode.

Review `plan --json`. If the change is intentional, update the repository
policy in a separate reviewed change and generate a fresh receipt for the exact
source commit. Never weaken digest comparison or reuse an older receipt.

## GitHub gate is red

Check in this order:

1. the PR's latest head SHA;
2. existence of `ccp-evidence/<head-sha>` in the base repository;
3. exact `.ccp/receipt.json` path and regular-file type;
4. receipt commit, digest, freshness, required checks, image, and platform;
5. policy from the target repository's trusted base;
6. pinned CCP source and trusted verifier build;
7. whether the PR is from a fork and therefore needs maintainer reproduction.

Do not rerun project code under `pull_request_target`. Do not give the evidence
checkout executable authority. The gate should remain small, fail closed, and
publish status on the exact PR head.

## Cache disk use or cleanup

Inventory first:

```console
commit-ci-preflight cache path
commit-ci-preflight cache inventory --json
commit-ci-preflight cache cleanup --dry-run --json
```

Cleanup is preview-only in the current prerelease. Do not recursively remove a
cache root until its exact path, CCP ownership marker, active-run state,
retention need, and rollback impact have been reviewed. Temporary worktrees,
container layers, and CCP managed caches have separate ownership and cleanup
lifecycles.

## Reporting a reproducible problem

Include:

- CCP version and exact CCP source commit;
- host OS/architecture and runtime flavor/version;
- sanitized configuration and policy digests;
- exact source commit (for public repositories only);
- command and exit code;
- bounded JSON status/report with sensitive fields removed;
- whether a receipt exists and whether independent verification ran;
- whether the behavior reproduces with a new persistent cache root.

Keep private source, raw logs, credentials, tokens, environment values, and
personal paths out of public issues. Security findings belong in the private
channel described by [SECURITY.md](../SECURITY.md).
