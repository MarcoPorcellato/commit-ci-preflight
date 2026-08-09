# Commit CI Preflight

Run reproducible Linux CI checks on developer-owned hardware, then emit a
machine-verifiable receipt before spending remote CI minutes.

Commit CI Preflight is an independent, vendor-neutral open-source project. It
is not a Matryca product and has no runtime or source-code dependency on
Matryca.

## Status

The project is in **bootstrap / pre-alpha**. The Rust core and receipt contract
will be implemented through the reviewed phases in
[`docs/IMPLEMENTATION_PLAN.md`](docs/IMPLEMENTATION_PLAN.md).

No current command should be treated as a security attestation or as proof of
GitHub Actions parity.

The implemented receipt integrity contract is documented in
[`docs/RECEIPT_SPEC.md`](docs/RECEIPT_SPEC.md). It deliberately does not claim
identity-bound attestation.

The TOML planner and local runner are documented in
[`docs/CONFIGURATION.md`](docs/CONFIGURATION.md) and
[`docs/LOCAL_RUN.md`](docs/LOCAL_RUN.md). The process supervisor, runtime
probe, and explicit container argv renderer are documented in
[`docs/RUNTIME.md`](docs/RUNTIME.md). Persistent cache ownership, inventory,
and explicit workspace mounts are documented in
[`docs/CACHE_AND_WORKSPACE.md`](docs/CACHE_AND_WORKSPACE.md).

## Product direction

The tool will:

- execute declared checks in a pinned Linux container through a small runtime
  abstraction;
- support Docker-compatible engines, with OrbStack as the first qualified
  macOS environment;
- produce canonical receipts bound to the Git commit, configuration, image,
  commands, platform, and results;
- verify receipts locally and in a deliberately small remote GitHub gate;
- distinguish `PASS`, `FAIL`, `PENDING`, and `NOT_RUN` without inflating local
  evidence into claims that were not executed;
- keep caches outside temporary directories and make their location and
  cleanup explicit.

## Rust core

The production core is written in Rust 2024 edition with a minimum supported
Rust version of 1.87. Third-party dependencies are added only in scoped,
reviewed pull requests and recorded in
[`docs/DEPENDENCIES.md`](docs/DEPENDENCIES.md).

```console
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo run -- --version
cargo run -- plan --config examples/config/rust-project.toml
cargo run -- dry-run --config examples/config/rust-project.toml
cargo run -- doctor --config examples/config/rust-project.toml
cargo run -- run --config examples/projects/rust/.commit-ci-preflight.toml \
  --repository examples/projects/rust --generation 1
cargo run -- cache path
cargo run -- cache init
cargo run -- cache inventory --json
cargo run -- cache cleanup --dry-run
```

`doctor` performs only a bounded, read-only Docker-compatible capability probe.
`dry-run` prints explicit argv and mount bindings, but never starts Docker,
creates the cache root, or executes a declared check. `run` requires a clean
Git checkout, executes checks without an implicit shell, and writes a canonical
receipt. Cleanup remains preview-only. The receipt is integrity evidence, not
an identity attestation; independent policy verification arrives in PR 06.

## Independence and clean-room boundary

The repository may encode general lessons learned while operating local CI,
but it must not copy proprietary corpora, secrets, product-specific fixtures,
or source code from another project. Compatibility work must rely on public
specifications, documented behavior, and independently authored tests.

## License and attribution

Licensed under the Apache License, Version 2.0. See [`LICENSE`](LICENSE) and
[`NOTICE`](NOTICE).

Copyright 2026 Marco Porcellato.

Apache-2.0 section 4(d) governs preservation of the attribution notice in
redistributions that include a `NOTICE` file. The license does not require
advertising endorsement and does not grant trademark rights.

## Security

Do not report vulnerabilities in public issues. Follow [`SECURITY.md`](SECURITY.md).
