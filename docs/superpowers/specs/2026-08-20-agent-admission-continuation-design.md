# Agent admission continuation design

Status: approved design; implementation not started  
Date: 2026-08-20  
Baseline: `c489ffcf8e4c5e2809f72c3c58740ededee53a69`  
Scope: opt-in agent-aware admission lifecycle for Commit CI Preflight (CCP)

## Decision

CCP will add a vendor-neutral, opt-in lifecycle for a live agent activity:

```text
submit -> waiting -> ready -> claim -> active -> terminal
                             |          |
                             v          v
                         expired     completed | cancelled | failed
```

The existing Rust admission coordinator remains the only authority for FIFO
ordering, host-wide slot locking, leases, heartbeats, cancellation, and safe
recovery. CCP will not add a Python scheduler, persist project commands in
tickets, or execute a command merely because a slot becomes free.

`ready` returns control to the same live activity. That activity must explicitly
claim a one-time capability before it can start a guarded command. Lost or
unclaimed work expires and the queue advances automatically.

## Problem and constraints

The current coordinator correctly keeps a synchronous `guard exec` process in
the FIFO queue until it can start. This works for a terminal owner, but an
adopted process can survive its original agent activity: after reparenting to
PID 1 it still owns its file lock and refreshes its lease. Conservative stale
recovery must not quarantine such a live lease, so several unreachable waiters
can block every visible activity.

This is an ownership-lifecycle gap, not evidence that FIFO admission or
fail-closed recovery should be bypassed. A process absent from one shell cannot
prove host-wide inactivity.

Codex supports durable work in `/goal` and readiness notifications, but its
public interface does not document an external API that can revive an arbitrary
ended local chat and authorize it to execute a command. CCP therefore uses
completion of a waiting CLI call as the wake-up path. Notifications are
informational only. The same neutral protocol must work for Codex, Claude Code,
and an ordinary terminal.

## Goals and non-goals

### Goals

- Preserve one Rust coordinator and strict FIFO order.
- Allow a living activity to await readiness without a heavy child starting.
- Require explicit, one-time claim before guarded execution.
- Detect and release a lost, reparented, cancelled, or reboot-invalidated agent
  ticket without an operator locating an orphan.
- Preserve legacy `guard exec` behavior for scripts and terminals.
- Keep all durable state privacy-minimized and vendor-neutral.

### Non-goals

- No unsupported control of Codex chats or hidden command execution.
- No Python daemon, cloud service, model call, or new runtime dependency.
- No weakening of lock, lease, resource, receipt, or GitHub fallback policy.
- No semantic change to legacy `guard exec` in this workstream.

## Architecture

### Core coordinator and session sentinel

Agent lifecycle state extends the existing per-user platform admission root; it
is not a second queue. The new agent mode provides a parent/session sentinel:
submission captures a PID plus anti-reuse process-start identity and boot
identity. The waiting client validates these periodically.

On macOS, the boot identity is the public `KERN_BOOTTIME` tuple. It is a stable
boot marker for conservative invalidation, not a cryptographic boot UUID; probe
failure, permission uncertainty, or inconsistent sampling is unsupported or
ambiguous and never evidence that a session is dead.

Parent exit, reparenting to PID 1, start-identity mismatch, explicit
cancellation, or changed boot identity make the session definitely invalid. In
that case, the client releases only its own ticket and lease, then exits without
claiming or starting a child. An ambiguous observation is never interpreted as
death; it fails safely and remains blocking until bounded expiry or owner
cancellation.

The sentinel is mandatory for the new agent protocol. It is intentionally not
imposed on legacy terminal/script use because detachment can be legitimate
there.

### Lifecycle and authority

`submit` allocates an opaque FIFO ticket and queued agent lease. It persists no
command, repository path, source SHA, prompt, username, raw log, or workload
content.

`wait` is tied to the session sentinel and renews the agent lease while queued.
When first in FIFO order, it changes the ticket to `ready` and returns a
cryptographically strong one-time capability. CCP stores only a digest of that
capability. A short ready TTL preserves order but does not start a child or
reserve a long-running execution slot.

`claim` validates ticket, capability digest, session identity, TTL, and FIFO
position. It consumes the capability and permits one following guarded argv.
Missing, replayed, expired, mismatched, or already-used capabilities fail
closed. An unclaimed ready ticket expires and advances the queue. `cancel` is
idempotent for the caller's queued or ready ticket and cannot release another
activity's active slot.

The implementation may reuse this lifecycle internally for synchronous acquire,
but legacy callers must retain their existing all-in-one wait, supervise, and
release behavior.

### Provisional CLI contract

