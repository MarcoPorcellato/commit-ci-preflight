# Contributing

Thank you for improving Commit CI Preflight.

Read the [README](README.md) for product fit and the
[threat model](docs/THREAT_MODEL.md) before proposing a trust-boundary change.
For a vulnerability, follow [SECURITY.md](SECURITY.md) rather than opening a
public report.

## Principles

- Keep the project vendor-neutral and independent from product-specific code.
- Prefer deterministic behavior and explicit evidence over heuristic claims.
- Never weaken receipt validation to make a check pass.
- Do not commit or paste secrets, private receipts, raw logs, proprietary
  fixtures, personal data, generated caches, container layers, or copied
  third-party code.
- Keep changes small, reversible, and covered by focused tests.

## Local checks

```console
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

These deterministic local checks do not qualify a native Docker runtime path.
Run focused tests first, then the full relevant suite before asking for review.

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

## Choose the right public route

- Report incorrect behavior or a trust-boundary gap with the
  [bug-report form](https://github.com/MarcoPorcellato/commit-ci-preflight/issues/new?template=bug_report.yml).
- Propose a bounded capability or documentation improvement with the
  [feature-request form](https://github.com/MarcoPorcellato/commit-ci-preflight/issues/new?template=feature_request.yml).
- Ask about fit, setup, or a first trial with the
  [issue chooser](https://github.com/MarcoPorcellato/commit-ci-preflight/issues/new/choose) and select **Adoption help**.
- Follow the existing [pull-request template](.github/PULL_REQUEST_TEMPLATE.md)
  for scope, evidence, rollback, and public-claim hygiene. Do not duplicate
  that checklist in the pull request description.

Keep pull requests narrowly scoped. State the user-visible outcome, the trust
or compatibility boundary, and the focused checks run; then state the full
suite result or why it was intentionally not run.

By submitting a contribution, you agree that it is licensed under the Apache
License, Version 2.0, as stated in `LICENSE`.
