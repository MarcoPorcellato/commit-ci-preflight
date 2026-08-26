# Task 8 report

## Scope

Documentation-only Matrix V2 compatibility contract for `matrix-v2-legacy-v1`.
The repository hygiene test scans the public operator documents and rejects
selected production digest constants. No production source was changed.

## Evidence

- RED: `rtk cargo test --test repository_hygiene_contract matrix_legacy_profile_is_documented_without_production_digest_constants -- --exact --nocapture`
  initially hit the existing target lock permission; rerun with
  `CARGO_TARGET_DIR=/tmp/ccp-task8-target` failed as expected on missing
  `matrix-v2-legacy-v1` documentation.
- GREEN: `rtk env CARGO_TARGET_DIR=/tmp/ccp-task8-target cargo test --test repository_hygiene_contract -- --nocapture` — 7 passed.
- GREEN: `rtk env CARGO_TARGET_DIR=/tmp/ccp-task8-target cargo test --test release_hardening_contract` — 7 passed.

## Boundaries

No network, CCP, Docker, install, push, PR, or publication was performed.
The persistent target directory was not modified; the isolated target was used
only because the repository target lock was not permitted by the environment.
