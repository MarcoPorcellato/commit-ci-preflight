# Contributing

Thank you for improving Commit CI Preflight.

## Principles

- Keep the project vendor-neutral and independent from product-specific code.
- Prefer deterministic behavior and explicit evidence over heuristic claims.
- Never weaken receipt validation to make a check pass.
- Do not commit secrets, proprietary fixtures, generated caches, container
  layers, or copied third-party code.
- Keep changes small, reversible, and covered by focused tests.

## Local checks

```console
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Every user-visible change must update `CHANGELOG.md`. Architecture or trust
decisions require an ADR under `docs/adr/`.

By submitting a contribution, you agree that it is licensed under the Apache
License, Version 2.0, as stated in `LICENSE`.

