# Commit CI Preflight

Run reproducible Linux checks on developer-owned hardware, produce a
commit-bound receipt, and let GitHub verify the evidence instead of repeating
the heavy workload.

Commit CI Preflight is an independent, vendor-neutral Apache-2.0 project with a
Rust core. It is designed for teams whose remote CI cost or queue time is
growing, but who do not want savings to weaken review, security, or platform
coverage.

> Status: **0.1.0 release candidate**. The source implementation and native
> benchmark evidence are complete. No package, signed artifact, tag, or GitHub
> Release has been published.

## The problem

Running the same formatter, compiler, test suite, and documentation build on a
powerful developer machine and again on paid hosted runners can be expensive and
slow. Moving everything to a self-hosted runner changes where jobs execute, but
still leaves a long-lived machine attached to GitHub and does not by itself
produce portable, independently checkable evidence.

Commit CI Preflight splits the work:

- the heavy, reproducible checks run locally in a pinned Linux container;
- a canonical receipt binds the exact Git commit, normalized configuration,
  image digest, commands, platform, and results;
- an independent Rust verifier applies repository policy;
- a small GitHub gate verifies the receipt against the exact pull-request head;
- review, permissions, trusted secrets, deployments, and uncovered native
  platforms remain remote.

The goal is not “zero CI”. The goal is to spend remote CI only where the remote
control plane adds information or trust.

## How it works

```mermaid
flowchart LR
    A["Reviewed source commit"] --> B["ccp run on developer hardware"]
    B --> C["Pinned Linux container checks"]
    C --> D["Canonical receipt"]
    D --> E["Independent ccp verify"]
    E --> F["Append-only evidence branch"]
    F --> G["Small GitHub receipt gate"]
    G --> H["Status on exact PR head"]
```

The source checkout is read-only inside the container. Only declared cache and
artifact paths are writable. Commands are explicit argv vectors, not an
implicit shell. Receipts omit raw output, environment values, source contents,
absolute home paths, and personal or machine identity fields.

A receipt is integrity and policy evidence. It is not an identity attestation,
a signature, or proof that arbitrary local and hosted workflows are identical.

## Quick start

### 1. Build from reviewed source

```console
git clone https://github.com/MarcoPorcellato/commit-ci-preflight.git
cd commit-ci-preflight
cargo test --locked --workspace --all-targets --all-features
cargo build --locked
./target/debug/commit-ci-preflight --version
```

### 2. Inspect a configuration without running checks

```console
./target/debug/commit-ci-preflight plan   --config examples/projects/rust/.commit-ci-preflight.toml
./target/debug/commit-ci-preflight doctor   --config examples/projects/rust/.commit-ci-preflight.toml
./target/debug/commit-ci-preflight dry-run   --config examples/projects/rust/.commit-ci-preflight.toml
```

`doctor` performs a bounded read-only runtime probe. `dry-run` prints the
exact container argv and mounts but does not execute project code.

### 3. Run the clean-room demo

Follow the [end-to-end tutorial](docs/TUTORIAL.md). It copies a tiny public Rust
fixture into its own Git repository, runs its test through a pinned container,
writes `.ccp/receipt.json`, and verifies that receipt against a
repository policy.

For installation, checksum verification, and local candidate archives, see
[the installation guide](docs/INSTALLATION.md).

## What makes it different

| Tool or approach | Primary model | Relationship to Commit CI Preflight |
|---|---|---|
| GitHub-hosted runners | GitHub provisions a remote VM and executes the workflow | Keep for remote identity, permissions, secrets, deployments, and platforms not covered by accepted local evidence |
| GitHub self-hosted runners | A machine you manage stays connected to GitHub and accepts GitHub-dispatched jobs | Commit CI Preflight runs before push without registering a long-lived runner; GitHub receives a minimized receipt |
| `act` | Reads GitHub Actions workflows and uses Docker to run actions locally | Commit CI Preflight intentionally does not execute marketplace actions; its importer emits an inert translated/manual/unsupported report and execution uses a smaller explicit config |
| Dagger | Programmable delivery engine with pipelines in code, content-addressed caching, services, and tracing | Commit CI Preflight is not a pipeline SDK or orchestration platform; it focuses on bounded local checks, canonical evidence, and a small remote policy gate |
| Earthly | Repeatable containerized builds described by Earthfiles | Commit CI Preflight does not introduce a build language; it wraps existing commands and focuses on receipt verification. Earthly's official documentation currently states that the project is no longer actively maintained |
| Commit CI Preflight | Explicit local check plan plus commit-bound receipt and independent policy verification | Optimizes the local/remote trust split rather than trying to reproduce every CI feature |

Official project descriptions used for this comparison:

