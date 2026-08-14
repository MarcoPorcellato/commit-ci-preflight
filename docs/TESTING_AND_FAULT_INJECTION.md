# Testing and fault-injection contract

## Purpose

This document separates deterministic development evidence from host-dependent
qualification. A test result is valid only for the exact commit, command, and
environment class that produced it. Skipped or unavailable live tests are never
PASS evidence.

## Test classes

| Class | Default suite | External services | Evidence provided |
|---|---:|---:|---|
| Unit | Yes | None | Pure parsing, validation, state transition, and injected-failure behavior |
| Contract | Yes | None | Public schema, CLI, compatibility fixture, and policy behavior |
| Integration | Opt-in | Local process or disposable filesystem | Boundary behavior across owned components |
| Native | Opt-in | Supported host runtime | Real Docker/OrbStack, OS process, filesystem, and resource behavior |
| Chaos | Opt-in | Disposable owned environment | Crash, daemon restart, ENOSPC, timeout, and interrupted-write recovery |

`cargo test --all-targets --all-features` is the deterministic default. It must
not require Docker, OrbStack, network access, or live host-pressure thresholds.
Native and chaos qualification require separate commands and persistent
receipts; source inspection or a deterministic fake cannot substitute for them.

## T0 characterization rules

The T0 tests deliberately describe six known gaps without claiming that the
gaps are fixed. Their names include `characterizes_` and the target tranche that
must eventually invert the assertion:

| Gap | Characterization location | Closure tranche |
|---|---|---|
| Immutable source execution (closed: positive snapshot tests replaced the characterization) | `src/source_snapshot.rs`, `src/workspace.rs` | T2 closed in source; native qualification pending |
| A complete cache entry can be mutated while retaining its complete marker | `src/cache.rs` | T5 |
| A newly sealed receipt can alter argv while retaining the declared configuration digest | `tests/verification_contract.rs` | T7 |
| A partial ticket counter blocks admission | `src/admission.rs` | T6 |
| A monitor failure currently returns before process cleanup | `src/process.rs` | T4 |
| Docker execution is currently a one-shot client command without persistent daemon identity | `src/runtime.rs` | T3 |

These tests are regression tripwires, not acceptance of the behavior. A closure
tranche must replace each characterization with a positive invariant test and
must not merely rename, ignore, or remove it.

## Deterministic seams already available

- Process execution uses `ProcessSpawner`, `ManagedProcess`, `SupervisorPort`,
  `CancellationToken`, and `GenerationGuard`.
- Runtime qualification can use a fake `SupervisorPort`.
- Resource sampling uses `ResourceCommandRunner` and injected watchdog timing.
- Run orchestration exposes `Clock` and `CompletionBarrier`.
- Admission uses an explicit coordinator root in tests.
- Cache and workspace tests use CCP-owned fixture roots.

T1 introduces the shared fault-injecting durable-filesystem seam. T2, T3, T5,
and T6 then consume it rather than creating unrelated filesystem abstractions.

The T1 implementation exercises each atomic-replacement operation with a
deterministic fail-at-N harness and accepts only a complete old or complete new
value. Run-journal tests cover strict transitions, deterministic replay,
read-only status, exact-owner quarantine, malformed IDs, stable exit codes,
privacy-minimized JSON, root/run-bound opaque ownership tokens, idempotent
post-rename recovery, and injected storage-full failures. On Windows, Rust's
standard library cannot durably replace an existing file without a
remove-then-rename gap, so that operation explicitly fails closed; the active
journal avoids the limitation by publishing immutable create-new events.

Native crash/power-loss and Windows-host qualification remain separate gates.
Deterministic source tests do not claim either result.

## Compatibility fixtures

The v1 compatibility baseline remains pinned in:

- `tests/fixtures/config-v1-read-only.toml`
- `tests/fixtures/policy-v1.toml`
- `tests/fixtures/receipt-v1-pass.json`
- `schema/receipt-v1.schema.json`
- `schema/policy-v1.schema.json`
- `schema/verification-report-v1.schema.json`

Receipt v1 remains readable during hardening, but it cannot provide v2 physical
assurance. Verifiers must report the actual assurance scope and fail closed when
repository policy requires evidence that the selected schema cannot carry.

Receipt v2 is additionally pinned in `schema/receipt-v2.schema.json` and
`tests/fixtures/receipt-v2-pass.json`. Deterministic tests cover v1/v2 dispatch,
snapshot-digest tampering, source revalidation and journal ownership.

## Rules for future fault tests

1. Inject failure at a named boundary; do not depend on timing races.
2. Assert the terminal state, cleanup state, and evidence state separately.
3. Preserve the last-known-good generation and all foreign state.
4. Use bounded clocks, output, file counts, and retries.
5. Never use a skipped live test as proof.
6. Record platform-native evidence separately from deterministic tests.
7. Keep fault fixtures free of secrets, personal data, network access, and the
   operator's real cache or repository state.
