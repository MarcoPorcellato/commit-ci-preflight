# Admission Reconciliation V1 Design

## Goal

Provide an explicit, owner-authorized way to classify and reconcile selected
abandoned admission tickets without starting a workload. The command must
preserve the read-only behavior and schema of `admission status`, keep
`recover` limited to run journals, and fail closed whenever ownership or
liveness is uncertain.

## Problem and evidence

`AdmissionCoordinator::status_with_timeout` reads tickets with reclamation
disabled. Admission acquisition enables reclamation, so a valid unlocked ticket
with an absent or expired lease can remain visible until a later workload
attempt acquires the coordinator. This creates an operational gap: operators
must not manually remove admission files, but no explicit admission-repair path
exists.

The condition does not establish why a ticket was abandoned. A missing process
in one shell, an `active: false` field, or a free-looking slot lock is not
enough to establish safety. In particular, a slot-lock/lease contradiction is
reported as `unknown` and remains blocking.

## Scope

Add `commit-ci-preflight admission reconcile` with two explicit modes:

- preview is the default and is read-only;
- `--apply` is mutating and requires one or more repeated exact `--ticket-id`
  arguments.

The command operates only on the platform coordinator root. It must not create
an absent root, create a ticket, advance the counter, invoke a child, produce a
receipt, access cache data, or alter a run journal. It never accepts an
implicit "all tickets" target.

## Command contract

```text
commit-ci-preflight admission reconcile [--json] [--timeout-seconds N]
commit-ci-preflight admission reconcile --apply --ticket-id ID [--ticket-id ID ...] [--json] [--timeout-seconds N]
```

Preview reports bounded classifications for currently observed tickets and
leaves coordinator bytes unchanged. It may report that a ticket is eligible;
that statement is advisory only and is revalidated by apply.

Apply validates every requested identifier before touching state. Duplicate,
malformed, unknown, unselected, or ambiguous identifiers are rejected. A
successful apply reports only the selected outcomes. If any requested ticket is
unsafe, locked, fresh, future-dated, foreign, malformed, or otherwise
ambiguous, it returns a fail-closed result and does not claim reconciliation.

Human output is concise. JSON uses a new versioned reconciliation report, not
`AdmissionStatusV1`, with only bounded operational facts: mode, selected opaque
ticket IDs, classification, reason category, and outcome. It excludes paths,
commands, environment values, process inventories, raw errors, and log data.

## Ownership and serialization model

The reconciler first confirms the existing owned root and strict layout without
initializing or repairing it. It acquires the queue bookkeeping lock using the
existing bounded deadline and cancellation behavior. While that lock is held,
it attempts the slot lock without waiting. If the slot is busy, it releases the
queue lock and returns a fixed blocked live-or-unknown result. It never waits
for the slot while holding queue: an active guard releases its slot only after
acquiring queue, and reversing that order would deadlock. Once both locks are
held by reconciliation, ordinary CCP acquisition cannot enter while it decides
or mutates state.

For every selected ticket, apply opens the existing regular file without
following links, acquires its exclusive advisory lock, rereads and validates
the ticket marker, rereads the matching lease, and evaluates liveness anew.
Eligibility requires all of the following:

1. the ticket and coordinator owner marker are valid CCP-owned records;
2. the exact ticket file is a regular non-symlink object and its lock is held
   by reconciliation;
3. the slot lock is held by reconciliation and no contradictory lease state is
   present;
4. the matching lease is absent or valid and definitely expired; and
5. the target remains selected and has not changed since preview/apply began.

Any other shape remains protected. This includes a held ticket lock, active or
future lease, malformed or foreign ticket/lease, unsafe object, unsupported
layout, timeout, cancellation, lease-only residue, and unexplained
slot/lease contradiction. Preview never mutates these records; apply never
widens the selected target set to repair them.

