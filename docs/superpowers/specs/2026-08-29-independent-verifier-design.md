---
type: architecture-design
title: "Physically independent Commit CI Preflight verifier"
status: proposed-for-review
last_verified: 2026-08-29
source_head: 6ff736b1e2a1dfde8778330efdd4b82c845d45e7
---

# Physically independent verifier design

## Decision

Convert the existing root package into a non-virtual Cargo workspace without
moving the root runner package. Add `ccp-core` as the single owner of protocol
types and pure verification logic, and add `ccp-verifier` as a thin binary that
depends only on `ccp-core`, `clap`, and `serde_json`. The existing
`commit-ci-preflight` package remains at the repository root and preserves its
public module paths and CLI behavior through explicit re-exports.

This is the behavior-preserving workspace approach selected over copying
protocol types or feature-gating the monolith. Rust types are nominal: copying
a struct or enum into another crate would create a different type. Public
re-exports preserve the single defining type and its identity.

## Evidence base

The design is bound to clean source
`6ff736b1e2a1dfde8778330efdd4b82c845d45e7`. Read-only local inventory found:

- one Cargo package containing both runner and verifier concerns;
- `config -> receipt` and `receipt -> config/runtime` dependency cycles;
- `verify -> matrix` and `matrix -> verify/runner` dependency cycles;
- schema assembly coupled to both receipt and the runner-owned Matrix module;
- no existing standalone verifier or physical crate-boundary test; existing
  receipt, schema, Matrix, verification, and CLI behavior tests remain the
  compatibility baseline.

Primary references:

- Cargo workspaces and default members:
  <https://doc.rust-lang.org/cargo/reference/workspaces.html>
- Cargo dependency and public-dependency behavior:
  <https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html>
- Cargo resolver behavior:
  <https://doc.rust-lang.org/cargo/reference/resolver.html>
- Cargo metadata and dependency-tree inspection:
  <https://doc.rust-lang.org/cargo/commands/cargo-metadata.html> and
  <https://doc.rust-lang.org/cargo/commands/cargo-tree.html>
- Rust visibility and re-exports:
  <https://doc.rust-lang.org/reference/visibility-and-privacy.html>
- Rust nominal types and layout limits:
  <https://doc.rust-lang.org/reference/types.html> and
  <https://doc.rust-lang.org/reference/type-layout.html>
- Cargo `rust-version` contract:
  <https://doc.rust-lang.org/cargo/reference/rust-version.html>

## Goals

1. `ccp-verifier` has no runner, Docker, process, cache, admission, resource,
   benchmark, migration, snapshot-materialization, or workspace dependency.
2. Receipt V1/V2, policy V1/V1.1/Matrix V2, canonical JSON, digest, schema,
   report, error, and exit-code behavior remain compatible.
3. Existing Rust paths such as `commit_ci_preflight::receipt::*`,
   `commit_ci_preflight::verify::*`, and verifier-facing Matrix types continue
   to name the same underlying definitions through `pub use`.
4. The root `commit-ci-preflight verify` and new `ccp-verifier verify` commands
   emit byte-identical stdout and the same exit code for the same fixture,
   arguments, and evaluation time.
5. The extraction is delivered as small red/green slices. No feature change is
   mixed into protocol movement.

## Non-goals

- No global CCP producer replacement, release, package publication, signing,
  static cross-platform claim, or GitHub gate migration occurs in M2.
- `verify-benchmark` and GitHub Actions migration remain root-runner concerns.
- M2 records binary size and dependency closure but does not invent a size
  threshold or claim a platform that was not built natively.
- M2 does not rename receipt, policy, schema, or report versions.

## Workspace and dependency graph

The root manifest keeps `[package]` and gains:

```toml
[workspace]
members = [".", "crates/ccp-core", "crates/ccp-verifier"]
default-members = ["."]
resolver = "3"
```

