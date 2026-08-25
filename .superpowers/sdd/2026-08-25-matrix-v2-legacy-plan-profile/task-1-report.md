# Task 1 report: historical Matrix V2 legacy fixture

## Scope

Added the compatibility configuration, independently materialized historical
plan JSON, provenance record, and fixture-integrity test owned by Task 1.

Historical source:

- commit: `044697dee9a0d678d30a4847d62ddf9b4970505b`
- tree: `5220164edf17831ce0c42dae1c14300ed1045015`

The plan was produced from an isolated `/private/tmp/ccp-044697` checkout using
the historical binary built with `cargo build --locked`. The fixture records
the exact command arguments, configuration/output hashes, plan digest, outer
digest, and ordered runtime configuration digests.

## Verification

RED was witnessed before the fixture set was complete: the focused test could
not compile while the included fixture files were absent. A subsequent test
attempt against the repository target was blocked by the sandbox's inability
to create `.cargo-build-lock`; this is an environment limitation, not a
product result.

GREEN command:

```text
rtk env CARGO_TARGET_DIR=/private/tmp/ccp-task1-target cargo test --test matrix_contract historical_legacy_fixture_is_self_consistent -- --exact --nocapture
```

Result: `1 passed, 0 failed`.

`git diff --check` passed. `cargo fmt` could not write the source file because
the repository target path was denied by the sandbox; the changed Rust code is
formatted manually and the focused test compiles and passes in the isolated
target.

## Limitations and concerns

- No CCP run, Docker runtime, network, cache/admission mutation, installation,
  publication, push, PR, or merge was performed.
- This report proves fixture self-consistency and historical plan production;
  it does not qualify runtime execution or current-source behavior.