- [GitHub self-hosted runners](https://docs.github.com/en/actions/concepts/runners/self-hosted-runners)
- [nektos/act](https://github.com/nektos/act)
- [Dagger documentation](https://docs.dagger.io/)
- [Earthly documentation](https://docs.earthly.dev/)

These projects solve overlapping but different problems. Commit CI Preflight
does not claim feature superiority or full GitHub Actions parity.

## When to use it

Use the beta candidate when:

- expensive or slow checks are deterministic and container-friendly;
- developers have capable local hardware;
- the repository can pin its runtime image by digest;
- project checks need only declared writable cache or artifact paths;
- maintainers can review a small explicit TOML plan;
- GitHub should retain the control-plane checks that only GitHub can know.

A strong initial fit is a private repository with large Rust, Python, Node, or
documentation checks that already run consistently in Linux containers.

## When not to use it

Do not use the beta as the sole gate when:

- checks require trusted cloud secrets, deployments, or private infrastructure;
- unreviewed code must be treated as actively hostile;
- macOS, Windows, GPU, or hardware-specific behavior lacks native evidence;
- the workflow depends on arbitrary marketplace actions or complex GitHub
  expression semantics;
- the organization requires signed identity-bound attestations;
- repository policy cannot safely accept locally produced evidence.

In those cases, retain the relevant remote or native jobs. Cost reduction never
overrides a missing trust fact.

## Core commands

```text
commit-ci-preflight plan
commit-ci-preflight doctor
commit-ci-preflight dry-run
commit-ci-preflight run
commit-ci-preflight verify
commit-ci-preflight cache path|init|inventory|cleanup
commit-ci-preflight migrate-github-actions
commit-ci-preflight benchmark
commit-ci-preflight verify-benchmark
```

Key boundaries:

- `run` requires a clean Git commit and writes a canonical receipt;
- `verify` separates integrity, policy, and identity assurance;
- cache cleanup is preview-only in 0.1.0;
- `migrate-github-actions` parses YAML as untrusted data and does not execute
  marketplace actions, expressions, commands, or secrets;
- benchmark timing is observational and never affects the pinned correctness
  digest.

## Evidence and limitations

The fixed benchmark contract produced the same correctness digest on:

- native macOS arm64, with a separate OrbStack capability probe;
- native Linux x86_64 on `ubuntu-24.04`;
- native Windows x86_64 on `windows-2025`.

Receipts, exact run metadata, hashes, and claim boundaries are in
[the PR09 evidence matrix](docs/evidence/pr09/README.md).

This proves the fixed benchmark contract, not complete runtime qualification on
every platform. The complete repository preflight is currently qualified on
macOS arm64 through OrbStack. Linux and Windows complete `run` paths remain
pending. See [the beta support matrix](docs/BETA_SUPPORT.md).

## Security and privacy

Read [the threat model](docs/THREAT_MODEL.md) before enforcing receipts.
Important non-claims:

- containers are not a complete sandbox against hostile code;
- SHA-256 integrity does not prove producer identity;
- checksums are not signatures;
- cache contents are not attestation evidence;
- a local PASS does not replace GitHub review, branch policy, or trusted
  deployment controls.

Report vulnerabilities privately as described in [SECURITY.md](SECURITY.md).

## Documentation

- [Implemented architecture](docs/ARCHITECTURE.md)
- [Installation and checksums](docs/INSTALLATION.md)
- [End-to-end tutorial](docs/TUTORIAL.md)
- [Configuration contract](docs/CONFIGURATION.md)
- [Local run contract](docs/LOCAL_RUN.md)
- [Receipt specification](docs/RECEIPT_SPEC.md)
- [Verification policy](docs/VERIFICATION_POLICY.md)
- [GitHub gate](docs/GITHUB_GATE.md)
- [GitHub Actions compatibility subset](docs/GITHUB_ACTIONS_COMPATIBILITY.md)
- [Cache and workspace contract](docs/CACHE_AND_WORKSPACE.md)
- [Benchmark and parity evidence](docs/BENCHMARK_AND_PARITY.md)
- [Upgrade and rollback](docs/UPGRADE_AND_ROLLBACK.md)
- [Beta support policy](docs/BETA_SUPPORT.md)
- [Architecture and implementation plan](docs/IMPLEMENTATION_PLAN.md)

## Contributing

Issues and narrowly scoped pull requests are welcome. Read
[CONTRIBUTING.md](CONTRIBUTING.md), preserve the fail-closed claim boundaries,
and never include secrets, proprietary fixtures, or personal data.

## License and attribution

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) and
[NOTICE](NOTICE).

Copyright 2026 Marco Porcellato.

Apache-2.0 section 4(d) governs preservation of the attribution notice in
redistributions that include a `NOTICE` file. The license does not require
advertising endorsement and does not grant trademark rights.
