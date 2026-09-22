# Draft issue: explicit admission reconciliation

## Title

Add explicit admission reconciliation for abandoned tickets without starting a workload

## Problem

An abandoned admission ticket can remain visible indefinitely through read-only
status inspection. Operators following a queue-zero preflight have no dedicated
recovery command: the existing reclamation path runs only when another workload
requests admission.

At source commit `1015a0b42962dbf18334fd6dfc0d9148551d4984`,
`status_with_timeout` calls `scan_tickets(None, false)`
(`src/admission.rs:338`). Admission acquisition enables reclamation and removes
stale records (`241–242`, `257–266`). Valid tickets are reclaimable only after
their OS ticket lock can be acquired and their lease is absent or expired
(`704–710`).

An expired active lease can also produce `active=false` together with
`slot.state=unknown`: `slot_status` marks a free underlying slot lock plus an
active lease as contradictory (`849–875`), and status derives `active` from the
resulting state (`340`). Therefore `active=false` alone is not an idle-state
guarantee.

A reported incident had retained tickets and an expired owner lease. A
separately authorized admission acquisition cleared the tickets while an
unrelated run journal remained unchanged. This is consistent with the source
behavior; the original interruption or cleanup failure that abandoned the
tickets has not been established.

The CLI currently exposes only `admission status`. Existing `recover
status/apply` operates on run journals and must remain a separate lifecycle.

## Proposed scope

Add an explicit admission reconciliation command with a read-only preview
default and an apply mode requiring exact selected ticket identifiers. Provide
a separate versioned result containing classifications, affected identifiers,
blocked reasons, and final outcomes.

Apply must revalidate the current filesystem and ownership evidence under the
queue lock, retain exclusive protection against concurrent admission, and
acquire the selected ticket locks before mutation. Only valid CCP-owned tickets
with absent or definitely expired leases are eligible. Held locks, fresh or
uncertain leases, malformed metadata, foreign ownership, unsafe filesystem
objects, and unexplained contradictions must fail closed.

The first version should support valid abandoned tickets only. It must not
expose the existing broad scanning helper directly: that helper also
quarantines staging and malformed records during scanning.

Reconciliation must not create a workload ticket, advance the ticket counter,
execute a child, generate a workload receipt, initialize an absent coordinator
root, or modify run journals, cache entries, existing receipts, or unrelated
tickets. It must preserve recoverable evidence and report partial failure
accurately.

## Acceptance criteria

- [ ] Status and preview leave persisted coordinator bytes unchanged.
- [ ] Apply requires explicit mode selection and exact target identifiers; no
  implicit “all tickets” operation exists.
- [ ] Valid unlocked tickets with absent or expired leases can be reconciled
  without executing a workload.
- [ ] Slot, queue, and ticket locking prevent races with admission and live
  owners.
- [ ] Apply revalidates preview evidence; changed or ambiguous ownership blocks
  mutation.
- [ ] Fresh, future-dated, malformed, foreign, locked, and unsafe records
  remain protected.
- [ ] Untargeted tickets, counters, journals, cache data, and receipts remain
  unchanged.
- [ ] Quarantine names cannot overwrite existing evidence.
- [ ] Fault-injection tests cover interruption and I/O failure between ticket
  and lease mutations; subsequent explicit recovery remains safe and outcomes
  never report false success.
- [ ] Timeout and cancellation are bounded and release acquired locks.
- [ ] Existing status schema, acquisition behavior, on-disk schemas, and
  journal recovery remain compatible.
- [ ] Machine-readable results contain bounded operational facts and exclude
  paths, commands, environment values, process inventories, and raw logs.
- [ ] Deterministic regression tests cover the retained-ticket status condition
  and successful explicit reconciliation.
- [ ] Coordination, troubleshooting, local-run, and CLI documentation explain
  the new recovery path, its authorization boundary, and the requirement for
  fresh post-operation checks.
- [ ] Documentation clarifies that status never reclaims tickets,
  `active=false` is insufficient when slot state is unknown, and journal
  recovery does not repair admission state.
- [ ] Documentation reconciles its “expired valid lease” wording with the
  existing supported absent-lease case.

## Non-goals

This change does not authorize automatic cleanup during status inspection,
process termination, broader malformed-state repair, automatic workload retry,
or replacement of an installed producer.
