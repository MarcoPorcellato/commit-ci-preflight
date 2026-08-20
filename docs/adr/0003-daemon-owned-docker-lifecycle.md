# ADR 0003: daemon-owned Docker lifecycle

- Status: **Proposed — T3 candidate, not accepted**
- Date: 2026-08-19
- Decision owner: Marco Porcellato
- Scope: CCP-owned Docker-compatible check containers

## Context

The pre-T3 runtime used one docker run --rm child process for each check.
That gives the local supervisor a process to contain, but it does not give CCP
an independently verifiable daemon-side owner. If the Docker CLI dies, the
daemon may continue the container, and a receipt or admission release could
otherwise be attempted without proving that the workload is gone.

The reliability plan requires a deterministic lifecycle with explicit
ownership, cleanup, and final absence proof. The contract must remain shell-free,
portable across Docker-compatible runtimes, and free of a new daemon API
dependency in this tranche.

## Decision

Use the Docker CLI as a structured, shell-free lifecycle adapter:

    create (without --rm)
      -> start
      -> attach + wait
      -> inspect
      -> stop
      -> kill (only if stop fails)
      -> rm --force
      -> inspect (must be not-found)

The lifecycle derives one deterministic container name from the run ID and
check ID using a domain-separated SHA-256 digest. Every created container has
these labels:

- com.commit-ci-preflight.owner=commit-ci-preflight
- com.commit-ci-preflight.run-id=<opaque run identifier>
- com.commit-ci-preflight.check-id=<normalized check identifier>

Only the exact derived name is used for lifecycle operations. Recovery and
future cleanup may select a container only when the exact CCP owner label,
run label, journal ownership, and derived name all agree. No broad label search,
prefix deletion, or unowned-container removal is permitted.

docker wait is authoritative for the check exit code. docker attach is used
only for bounded stdout/stderr capture. A successful check result is returned
only after the final inspect proves the named container is absent. Any create,
start, attach, inspect, stop, kill, remove, or final-verification uncertainty
is a typed non-PASS outcome.

## Alternatives considered

### Docker Engine API

Rejected for this tranche. It could provide stronger daemon interaction, but it
would introduce a new client dependency and a second transport/security surface
before the existing shell-free command contract is covered by deterministic and
native tests. Reconsider only after a bounded dependency and socket-permission
review.

### docker run --rm

Rejected. The daemon owns the container, but CCP cannot reliably identify or
prove final absence after losing the client.

### Broad cleanup by label or name prefix

Rejected. It can terminate another activity's container and violates the
fail-closed ownership boundary.

## Consequences

Positive:

- daemon-side ownership survives Docker CLI loss;
- every terminal path has an explicit cleanup proof;
- no new runtime dependency is required;
- deterministic command vectors can be tested without Docker.

Costs and residual gates:

- lifecycle cleanup consumes bounded command time in addition to check time;
- T4 must add one total deadline and cleanup sub-budgets;
- T7 must bind the lifecycle/runtime evidence to receipt v2;
- T11 must execute native Docker/OrbStack failure-path tests on an exact
  accepted head;
- the current T3 worktree is a candidate only and must not be treated as
  qualified while T2 exact-head evidence is pending.