All packages retain `edition = "2024"` and `rust-version = "1.87"`.
Edition 2024 implies resolver 3. The workspace records `resolver = "3"`
explicitly for auditability; it is valid because the declared MSRV is 1.87.
If the MSRV is ever lowered below Rust 1.84, resolver 2 becomes the fallback
and the complete locked graph must be requalified.

```text
ccp-core
   ^
   |-- commit-ci-preflight (root runner and compatibility facade)
   `-- ccp-verifier (bounded verify/schema CLI)
```

`ccp-core` direct dependencies are limited to `schemars`, `serde`,
`serde_json`, `sha2`, and `toml`. `ccp-verifier` directly depends on
`ccp-core` and narrowly configured `clap`; `serde_json` is test-only in that
package. It must not depend on
`ctrlc`, `fs2`, `process-wrap`, `nix`, `saphyr`, or `saphyr-parser`.

One workspace lockfile is expected. Its containing runner-only packages does
not imply verifier linkage; the selected `ccp-verifier` resolved normal
dependency closure is the controlling evidence.

## Core ownership

| Core module | Owns | Must not own |
|---|---|---|
| `errors` | the single nominal `ReceiptError` definition used by canonical/config/receipt code | runner or CLI errors |
| `canonical` | canonical JSON recursion, byte encoding, SHA-256 digest using core `ReceiptError` | files, clocks, runner state |
| `config` | V1 config contract, normalized execution-plan types, pure parse/validate/normalize | Docker probing or execution |
| `runtime_evidence` | `RuntimeCapabilityEvidenceV1` wire type | runtime adapters or commands |
| `receipt` | receipt V1/V2 and shared evidence types, validation, sealing | receipt writing or atomic filesystem operations |
| `verification_model` | accepted-platform, status, decision, finding, report, and the pure validation/time helpers required by Matrix verification | policy dispatch or CLI rendering |
| `matrix` | pure Matrix config/plan/receipt/policy types, legacy digest compatibility, Matrix verification | cache, process, run, runtime, source materialization |
| `verify` | bounded policy/receipt loading, V1/V1.1/Matrix V2 dispatch, trusted-plan reconstruction | runner commands or publication |
| `schema` | receipt, policy, report, and combined Matrix schema generation | runner configuration schema unrelated to verification |

The root `config`, `receipt`, and `verify` modules become compatibility
facades. The root `matrix` module re-exports core contract types while retaining
only execution composition. The root `runtime` module re-exports
`RuntimeCapabilityEvidenceV1` from the core so its old public path remains
valid.

## Compatibility contract

The following are explicitly adopted as project compatibility contracts and
frozen before movement; Rust itself does not guarantee their wire or display
stability:

- fixture and schema SHA-256 values recorded in a versioned manifest;
- receipt IDs and canonical bytes for V1, V2, and Matrix V2;
- policy parsing and dispatch for versions `1.0`, `1.1`, and `2.0`;
- trusted-config path resolution relative to the policy file;
- malformed, unknown-field, oversized, stale, wrong-head, revoked-producer,
  and missing-input behavior;
- public error variants, payload types, adopted `Display` strings, source-chain
  behavior, and `Send + Sync` behavior;
- root verifier JSON/human output and exit codes: usage/policy error `2`,
  verification decision failure `3`, internal receipt error `70`;
- checked-in schema bytes and historical verifier compatibility fixtures.

A compile-time consumer matrix imports every frozen receipt, config/plan,
policy, report, Matrix, and error family through both the root facade and
`ccp_core`, then assigns values across the two paths. This proves re-exports
preserve type identity rather than merely providing look-alike types. A public
surface review also checks all `pub use` paths because dependency graph evidence
alone cannot prove public API compatibility.

Rust layout and enum discriminants are not wire-format guarantees. Byte
compatibility comes only from the existing canonical encoder and is proven by
golden bytes, field-order cases, old-version decode, and schema fixtures.

## Dedicated verifier CLI

The binary name is `ccp-verifier`. It exposes exactly two subcommands:

```text
ccp-verifier verify --receipt <path> --policy <path>
                    --expected-commit <40-or-64-lowercase-hex>
                    [--evaluated-at-utc <strict-UTC>] [--json]

