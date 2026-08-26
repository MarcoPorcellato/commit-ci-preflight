# Cross-activity coordination runbook

## Purpose

Commit CI Preflight (CCP) uses one host-wide admission coordinator for heavy
local work. The coordinator is shared by repositories, worktrees, agent
activities, terminals, and users on the same machine. This runbook is the
operational contract for using that shared slot without starting overlapping
OrbStack/Docker workloads or quarantining another activity's state.

The local shell is not the ownership boundary. The CCP admission root and its
OS locks are host-wide. A process list from one activity cannot prove that the
host-wide slot is idle.
A child exit is not a slot-release handoff. A terminal handoff requires the
explicit terminal result plus fresh `admission status --json`, `docker ps -q`,
and `resource status --json` results before another activity proceeds.

## What is coordinated

These commands acquire the same host-wide heavy-work slot:

| Command | Slot | Receipt | Typical use |
|---|---:|---:|---|
| `run` | Yes | Yes | Exact-commit project checks and receipt production |
| `benchmark` | Yes | Yes | Fixed correctness/timing workload |
| `guard exec` | Yes | No | One long explicit launcher, such as an OrbStack CI target |

These command families do not acquire the heavy slot:

| Command family | Notes |
|---|---|
| `plan`, `doctor`, `dry-run`, `verify` | Read-only planning, probing, or receipt verification |
| `resource status`, `resource history` | Read-only host/resource observations |
| `admission status` | Read-only coordinator inspection |
| `cache path`, `cache inventory`, `cache cleanup --dry-run` | Read-only cache inspection |
| `recover status` | Read-only journal inspection |
| `migrate-github-actions` | Parses workflow data as inert input; never executes it |

CCP's own integration tests and any command that invokes `run`, `benchmark`, or
`guard exec` internally must not be wrapped in another `guard exec`. Admission
is intentionally non-reentrant.

### Standard run lock and opt-in cache pins

The host-wide admission slot remains the primary coordination mechanism for
`run`, `benchmark`, and `guard exec`. An opt-in `guard exec` managed-cache pin
is an additional filesystem-use boundary for already-complete cache entries;
it is not a second scheduler, a replacement for admission, or a receipt
qualification signal. Pins use the entry's existing advisory lock and remain
held for the guarded child lifecycle; they have no TTL and are released when
the guarded operation returns.

Only explicitly declared, completed managed-cache sources are pinned; in
particular, `undeclared paths are not pinned`. A path used by a raw launcher
but omitted from the pin arguments receives no CCP ownership or race
protection. Pin acquisition and spawn-boundary revalidation
must succeed before the child starts. A cooperative mutator must use the same
entry lock and revalidate after acquiring it; manual deletion, quarantine,
replacement, or other non-cooperative mutation is outside this guarantee.

Pins do not initialize, repair, delete, quarantine, or publish receipts. A
qualification result remains separate and requires its own exact source,
configuration, runtime, and receipt evidence.

## Required preflight before heavy work

Every activity must perform these checks immediately before reserving a heavy
run. Do not rely on a status copied from an earlier message or terminal:

```console
commit-ci-preflight --version
git status --short --branch
git rev-parse HEAD
commit-ci-preflight resource status --json
commit-ci-preflight admission status --json
docker context show
docker ps -q
```

Proceed only when the source worktree and intended commit are known, resource
status is `decision: admit` on a platform where enforcement is supported,
admission status is readable with `active: false` and `queue_count: 0`, the
Docker-compatible runtime is responsive, and no unaccounted container or owner
is known to be competing for the host. A resource sample is not a reservation:
another activity can acquire the slot immediately afterwards.

## Admission status interpretation

`admission status --json` uses schema `2.0` and reports separate objects for
`slot` and `queue_lock`:

```json
{
  "active": true,
  "queue_count": 0,
  "slot": {
    "kind": "slot_lock",
    "state": "held",
    "owner_run_id": "opaque-ticket-id",
    "acquired_at_unix_seconds": 0,
    "heartbeat_at_unix_seconds": 0,
    "lease_state": "active"
  },
  "queue_lock": {
    "kind": "queue_lock",
    "state": "free",
    "owner_run_id": null,
    "acquired_at_unix_seconds": null,
    "heartbeat_at_unix_seconds": null,
    "lease_state": "not_applicable"
  }
}
```

- `slot_lock` is authoritative for heavy-work ownership;
- `queue_lock` protects queue bookkeeping and does not prove the slot is free;
- `owner_run_id` is an opaque CCP ticket identifier, not a repository, process,
  username, or Codex task identifier;
- heartbeat and lease fields help identify a live or definitely expired lease,
  but never replace the OS advisory lock;
- `unknown`, missing owner metadata, malformed metadata, or a lock/lease
  contradiction is fail-closed and blocking;
- `process_visibility_note` is normative: absence of a process in one local
  shell does not prove global inactivity.

If `active` is true, preserve the exact JSON and contact the activity that owns
the identifier when it is known. Never delete `slot.lock`, `queue.lock`, ticket
files, lease files, counters, ownership markers, or the admission root. CCP may
reclaim a ticket only when its ticket OS lock is demonstrably unlocked and its
valid lease is definitely expired. Manual quarantine is unsupported.