```console
commit-ci-preflight admission agent submit --parent-pid <pid> --json
commit-ci-preflight admission agent wait --ticket <opaque-id> --json
commit-ci-preflight admission agent claim --ticket <opaque-id> --capability <token> --json
commit-ci-preflight admission agent cancel --ticket <opaque-id> --json
commit-ci-preflight guard exec --admission-grant <grant> -- <explicit argv>
```

The final plan may combine `claim` and `guard exec` atomically if that more
strongly prevents a time-of-check/time-of-use gap. It must preserve the explicit
agent decision between `ready` and execution; the guarded argv is supplied only
then and is never durable coordinator data.

`admission status --json` will distinguish slot ownership, queue-lock state,
queued-agent count, ready-agent count, opaque owner/run IDs, heartbeat, and
lease state. It retains the normative statement that a local process list does
not prove global inactivity.

## Persistence, recovery, and privacy

Durable state is limited to opaque IDs, lifecycle state, timestamps,
lease/heartbeat information, process-start evidence, boot-session identity, and
capability digests. Admission-root permissions remain per-user and restrictive.
No ticket enters a receipt.

After reboot, a changed boot identity proves the prior agent session is not
live. Recovery may clear its expired agent ticket without inferring anything
about a legacy active lock. Missing, malformed, foreign, contradictory, or
otherwise uncertain data remains fail-closed and is never silently deleted.

## Harness integration

Codex templates will keep work in `/goal`, call `submit`, block in `wait`, and
receive the ready result in the same task. The task re-checks its context and
chooses whether to claim. Claude Code and terminal templates use the same CLI
contract. Optional host notifications are informative and never execute work.

The existing multi-harness integration reference will be amended only after the
core behavior is implemented and tested. It must describe this as an opt-in
protocol, not as a capability to revive terminated chats.

## Deterministic verification

Tests will inject clock, process inspection, boot identity, token randomness,
and filesystem seams. Required coverage:

- FIFO ordering across five independent agent clients;
- no child execution before a valid claim;
- ready TTL expiry and advancement to the next ticket;
- one-time token and replay rejection;
- wrong ticket/session/capability rejection;
- parent exit, reparenting to PID 1, cancellation, and client-crash cleanup;
- ambiguous or PID-reused parent identity remaining fail-closed;
- reboot invalidation without touching a legacy active lock;
- malformed or contradictory state remaining blocking;
- status separation for slot, queue lock, queued, ready, and active states;
- legacy `guard exec` queueing and cancellation regression;
- documentation/template contract and public-data scans.

Core tests must not require Docker, OrbStack, a model, network access, or a
live harness. Later integration smokes are separate evidence, not substitutes
for the core tests.

## Delivery gates

1. **T1: model and seams.** Add lifecycle types, validation, and deterministic
   platform abstractions without a user-facing change.
2. **T2: queue operations.** Add `submit`, `wait`, `cancel`, and status;
   demonstrate FIFO and parent-loss cleanup with no pre-claim execution.
3. **T3: guarded claim.** Bind a consumed capability safely to one explicit
   guarded argv; demonstrate replay resistance, expiry, and cleanup.
4. **T4: references and pilot.** Add Codex `/goal`, Claude Code, and terminal
   templates; run one sanitized pilot after core tests pass.

Each tranche is an isolated, reviewable, reversible PR. It requires focused
Rust tests, source/diff review, and no heavy OrbStack gate unless an exact-head
qualification is separately authorized.

## Acceptance criteria

- A lost agent cannot retain or start work indefinitely through its ticket.
- A living agent receives readiness in FIFO order and claims exactly once.
- Tickets never expose or retain command or repository content.
- Legacy CLI behavior remains compatible.
- Uncertainty around locks, identity, leases, and recovery fails closed.
- Templates honestly state that CCP cannot externally revive a terminated chat.
- A pilot shows automatic queue advance after a deliberately lost client, with
  no manual lock cleanup and no unapproved execution.

## Deferred decisions and roadmap amendment

T1 must prove a reliable cross-platform process-start identity without a new
dependency. If that proof is unavailable, the sentinel remains platform-scoped
or returns a conservative unsupported outcome; PID-only identification is not
acceptable.

The final public command names, claim/grant transport, TTL values, and atomic
versus separate claim operation are deferred to the implementation plan.

`docs/PRODUCT_ROADMAP.md` currently defers new admission features pending other
activation gates. Before code implementation, it needs a narrow owner-approved
amendment recording this orphan-prevention and agent-safety exception. This
design does not override that roadmap by itself.

## Rationale

The design resolves the contradiction between exclusive heavy-work admission
and unattended agent work by separating waiting from execution in time, adding
a session sentinel, and feeding loss of ownership back into automatic cleanup.
It deliberately rejects a daemon that would execute work after an agent has
disappeared, preserving safety and vendor neutrality.
