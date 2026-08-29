# Task 2 Report: Position-aware inventory accounting

## Result

Implemented exact payload-root dispatch in `src/cache.rs`. Inventory now delegates only `entry/data`, `entry/.staging-*/data`, and `entry/.backup-*/data` to the Task 1 payload walker. Payload descendant symlinks are counted as opaque link payloads; all other managed-root symlinks remain rejected.

## RED/GREEN evidence

- Added the three required Unix inventory fixtures and assertions.
- GREEN: `rtk env CARGO_TARGET_DIR=/private/tmp/ccp-task2-target CCP_TEST_ROOT=/private/tmp/ccp-task2-fixtures cargo test --locked cache::tests::inventory_ -- --nocapture` — 4 passed.
- GREEN: focused completed-source pin symlink/type test — 1 passed.
- GREEN: `rtk cargo fmt -- --check`.
- GREEN: `rtk env CARGO_TARGET_DIR=/private/tmp/ccp-task2-target cargo clippy --locked --all-targets -- -D warnings`.

## Files

- `src/cache.rs`: inventory dispatch, strict walker preservation, exact-root predicate, and regression fixtures.
- This report.

## Self-review and concerns

The original strict `bounded_tree_size` remains used by completed-source cloning/pinning. Inventory uses the new wrapper and passes the shared node counter into `measure_payload_tree`, preserving bounds and checked accounting. No Task 1 traversal code was changed. No known concerns.
