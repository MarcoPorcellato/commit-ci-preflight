# Admission Layout Recovery Design

## Status and purpose

Status: approved in chat on 2026-08-24; implementation not started.

PR #64 cannot produce its exact-head receipt because the canonical CCP
candidate rejects the host-wide admission root at the historical
`agent-tickets/` child. The root has the exact CCP ownership marker and the
child is currently a plain empty directory, but current `main` does not
implement the experimental agent-ticket lifecycle that created it.

This change adds a bounded, explicit, non-destructive recovery path for that
one known historical layout. It preserves fail-closed admission, does not
interpret agent-ticket state, and does not authorize a CCP run or any other
heavy operation.

## Scope and non-goals

In scope:

- Diagnose the exact historical `agent-tickets/` layout without relying on the
  normal status path that rejects unknown root children.
- Produce a versioned, hash-bound recovery plan only when the target is a
  plain empty directory and the complete coordinator is provably idle.
- Apply that exact plan only after reacquiring authoritative locks and
  revalidating every safety condition.
- Preserve the empty directory by atomically moving it into CCP's existing
  quarantine directory.
- Document the operator boundary and prove it with deterministic temporary-root
  tests.

Out of scope:

- Accepting or ignoring `agent-tickets/` during normal admission.
- Parsing, adopting, repairing, deleting, or quarantining any agent-ticket
  record, staging file, ticket, lease, or active lock.
- Merging the experimental agent-admission lifecycle.
- Recovering arbitrary unknown root children.
- Running CCP checks, Docker, a model, or network activity during verification.
- Automatically applying recovery from `status`, `run`, `benchmark`, or
  `guard exec`.

## Rejected approaches

### Allowlist the directory

Adding `agent-tickets/` to `validate_layout` would let the current coordinator
ignore state it cannot understand. A non-empty directory could contain an
active record, a staging file, malformed state, or contradictory companions.
This would bypass host-wide coordination and is rejected.

### Import the experimental lifecycle

The historical branch changes admission, durable filesystem, CLI, execution,
tests, and documentation across thousands of lines. It is not a bounded
prerequisite for terminal diagnostics and would materially enlarge PR #64.

### Manual deletion or quarantine

The coordination contract prohibits operators from deleting, moving, or
reinterpreting shared admission state. Recovery must be a CCP-owned operation
whose preconditions and outcome are testable and machine-readable.

Only the new hash-bound CCP command is supported. Its implementation does not
make an equivalent manual filesystem operation supported.

## CLI contract

Add a nested admission recovery command:

```console
commit-ci-preflight admission layout-recovery status --json \
  --timeout-seconds <bounded-seconds>

commit-ci-preflight admission layout-recovery apply \
  --expected-plan <lowercase-sha256> --json \
  --timeout-seconds <bounded-seconds>
```

Both operations default to five seconds and accept only integer timeouts from
1 through 60 seconds. Zero, values above 60, and values that cannot be parsed
are CLI input errors with exit code `2`, before any coordinator access. A
deadline reached while `status` is acquiring its snapshot returns
`operator_required` with the closed reason `lock_timeout`. The same deadline
during `apply` returns `not_applied`, exit code `70`, and no mutation.

`status` is read-only. Its schema `admission-layout-recovery/1.0` returns one
of these closed classifications:

- `not_needed`: the canonical layout is already valid and no historical
  target exists;
- `recoverable_empty_historical_agent_tickets`: all recovery preconditions are
  true, accompanied by `plan_sha256`;
- `operator_required`: any state is unsupported, non-empty, active,
  contradictory, foreign, malformed, or uncertain.

`apply` requires the exact 64-character digest returned by `status`. A stale,
malformed, or mismatched digest fails closed and performs no mutation. The
command is not invoked implicitly and is not evidence that a later heavy run
is authorized.

Apply outcomes are closed: `recovered` means the move, synchronization,
post-validation, and explicit lock release all completed; `not_applied` means
no recovery mutation occurred; `recovery_uncertain` means the directory may
already have moved but a later synchronization, validation, or unlock step
failed. Both non-success outcomes exit `70`; only `recovered` exits `0`.

The JSON is privacy-bounded. It contains schema, classification, target kind,
bounded reason codes, plan digest, and outcome. It omits absolute paths,
process data, commands, repositories, users, raw record contents, and
environment values.

## Recovery plan and binding

The plan digest is SHA-256 over canonical JSON containing only:

- plan schema and recovery kind;
- the validated CCP root-owner tuple;
- the exact historical child name and plain-directory type;
- canonical root-child names and types;
- empty canonical ticket and lease inventories;
- proof that the slot lock was acquirable and the queue lock is exclusively
  held by the recovery snapshot; and
- empty target inventory.

The digest is an operator binding, not a substitute for locking or validation.
`apply` reconstructs the plan under lock and requires exact digest equality.
Even when the digest matches, every semantic precondition is checked again
immediately before the rename.

The destination basename is derived from the completed plan digest, so it is
deterministic and append-once without making the digest definition circular:

```text
agent-tickets.recovered-v1-<full-plan-sha256>
```

An existing destination is a collision and blocks recovery. The implementation
never overwrites or removes a quarantine entry.

## Locking and state validation