ccp-verifier schema --kind <receipt-v1|receipt-v2|policy-v1|
                              policy-v1-1|policy-v2|verification-report-v1>
```

`verify` shares the same library entry point as the root CLI. All runtime JSON
serialization is performed by `ccp-core`, so the verifier binary never imports
`serde_json` outside tests. `schema` writes
the exact checked-in schema bytes to stdout and has no filesystem input beyond
normal stdout. It does not expose config, plan, run, benchmark, migration,
cache, resource, or publication commands.

## TDD and migration order

1. Pin fixture hashes, public paths, type identity, CLI parity, and forbidden
   dependencies with failing tests.
2. Introduce the workspace and empty package boundaries without moving behavior.
3. Extract canonicalization and pure config/plan contracts.
4. Extract runtime evidence, receipt contracts, and schema generation.
5. Split Matrix pure contract/verification/legacy logic from runner execution.
6. Extract common verification models and policy dispatch, breaking the final
   Matrix/verifier cycle.
7. Add the dedicated verifier CLI and prove old/new output and exit parity.
8. Remove duplicated definitions, leave explicit facades, and prove the final
   resolved dependency closure.
9. Reconcile public architecture documentation, changelog, release metadata,
   and M2 evidence without claiming distribution or identity.

Each step first runs a focused test that fails for the intended missing or
coupled boundary, then applies the smallest movement needed, then reruns the
focused contract before broader validation.

## Verification evidence

Required deterministic gates:

```console
rtk cargo fmt --all -- --check
rtk cargo test --locked --workspace --all-targets --all-features
rtk cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
rtk cargo doc --locked --workspace --no-deps
rtk cargo tree -p ccp-verifier --edges normal,no-dev,no-build
rtk cargo metadata --locked --format-version 1
rtk cargo run --locked --quiet --example generate_release_metadata -- --check
```

The dependency gate parses `cargo metadata` format 1, locates the exact
`ccp-verifier` package ID, walks only normal edges in `resolve.nodes[*].deps`
where a dependency kind is `null`, and rejects any reachable root or forbidden
package ID. `cargo tree -p ccp-verifier` is human-readable corroboration rather
than the parser. The gate also reviews public signatures/re-exports because
metadata cannot prove public API exposure. Optional
`cargo-semver-checks`, `cargo-public-api`, or `cargo-deny` may be evaluated
later as supplemental evidence; M2 does not require installing a new tool.

Full workspace validation and the release-only local verifier build must obey
the current global heavy-work contract and receive an exact `guard exec`
authorization if they qualify as heavy on the live host. The exact-head CCP
qualification is a separate heavy-run gate after deterministic review. Push,
PR, evidence publication, ready transition, and merge remain separate external
gates.

## Documentation rulings

- `ccp-core` is the canonical M2 package name because the approved programme
  specification and current product roadmap use it. The older
  `ccp-contract` name in `RELIABILITY_HARDENING_PLAN.md` is reconciled during
  documentation closure; no alias package is created.
- Static `x86_64-unknown-linux-musl` distribution belongs to M3. M2 may record a
  local candidate's size but cannot claim static or native availability without
  the corresponding target evidence.
- Benchmark verification remains outside the dedicated verifier until a future
  design proves it belongs in the trusted receipt gate.

## Completion criteria

M2 is complete only when:

1. the root API and CLI compatibility contracts pass;
2. old fixture/schema hashes and canonical bytes remain unchanged;
3. the new verifier passes the full positive and negative fixture matrix;
4. Cargo metadata/tree evidence proves physical independence;
5. full deterministic workspace tests, Clippy, docs, and release metadata pass;
6. exact-head review has no unresolved material finding;
7. a separately authorized CCP run produces a terminal independently verified
   receipt for the exact M2 head.

No release, distribution, identity, publication, or platform claim follows
from M2 alone.
