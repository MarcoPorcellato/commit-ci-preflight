# M0 compatibility manifest

This manifest is bound to base commit `5fed7c443504969e62980141048f9279f9fa1dfe`.
It records SHA-256 digests for the Task 2 compatibility captures, Task 3
downstream fixture, final compatibility test, and all nine schemas.

Task 2 capture commands:

```text
rtk cargo run --locked --offline -- --help
rtk cargo run --locked --offline -- plan --help
rtk cargo run --locked --offline -- verify --help
rtk cargo run --locked --offline -- plan --config tests/fixtures/config-v1-read-only.toml --json
rtk cargo run --locked --offline -- plan --config tests/fixtures/config-v2-matrix.toml --json
rtk cargo run --locked --offline -- plan --config tests/fixtures/config-v2-legacy-compatible.toml --matrix-plan-profile matrix-v2-legacy-v1 --json
rtk cargo run --locked --offline -- verify --receipt tests/fixtures/receipt-v1-pass.json --policy tests/fixtures/policy-v1.toml --expected-commit 0123456789abcdef0123456789abcdef01234567 --evaluated-at-utc 2026-08-08T12:30:00Z --json
rtk cargo run --locked --offline -- verify --receipt tests/fixtures/receipt-v1-pass.json --policy tests/fixtures/policy-v1.toml --expected-commit bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb --evaluated-at-utc 2026-08-08T12:30:00Z --json  # exits 3; capture stdout only
rtk cargo test --locked --offline --test compatibility_baseline print_normalized_dry_run_baselines -- --ignored --nocapture
```

Task 2 focused GREEN checks:

```text
rtk cargo test --locked --offline --test compatibility_baseline root_help_bytes_match_the_baseline
rtk cargo test --locked --offline --test compatibility_baseline command_help_bytes_match_the_baseline
rtk cargo test --locked --offline --test compatibility_baseline plan_and_verification_bytes_and_exit_codes_match_the_baseline
rtk cargo test --locked --offline --test compatibility_baseline matrix_plan_profiles_match_the_baseline
rtk cargo test --locked --offline --test compatibility_baseline dry_run_profiles_match_the_baseline_without_execution
rtk cargo test --locked --offline --test compatibility_baseline usage_error_exit_code_remains_two
```

Task 3 commands:

```text
rtk cargo generate-lockfile --offline --manifest-path tests/fixtures/public-api-compat/Cargo.toml
rtk cargo test --locked --offline --test compatibility_baseline supported_public_facade_compiles_downstream
```

Task 2 terminal test targets:

```text
rtk cargo test --locked --offline --test compatibility_baseline
rtk cargo test --locked --offline --test plan_cli
rtk cargo test --locked --offline --test verify_cli
rtk cargo test --locked --offline --test receipt_contract
rtk cargo test --locked --offline --test matrix_contract
rtk cargo test --locked --offline --test verification_contract
```

Regenerate captures with the commands above, update files using reviewed
patches, then measure every listed path with `sha256sum` and update this
manifest. Fixture hashes prove byte stability only; they do not prove Docker,
runtime, admission, or project-run behavior. No receipt publication or heavy
surface is exercised by this baseline.