### Planned agent continuation safety boundary

The owner-approved agent continuation mode is opt-in. The opt-in agent
continuation exists solely for orphan prevention. It is not a second scheduler
and does not relax the rules above:
unknown ownership remains blocking, and a missing process in this shell is not
proof that another activity is gone. CCP may release only a ticket whose parent
or session loss is independently verified; ambiguous liveness remains
fail-closed.

A ready activity receives no hidden execution. It must make an explicit claim
and then choose whether to invoke its own command. CCP never revives a
terminated chat, persists a guarded command for later execution, or executes a
command after the activity disappears. The legacy `guard exec` path remains
synchronous and unchanged for terminals and existing official launchers.

## Activity reservation and handoff

Use one owner activity for the complete heavy lifecycle. Record this card before
starting:

```text
CCP heavy reservation
- activity: <agent task or operator label>
- repository/worktree: <logical name and persistent path>
- source commit: <exact SHA>
- command: <run | benchmark | guard exec + launcher name>
- workload family: <stable bounded label, if guard exec>
- expected receipt/output: <exact path or none>
- started after: resource=admit, admission=inactive/queue0, runtime=empty
- owner_run_id: <opaque ID, once acquired>
```

The owner must start exactly one official CCP command, avoid competing
code-intelligence/LSP, Docker, OrbStack, or large-build processes, preserve
output and receipt state,
and report pressure, cancellation, timeout, or cleanup uncertainty immediately.
After termination it must run `admission status --json`, `docker ps -q`, and
`resource status --json`, then send:

```text
CCP heavy slot released
- source commit: <exact SHA>
- command result: <exit code and outcome>
- receipt: <exact path, PASS/PENDING/NOT_RUN>
- inner stages: <summary, if applicable>
- cleanup_status: <verified or uncertain>
- admission after run: active=<...>, queue_count=<...>
- runtime after run: docker ps -q=<empty or bounded finding>
- next owner: <none or named activity>
```

The next activity must wait for this terminal handoff and independently repeat
the preflight. “The other terminal looks idle” is not a handoff. Never report
PASS when an outer guard returned pressure, timeout, internal, or cleanup error;
an inner receipt does not override a failed outer contract unless policy says so.

## Worktrees and repository isolation

A worktree is a Git boundary, not a CCP admission boundary: different
worktrees still share the same host-wide slot.

- create a fresh `codex/*` worktree from the exact current base before editing;
- do not edit `main` or reuse another activity's dirty worktree;
- keep long-lived worktrees and CCP caches outside `/tmp` and `/private/tmp`;
- before cleanup, inspect `git worktree list --porcelain`, branch, HEAD, dirty/
  untracked state, and unique commits;
- never use `git clean`, recursive deletion, or `git worktree prune` as a
  substitute for ownership proof;
- remove only a worktree confirmed clean, merged or superseded, and unowned.

Build cache, admission coordinator, resource history, source worktrees,
Docker/OrbStack images, and receipt/evidence branches have separate lifecycles.
Do not clean them as one group.

## Receipts and GitHub handoff

For a receipt-producing PR:

1. generate the receipt from one exact clean source SHA;
2. verify it with the trusted policy and the same expected SHA;
3. publish only `.ccp/receipt.json` on
   `ccp-evidence/<40-character-source-sha>`;
4. wait for the exact-head GitHub receipt gate;
5. treat `SKIPPED`, `PENDING`, stale-head, or historical evidence as not
   qualified;
6. recheck current head, base, evidence branch, and required status before merge.

An evidence branch belongs to the source SHA, not to the activity that produced
it. Do not force-push it or reuse a receipt after changing source, policy,
image, configuration, or required checks.

## Safe recovery matrix

| Observation | Safe action | Forbidden shortcut |
|---|---|---|
| `resource decision=deny` | Stop heavy work and inspect later | Lower thresholds or bypass CCP |
| `admission active=true` with owner/heartbeat | Wait or coordinate with owner | Kill/quarantine its lock |
| `active=true` with unknown/malformed lease | Preserve evidence and stop | Delete lock/lease files |
| `active=false`, queue 0, Docker empty | Repeat fresh checks, then one owner starts | Assume another shell is impossible |
| Outer guard fails but inner receipt exists | Keep outer result failed/pending | Claim complete PASS from inner receipt |
| Process/container remains after release claim | Owner cleans only exact owned state | Global `docker rm -f` or broad termination |
| Status hangs or returns `unknown` | Do not start heavy work; preserve output | Retry in parallel or delete admission root |

## Privacy and scope

Coordination records contain only bounded operational facts: opaque identifiers,
status, timestamps, lease state, exact source SHA when receipt binding requires
it, and sanitized outcome. Do not copy commands, repository paths, usernames,
environment values, secrets, customer data, raw logs, or container identifiers
into public receipts or shared evidence.

For resource-history fields and OrbStack coverage, see
[resource observation history](RESOURCE_OBSERVATION_HISTORY.md) and
[OrbStack telemetry coverage](ORBSTACK_TELEMETRY_COVERAGE.md). For receipt
publication, see the [GitHub gate](GITHUB_GATE.md).
