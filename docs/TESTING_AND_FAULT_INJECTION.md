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

Two CLI contracts intentionally acquire the real host admission slot and are
therefore native opt-in tests rather than members of the deterministic default:

```console
cargo test --locked --test benchmark_contract \
  native_cli_run_and_independent_verify_use_stable_exit_codes \
  -- --ignored --exact
cargo test --locked --test guard_exec_cli \
  native_guard_exec_portable_end_to_end_contract \
  -- --ignored --exact
```

Run them only after `resource status` is `Admit` and `admission status` reports
no active or queued work. An ignored result in the default suite is not native
PASS evidence; record the explicit command and exact commit separately.

## T0 characterization rules

The T0 tests deliberately describe six known gaps without claiming that the
gaps are fixed. Their names include `characterizes_` and the target tranche that
must eventually invert the assertion:

| Gap | Characterization location | Closure tranche |
|---|---|---|
| Immutable source execution (closed: positive snapshot tests replaced the characterization) | `src/source_snapshot.rs`, `src/workspace.rs` | T2 closed in source; native qualification pending |
| A complete cache entry can be mutated while retaining its complete marker | `src/cache.rs` | T5 |
| A historical v1 receipt can alter argv while retaining its declared digest | `tests/verification_contract.rs` | Intentional v1 compatibility; policy v1.1 requires v2 and independently rejects the plan mismatch |
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
- Admission tests include a separate-process activity that observes the slot
  owner/run identifier, distinguishes queue and slot lock roles, and verifies
  that an unlocked ticket without lease evidence is not quarantined.
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

Deterministic fake-closure tests prove terminal ordering and precedence: the
completion step precedes exactly one release, and a release failure overrides
the primary result. Process-tree and Docker lifecycle tests prove their own
containment boundaries only; they do not prove a real admission root, host
cleanup, a published receipt, or another platform. Process lists do not prove
release.

## Opaque cache-payload symbolic links

Deterministic coverage includes `src/cache.rs::complete_payload_symlinks_are_preserved_across_generation_reuse`, `src/cache.rs::failed_payload_preflight_removes_the_new_staging_generation`, `src/cache.rs::forced_fallback_copy_failure_removes_only_its_owned_staging_generation`, and `src/cache.rs::staging_cleanup_unlinks_payload_links_without_touching_targets`. Payload measurement is covered by `src/cache_payload.rs::payload_measurement_counts_links_without_following_targets`; fallback semantics and complete operation tracing by `src/cache_payload.rs::fallback_copy_preserves_each_link_target_and_external_sentinel` and `src/cache_payload.rs::fallback_copy_records_only_payload_paths_and_every_copy_operation`.

On macOS, deterministic tests distinguish attempted-and-succeeded clone reuse and deliberately forced fallback; the clone-success and forced-fallback tests preserve link identity. Injected fallback-copy failure occurs only after one successful copied object and proves exact owned-staging cleanup. `src/cache.rs::data_directory_preparation_failure_removes_owned_staging_and_releases_the_entry_lock` and `src/cache.rs::manifest_write_failure_removes_owned_staging_and_releases_the_entry_lock` cover the pre-data-root and pre-manifest failure windows. Owner-drop tests preserve unrelated and identity-mismatched staging directories. Unix fallback copy preserves each link target without following it and records metadata, directory enumeration, link reads, regular-file source/destination pairs, directory creation, and link creation without treating either copy path or a link target as a host path. Windows link-bearing payload reuse remains fail-closed and unsupported. These are deterministic source tests, not a native CCP receipt or native qualification.

## Managed-cache pin contract

The managed-cache pin tests are deterministic contract tests over an owned
fixture root. They cover canonical completed-entry selection, duplicate
deduplication and stable ordering, advisory-lock lifetime, invalid roots and
components, missing or incomplete entries, symlink and wrong-type rejection,
and release of previously acquired pins when a later acquisition fails. The
pin API does not initialize, repair, delete, quarantine, or publish receipts.

The lifecycle tests separately verify spawn-boundary revalidation and that a
failed validation makes zero child calls. The non-cooperative race test is
bounded to a change after pin acquisition but before the child spawn: the
validator must fail and the child-call count must remain zero. Replacement
after spawn-boundary revalidation is not prevented or guaranteed by this pin;
it is an unsupported external race and must not be converted into a success
claim. Qualification of a real runtime or receipt is a separate
native/evidence-gated activity.

## Compatibility fixtures

The v1 compatibility baseline remains pinned in:

- `tests/fixtures/config-v1-read-only.toml`
- `tests/fixtures/policy-v1.toml`
- `tests/fixtures/receipt-v1-pass.json`
- `schema/receipt-v1.schema.json`
- `schema/policy-v1.schema.json`
- `schema/policy-v1_1.schema.json`
- `schema/verification-report-v1.schema.json`

Receipt v1 remains readable during hardening, but it cannot provide v2 physical
assurance. Verifiers must report the actual assurance scope and fail closed when
repository policy requires evidence that the selected schema cannot carry.

Receipt v2 is additionally pinned in `schema/receipt-v2.schema.json` and
`tests/fixtures/receipt-v2-pass.json`. Deterministic tests cover v1/v2 dispatch,
snapshot-digest tampering, source revalidation and journal ownership.

Trusted-plan policy tests additionally cover policy-relative regular-file
configuration resolution, independent normalized-plan comparison with bounded
field pointers, v1 downgrade rejection, unsupported/revoked producers,
source-snapshot strategy, and the rule that a missing receipt cannot bypass
trusted configuration validation.

## Rules for future fault tests

1. Inject failure at a named boundary; do not depend on timing races.
2. Assert the terminal state, cleanup state, and evidence state separately.
3. Preserve the last-known-good generation and all foreign state.
4. Use bounded clocks, output, file counts, and retries.
5. Never use a skipped live test as proof.
6. Record platform-native evidence separately from deterministic tests.
7. Keep fault fixtures free of secrets, personal data, network access, and the
   operator's real cache or repository state.
## Matrix V2 compatibility test matrix

Focused tests cover the Matrix-only `matrix-v2-legacy-v1` profile: projection
`tests/matrix_contract.rs::legacy_profile_reproduces_historical_plan`,
representability `tests/matrix_contract.rs::legacy_profile_rejects_each_non_representable_current_field`,
command parity
`tests/plan_cli.rs::matrix_plan_profile_flag_is_exposed_only_by_configuration_commands`,
producer uniformity `tests/matrix_contract.rs::legacy_receipt_provenance_is_uniform`,
mutation rejection
`tests/verification_contract.rs::current_matrix_verifier_accepts_legacy_profile_receipt_and_rejects_mutations`,
and zero pre-admission mutation
`tests/runtime_cli.rs::legacy_profile_rejection_precedes_shared_state` and
`tests/runtime_cli.rs::legacy_profile_rejects_current_only_matrix_syntax_before_shared_state`.
The historical verifier is
`tests/verification_contract.rs::historical_matrix_verifier_accepts_legacy_profile_receipt_and_rejects_mutations`,
marked `#[ignore]` and run with `--ignored` only when
`CCP_HISTORICAL_VERIFIER_044697` points to the retained verifier binary with
SHA-256 `5321ff4d291ec24db6a7a5919bc08fc00a9d63767b630a3469fc39318c400277`,
The ordinary suite does not prove historical acceptance. The retained binary
was built from commit `044697dee9a0d678d30a4847d62ddf9b4970505b`. `verify` has no profile flag.
Tests do not infer policy or general trust; rollback target is `current-v2`.
