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

## Contributor onboarding workflow

### Setup

```console
git clone https://github.com/MarcoPorcellato/commit-ci-preflight.git
cd commit-ci-preflight
rustup toolchain install 1.96.0
rustup component add clippy rustfmt
cargo build --locked
```

- Use the README first-inspection path before opening a PR for
  documentation-facing changes.
- Use `git status --short` before tests and keep the worktree clean for release
  candidate operations.

### Component-to-test mapping

Use these component anchors to pick focused tests before opening a PR.

- Command surface and CLI UX:
  - `cargo test --locked --test plan_cli`
  - `cargo test --locked --test verify_cli`
  - `cargo test --locked --test runtime_cli`
  - `cargo test --locked --test benchmark_contract`
- Receipt and policy contracts:
  - `cargo test --locked --test receipt_contract`
  - `cargo test --locked --test verification_contract`
  - `cargo test --locked --test release_hardening_contract`
- GitHub gate and migration behavior:
  - `cargo test --locked --test github_gate_contract`
  - `cargo test --locked --test github_actions_compatibility`
- Runtime and process supervision:
  - `cargo test --locked --test guard_exec_cli`
  - `cargo test --locked --test process_supervisor`

### PR release-boundary checks

For any documentation change touching release or package-facing statements, run:

```console
cargo test --locked --quiet --test release_hardening_contract
cargo run --locked --quiet --example generate_release_metadata -- --check
```

Dependency additions or upgrades must update `docs/DEPENDENCIES.md` with their
purpose, enabled features, license, and transitive-risk review.

By submitting a contribution, you agree that it is licensed under the Apache
License, Version 2.0, as stated in `LICENSE`.
