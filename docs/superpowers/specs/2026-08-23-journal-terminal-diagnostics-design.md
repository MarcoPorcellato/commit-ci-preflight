# Journal Terminal Diagnostics Design

## Purpose

Issue #63 identified a terminal CCP run whose durable journal contained only
`failure_kind: "unknown"`. The journal did not retain a safe diagnostic and no
cache artifact existed, so the event cannot be investigated or safely retried
from the retained evidence.

This change makes future unmatched top-level failures actionable without
weakening the fail-closed recovery model or persisting sensitive execution
data. It does not reinterpret the historical run and it does not authorize a
new R5 run.

## Scope and non-goals

In scope:

- Persist a bounded, enum-only diagnostic for a terminal `unknown` failure.
- Surface the diagnostic through the read-only recovery status JSON.
- Preserve strict parsing and terminal non-actionability.
- Prove the behavior with a deterministic synthetic failure after `executing`.

Out of scope:

- Capturing `Display` or debug error text, paths, environment variables,
  commands, check output, or secrets.
- Retrying, recovering, running Docker, publishing a receipt, or qualifying
  R5.
- Changing existing v1 journal entries, ownership markers, or source-binding
  token derivation.

## Chosen design

Keep the existing `RunJournalEntryV1` wire contract unchanged. Instead, add a
separate owned `terminal-diagnostic-v1.json` artifact inside the exact run
directory. Its strict, bounded payload contains:

- the existing `schema_version` and exact `run_id`;
- the terminal `failure_kind` (which must be `unknown`); and
- `diagnostic_code`, a closed enum initially containing
  `internal_command_failure` and `unclassified_top_level`.

The artifact is written before the terminal journal transition. Thus, if a
`failed` entry is durable, the new writer attempted to preserve its diagnostic;
if the later transition fails, recovery still sees the last non-terminal state
and remains fail-closed. Historic v1 journals legitimately have no diagnostic
artifact and remain readable.

`RunJournalStore` validates the artifact as an owned regular file with the
same small-size limit as markers. A present-but-invalid artifact makes the run
operator-required rather than silently accepted. The recovery status model
gains an optional bounded `failure_diagnostic` field only when a valid artifact
exists. `recover apply` continues to reject all terminal failed runs.

## Failure classification and data flow

`cli_failure_kind` retains its current coarse result. A companion classifier
returns a diagnostic only when that result is `unknown`:

1. `CliError::Internal(_)` maps to `internal_command_failure`.
2. Any other currently-unmapped top-level `CliError` maps to
   `unclassified_top_level`.
3. Mapped error domains retain their present `failure_kind` and no new
   diagnostic artifact.

`JournalLifecycleObserver::fail` accepts the coarse kind and optional
diagnostic. It delegates to the store so artifact persistence and terminal
transition remain the only write path. No raw error value is serialized.

## Compatibility and safety

An additive field on `RunJournalEntryV1` would make old strict readers reject
new entries because they use `deny_unknown_fields`. A standalone owned v1
artifact avoids changing that public entry schema and avoids changing the
global marker/version contract. Old journals without the artifact are valid;
unknown future artifact fields or versions fail closed.

The artifact is not an authorization signal. Recovery classification continues
to derive from the terminal lifecycle state. A diagnostic never changes a
failed run into a restartable or recoverable run.

## Verification

TDD starts with a unit-level synthetic `CliError::Internal` after the journal
has reached `executing`. The red test asserts a terminal `unknown` failure,
the safe code, absence of the synthetic error text, and no receipt behavior.
Additional tests prove strict artifact parsing, legacy no-artifact readability,
read-only recovery status, terminal `recover apply` rejection, and generated
schema/serialization redaction. Focused Rust tests precede the appropriate
full test suite. No CCP `run`, Docker, network, or R5 activity is part of this
verification.