Both commands inspect the existing platform admission root. They do not create
an alternate root.

The recovery snapshot follows the coordinator's lock order:

1. validate that the root and existing queue-lock path are plain objects;
2. acquire `queue.lock` exclusively with the bounded timeout;
3. require the existing `slot.lock` to be a plain file and acquire it
   exclusively without creating or changing either lock file;
4. validate the exact CCP owner marker;
5. validate every canonical root child and reject any unknown sibling other
   than the exact `agent-tickets` target;
6. require canonical `tickets/` and `leases/` to be plain directories with
   empty inventories;
7. require `agent-tickets/` to be a plain, non-symlink directory with an empty
   inventory;
8. require `quarantine/` to be a plain directory and the destination to be
   absent.

Failure to open or lock either lock, missing ownership, unexpected types,
permission uncertainty, clock/timeout uncertainty, or any directory entry
returns `operator_required` from `status`. A status lock deadline uses reason
`lock_timeout`; `apply` instead returns the separately defined `not_applied`
outcome and exit code `70`. The process releases locks without changing state.

Normal `admission status`, acquisition, and release behavior remains unchanged:
it continues to reject `agent-tickets/` until an explicitly authorized recovery
has completed. This prevents the current coordinator from ever ignoring agent
state it cannot interpret.

## Apply transaction

After reconstructing and matching the plan under both locks, `apply`:

1. atomically renames the empty `agent-tickets/` directory to the planned child
   of `quarantine/` on the same filesystem;
2. durably synchronizes the affected parent directories where the platform
   supports directory synchronization;
3. runs the existing strict canonical layout validation;
4. releases the slot lock, then the queue lock; and
5. returns `recovered` with the bounded destination basename.

No file is deleted. If the rename fails, the target remains in its original
location and the command reports a failure. If post-rename durable sync or
validation or explicit lock release is uncertain, the command returns
`recovery_uncertain` with the internal/unsafe-state exit class and does not
claim successful recovery. The preserved quarantine entry remains operator
evidence.

Re-running `status` after a successful apply returns `not_needed`. Re-running
`apply` with the old digest is non-actionable and does not change state.

## Code boundaries

- `src/admission.rs`: recovery report/plan types, strict snapshot validation,
  lock-scoped status and apply operations, and unit tests.
- `src/main.rs`: nested CLI parsing, bounded JSON/text rendering, and existing
  admission error/exit-code mapping; its unit tests exercise parsing and the
  test-only injected-coordinator dispatch seam.
- `src/admission.rs` unit tests cover coordinator behavior and no-mutation
  failure cases against owned temporary roots.
- `docs/COORDINATION_RUNBOOK.md` and `docs/TROUBLESHOOTING.md`: exact operator
  sequence and explicit statement that manual recovery remains unsupported.

Receipt schemas, run journals, source-binding tokens, resource policy, and
normal admission status schema are unchanged.

## TDD and verification

The first RED test exercises the CLI parser in-process: it constructs an owned
temporary coordinator root containing only an empty plain `agent-tickets/`
incompatibility, proves that normal status fails, then parses `admission
layout-recovery status`. The unmodified binary parser rejects that unknown
subcommand, providing an observable RED without referencing an absent Rust
API. GREEN adds a dispatcher seam that accepts an explicitly injected
coordinator only from `src/main.rs` unit tests; production dispatch continues
to resolve `AdmissionCoordinator::platform()` internally.

Integration behavior is covered through core coordinator tests plus in-process
CLI parse/dispatch tests. The release binary gains no coordinator-root flag,
environment override, hidden test mode, or path supplied by a caller. This is
required because the production admission boundary intentionally rejects
temporary roots and roots inside the current repository.

Required deterministic coverage:

- recoverable owned empty historical directory;
- normal status remains fail-closed before apply and works after apply;
- exact plan digest is required and stale/malformed digests make no changes;
- timeout values `0` and `61` are rejected before coordinator access, while a
  lock deadline reports the bounded fail-closed timeout outcome;
- non-empty target, including one staging entry, is operator-required;
- target symlink, regular file, permission/type uncertainty, or foreign owner
  is operator-required;
- held slot, queued ticket, lease, unknown sibling, or quarantine collision is
  operator-required;
- injected rename or synchronization failure never reports success;
- successful apply preserves the directory at the append-once quarantine path;
- repeated status/apply is idempotent and non-mutating;
- JSON contains only closed reason codes and no absolute path or record data;
- legacy admission, run-journal, and receipt tests remain unchanged and pass.

Verification uses formatting, focused admission unit tests, focused CLI tests,
and the full deterministic Rust suite. It performs no global-root recovery,
CCP `run`, Docker operation, evidence publication, network action, or R5 work.

## Operational gates after implementation

Implementation success does not authorize mutation of the live coordinator.
Before a live apply, the exact candidate path, source commit, and binary
SHA-256 must be frozen; read-only recovery status and its exact plan digest
must be preserved; and the user must explicitly authorize that one hash-bound
apply operation.

After apply, a fresh canonical `admission status --json` must report an idle,
readable coordinator before any separate heavy-run authorization is requested.
PR #64 receipt production and publication remain distinct later gates bound to
the resulting exact PR head.
