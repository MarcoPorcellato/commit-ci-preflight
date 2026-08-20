# Coding-harness integration contract

## Scope

This is the vendor-neutral CCP contract for a coding harness, whether it is a
desktop application, CLI, IDE extension, plugin host, or delegated activity.
It does not install a plugin, edit a user profile, create a hook, or start a
container. The [compatibility matrix](COMPATIBILITY_MATRIX.md) records the
evidence level for every reference page.

## Read-only preflight

Before any mutation or heavy command, establish the worktree, exact-head, and
host facts with read-only commands:

```console
commit-ci-preflight --version
git status --short --branch
git rev-parse HEAD
commit-ci-preflight resource status --json
commit-ci-preflight admission status --json
docker context show
docker ps -q
```

Proceed only when the source SHA and worktree state are known, resource status
is a coherent `admit`, admission is readable with no active owner and no queue,
and the configured runtime is responsive. An absent process in one shell does
not prove that a host-wide CCP slot is free.

## Heavy-work ownership

One activity owns one heavy lifecycle. Worktrees isolate Git state but do not
isolate CCP admission. Slot ownership is distinct from queue bookkeeping.
Unknown, missing, legacy, malformed, or contradictory lease state is blocking.
No activity may delete, quarantine, rewrite, or guess about admission locks,
leases, tickets, counters, or coordinator data.

The owner must issue a terminal handoff containing the exact source SHA,
command result, outer guard outcome, receipt state, cleanup state, post-run
admission state, and runtime state. An outer guard failure is not a PASS even
when an inner check sequence was green.

## Receipt truthfulness and fallback

A receipt is eligible only for its exact source SHA, trusted policy, supported
schema, complete outer outcome, and independent verification. `SKIPPED`,
`PENDING`, stale, historical, incomplete, cancelled, pressure-stopped, or
unverifiable evidence is not PASS.

GitHub-hosted CI remains the fail-closed fallback whenever a local CCP run is
not qualified. The harness must not bypass review, branch policy, trusted
secrets, native-platform obligations, or protected-environment controls.

## Evidence levels

| Level | Required evidence | Permitted statement |
| --- | --- | --- |
| L0 | Inventory only or stale/unknown upstream information | No CCP integration claim |
| L1 | Dated review of a public upstream source | Reference guidance exists |
| L2 | Sanitized fresh-session no-op marker | Discovery/bootstrap observed |
| L3 | Bounded read-only preflight following this contract | Activity contract observed |
| L4 | Exact clean SHA, complete outer result, and verified receipt | Native CCP flow observed |

`VERIFIED` is reserved for L4. A manual reference can be useful at L0 or L1,
but is never evidence that the harness automatically loads it.

## Public-boundary rules

Do not publish private paths, usernames, raw logs, commands, environment names
or values, tokens, container identifiers, repository names, customer data, or
private tool settings. A coding harness is not a CCP dependency: a future
change in a vendor tool must degrade its row to L0 or L1, not change CCP
runtime, policy, receipt, or GitHub fallback behaviour.
