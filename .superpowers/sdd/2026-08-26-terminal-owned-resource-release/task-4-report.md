# Task 4 report: operator documentation

## Result

- Commit: `592c5e4` (`docs: define terminal resource release evidence`)
- Files: `docs/LOCAL_RUN.md`, `docs/COORDINATION_RUNBOOK.md`,
  `docs/ARCHITECTURE.md`, `docs/TESTING_AND_FAULT_INJECTION.md`
- Diff: 22 insertions, documentation only.

## Semantic review evidence

Reviewed the complete four-document diff against `src/terminal.rs` and the
Task 1-3 terminal call sites in `src/main.rs`. The docs record that run
watchdog completion joins before admission release, release is attempted once,
and release failure overrides the primary result. They preserve benchmark's
pre-start-only admission/no-mid-workload-watchdog behavior, managed-cache pin
child-lifecycle ownership, and source-snapshot cleanup before receipt sealing.
The coordination handoff requires the explicit result plus fresh admission,
runtime, and resource status; it does not infer release from child exit or
process visibility. Test prose limits deterministic evidence to fake closure
ordering/precedence and separate process-tree/Docker containment boundaries.

## Exact validation

```text
rtk cargo test --offline --locked --test cache_pin_contract
cargo test: 1 passed (1 suite, 0.00s)

rtk cargo test --offline --locked --test repository_hygiene_contract
cargo test: 6 passed (1 suite, 0.01s)

rtk cargo test --offline --locked --test release_hardening_contract
cargo test: 7 passed (1 suite, 0.01s)

rtk git diff --check
PASS (no output)
```

## Self-review and concerns

Self-review found no ownership broadening: the prose does not claim control of
swap, unrelated processes, foreign containers, or undeclared paths, and does
not claim real admission-root, Docker-cleanup, published-receipt, or
cross-platform qualification from deterministic tests. No known concerns.

## Fix round 1

Finding: P2 duplicate required claim in `docs/LOCAL_RUN.md`; the benchmark
no-mid-workload-watchdog sentence appeared twice in the same procedure.

Change: removed only the redundant second occurrence, preserving the first
bounded sentence and terminal-order prose.

Validation:

```text
rtk cargo test --offline --locked --test cache_pin_contract
cargo test: 1 passed (1 suite, 0.00s)

rtk cargo test --offline --locked --test repository_hygiene_contract
cargo test: 6 passed (1 suite, 0.01s)

rtk cargo test --offline --locked --test release_hardening_contract
cargo test: 7 passed (1 suite, 0.01s)

rtk git diff --check
PASS (no output)
```

The documentation fix commit is `5418ea0` (`docs: remove duplicate watchdog
claim`). The report append remains to be committed separately because the
`.superpowers` path is ignored by repository policy.