The v1 reconciliation boundary is cooperative trusted-local coordination:
every supported CCP ticket-namespace writer must honor the queue, slot, and
ticket locks. Apply captures selected file identities from locked descriptors,
then rechecks every selected pathname before its first mutation. A hostile
same-UID process that ignores those locks and replaces a pathname after the
final recheck is outside this portable v1 guarantee; no macOS/Linux primitive
can atomically rename an arbitrary open descriptor by identity.

## Mutation and durability model

The command does not call the existing broad `scan_tickets(..., true)` or
`remove_stale` path: that path also quarantines staging and malformed entries
during scanning, which is outside this feature's authorization boundary.

After all selected tickets have been locked and revalidated, apply handles each
ticket independently with truthful outcomes. It rechecks the identity of every
selected ticket before beginning any mutation. It removes a selected eligible
lease before moving its ticket to an owned quarantine location and synchronizes
the affected directories after each durable mutation. Thus an
interruption after lease removal leaves a still-locked-at-decision ticket with
no lease, which a later explicit reconciliation can classify again; it avoids
creating a lease-only record from the normal success sequence.

Quarantine destination creation must be collision-safe and must never replace
existing evidence. A failure after one durable substep is reported as partial,
not success; unchanged, residual, and quarantined records remain inspectable
by a later explicit preview. The implementation must use an atomic no-replace
primitive on supported platforms and a documented fail-closed fallback where
that guarantee cannot be established.

All acquired ticket, slot, and queue locks are released on success, failure,
timeout, and cancellation. Tests cover busy-slot return plus active-guard
release, lease removal, all affected directory syncs, move collision/failure,
timeout, cancellation, and lock reuse. No result claims all-or-nothing
transactionality unless every selected outcome reached its terminal durable
state.

Once any selected ticket changes, a later storage, synchronization, timeout, or
cancellation failure returns a bounded partial report in canonical ticket
order. It records the changed ticket, the failing ticket, and `not_attempted`
for later selected tickets without exposing paths or raw error text.

## Compatibility

- Preserve `admission status` behavior, its exact schema `2.0`, and exit
  mappings.
- Preserve on-disk owner, ticket, lease, counter, and journal schemas.
- Keep `recover status/apply` exclusively for run journals.
- Prefer a dedicated reconciliation error/result type over adding public
  exhaustive variants to `AdmissionError`.
- Do not alter `run`, `benchmark`, `guard exec`, receipt, verification, cache,
  or resource-policy behavior.

## Tests and acceptance criteria

Deterministic tests must use injected time and synchronization rather than
host-process inference. They must prove:

1. status and preview preserve coordinator bytes;
2. apply rejects missing, duplicate, malformed, and unselected target IDs;
3. valid unlocked tickets with absent or expired leases reconcile without a
   workload, counter change, child process, receipt, cache, or journal change;
4. held ticket/slot locks, fresh or future leases, unsafe filesystem objects,
   malformed/foreign metadata, lease-only state, and unknown state fail closed;
5. preview evidence is revalidated before mutation and changes between phases
   block apply;
6. concurrent acquisition cannot enter while reconciliation holds queue and
   slot exclusion;
7. quarantine destinations never overwrite existing evidence;
8. injected lease-removal, quarantine, sync, timeout, and cancellation failures
   produce accurate per-ticket outcomes and release locks; and
9. existing status, acquisition, journal recovery, CLI help, privacy, and exit
   compatibility tests remain green.

## Documentation and public issue

Update the coordination runbook, troubleshooting, local-run guidance, CLI help
and generated documentation to distinguish status inspection, explicit
admission reconciliation, and run-journal recovery. Document that `active:
false` does not override `slot.state: unknown`, and that manual file deletion
remains unsupported.

The existing sanitized issue draft may be published only with a separate
maintainer authorization. It must not include local paths, ticket or journal
identifiers, raw command output, process data, or any unproven incident cause.

## Non-goals

This version does not auto-clean during status, terminate processes, infer
liveness from `ps`, repair malformed/foreign/staging state, retry workloads,
repair run journals, change the active installed producer, or modify shared
admission state without exact operator selection.
