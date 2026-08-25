# Task 1 Report

## RED

Command:

```console
rtk cargo test --lib prepared_entry_clones_share_cleanup_and_lock_until_final_drop
```

Result: failed as expected before the production edit. The test panicked at
`src/cache.rs:2022:9` with `one clone must not remove live staging` (0 passed,
1 failed, 219 filtered out). This demonstrated that dropping the first clone
removed shared staging while the second clone still existed.

## GREEN

Commands and results:

```console
rtk cargo test --lib prepared_entry_clones_share_cleanup_and_lock_until_final_drop
# 1 passed, 219 filtered out

rtk cargo test --lib cache::tests
# 18 passed, 202 filtered out

rtk cargo fmt -- --check
# passed

rtk git diff --check
# passed
```

## Scope and implementation

`src/cache.rs` now gives each prepared generation one `Arc`-owned final-drop
state containing the staging identity and entry lock. Cleanup therefore runs
only on the final clone, while the lock remains held through validated cleanup.
The crate-visible `CacheGenerationExpectation` and
`PreparedCacheEntry::generation_expectation` API were added for Task 2. The
regression test verifies staging retention, lock contention, final cleanup, and
subsequent lock release.

## Commit

Commit SHA: `bd759e2`.

## Concerns

No known concerns within Task 1 scope. The report is intentionally limited to
`src/cache.rs` and this report file; no CCP, Docker, network, host-cache, or
remote operations were performed.
