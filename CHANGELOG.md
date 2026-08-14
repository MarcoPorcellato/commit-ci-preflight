# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project intends to follow
Semantic Versioning after its first public release.

## [Unreleased]

### Added

- T2 immutable Git-object source materialization with a canonical path/mode/OID
  manifest, fail-closed LFS/submodule/symlink policy, pre-admission snapshot
  preparation, read-only runtime binding, post-run byte revalidation, and
  journal-owned cleanup/recovery. The accompanying ADR, TRIZ contradiction
  ledger, and invariant/evidence matrix distinguish deterministic proof from
  still-pending native qualification.

- Durable filesystem primitives with deterministic operation-failure tests,
  append-only run-journal v1 state transitions, and bounded `recover status`
  / `recover apply <run-id>` commands. Recovery status is read-only; apply
  uses root/run-bound ownership tokens, quarantines one exact CCP-owned
  unfinished journal, is retry-safe after rename, and never deletes it.
- Reliability-hardening roadmap with six explicit P0 characterization
  tripwires, an ordered T0-T11 delivery sequence, and a separate deterministic,
  integration, native, and chaos testing contract.
- Product roadmap for evolving the beta into proof-carrying CI with staged
  positioning, verifier separation, slim remote verification, distribution,
  one-command adoption, cost intelligence, Linux qualification, signed
  identity assurance, and evidence-backed case studies.
- Expanded PR 1 onboarding documentation: README proof-first inspection path,
  explicit users/non-users, assumption-labelled cost guidance, official
  comparison links, and contributor onboarding/tests guidance in
  `CONTRIBUTING.md`.
- Added reusable issue and pull-request templates, a public roadmap entrypoint,
  repository presentation guidance, and a readable 1280 × 640 social-preview
  source asset with deterministic contract checks.
- Complete cross-repository adoption and safe-recovery documentation, including
  a pinned-verifier GitHub gate template that keeps adopting-repository policy,
  CCP source, and commit-bound evidence in separate trust domains.
- Default-on host-wide single-slot admission for heavy `run` and `benchmark`
  commands with persistent advisory-lock tickets, cancellation, bounded
  timeout, stale-ticket recovery, and read-only status reporting.
- New `guard exec` shell-free wrapper for one explicit argv, with separate
  admission and child timeouts and live tee output for long local workflows.
- Pinned offline `fs2` 0.4.3 dependency for cross-process advisory locks.
- Default-on macOS-v1 host-memory pre-start admission and a two-second `run`
  watchdog with typed resource-pressure cancellation.
- Read-only `resource status --json`; Linux and Windows explicitly report
  `unsupported_not_enforced` for resource protection.
- Observation-only macOS `guard exec` resource history with bounded workload
  profiles, baseline/extrema summaries, deterministic outcomes, private atomic
  JSONL persistence and 100-record rotation. It does not alter admission,
  watchdog, receipts or remote execution.
- Privacy-minimized resource history v2 with stable workload families,
  executor/cache/execution-mode/target classifications, optional requested
  limits, 500-record rotation, direct OrbStack detection, and an explicit
  cross-repository coverage/adoption inventory. Legacy v1 data is untouched.

### Changed

- Relax macOS pre-start swap admission to the smaller of 10 GiB and 30% of
  physical RAM, while retaining all independent memory, compressor and in-run
  watchdog protections (`macos-v2`).

### Not Yet Included

- Predictive admission, `run`/`benchmark` history, container peak/stage metadata,
  observation of direct runtime processes that bypass `guard exec`,
  admission and resource evidence integration into receipts, and `benchmark`
  mid-workload watchdog coverage remain deferred.

### Fixed

- Keep the host-wide admission coordinator in its own persistent platform-cache
  root so starting guarded work cannot make the independently managed build
  cache uninitializable or invalid.
- Move the default build cache to the versioned
  `commit-ci-preflight-build-v1` namespace so legacy pre-release admission
  files cannot block initialization; legacy state is left untouched.
- Reconcile the implementation plan with the merged admission, macOS resource
  guard, and guarded external workflow tranches.

## [0.1.0-rc.1] - 2026-08-10

### Added

- Initial Rust 2024 application skeleton.
- Apache-2.0 licensing and attribution notice for Marco Porcellato.
- Security, contribution, architecture, and autonomous implementation plans.
- Versioned receipt v1 types, strict semantic validation, canonical JSON,
  SHA-256 integrity IDs, generated schema, and deterministic golden fixtures.
- Fail-closed TOML configuration v1, deterministic DAG normalization, bounded
  runtime/check/cache policy, and the read-only `ccp plan` CLI surface.
- Cross-platform process-supervisor contract with bounded output, timeout,
  cancellation, stale-generation rejection, and fail-closed cleanup semantics.
- Docker-compatible runtime capability probe, OrbStack identification, and a
  deterministic non-executing `dry-run` argv surface.
- Persistent cross-platform cache-root resolution with atomic ownership,
  content-addressed keys, bounded inventory, and preview-only cleanup.
- Deterministic workspace isolation contract with a read-only repository mount
  and narrowly scoped read-write cache and artifact bindings.
- End-to-end local `run` orchestration with clean-commit binding, deterministic
  result aggregation, stable exit codes, atomic canonical receipts, and
  fail-closed cache completion.
- Clean-room Rust, Python, and Node sample projects plus macOS OrbStack evidence
  for pinned-image execution with a read-only repository.
- Independent `verify` command with strict repository policy for commit,
  configuration, required checks, image, platform, and freshness; canonical
  machine report; and stable verification exit code 3.
- Pinned policy/report JSON schemas and deterministic tamper tests that keep
  integrity, policy, and identity assurance explicitly separate.
- Lightweight GitHub receipt gate with exact pull-request SHA binding,
  trusted-base verifier compilation, append-once evidence branches,
  least-privilege permissions, bounded summaries, and no remote project tests.
- Repository-local preflight configuration and policy for the Rust format,
  test, clippy, and documentation gates on accepted Apple Silicon macOS
  Docker-compatible execution.
- Read-only GitHub Actions migration assistant with a versioned deterministic
  compatibility report, explicit translated/manual/unsupported classifications,
  public fixtures, and fail-closed handling of arbitrary actions and expressions.
- Fixed native benchmark receipt and independent verifier with pinned correctness
  digest, create-new output, Mac/OrbStack probing, opt-in Linux/Windows public
  runner evidence, and an assumptions-based cost model.
- Genuine macOS arm64 with OrbStack, Linux x86_64, and Windows x86_64 native
  benchmark receipts plus exact GitHub run, job, runner-label, and artifact
  provenance for the fixed PR09 qualification workload.
- Beta-candidate hardening with a human-first README, SPDX 2.3 SBOM,
  deduplicated third-party notices, deterministic release-metadata drift gate,
  local candidate archive and checksums, installation and rollback runbooks,
  closed threat-model review, support matrix, and end-to-end demo tutorial.

### Fixed

- Fail before Docker with a precise error when a nested cache or artifact mount
  destination is missing, symlinked, or has the wrong object type beneath the
  read-only repository mount.
- Made benchmark contract verification explicit about GitHub Actions metadata
  and kept versioned text contracts byte-identical across Windows checkouts.

[Unreleased]: https://github.com/MarcoPorcellato/commit-ci-preflight/compare/v0.1.0-rc.1...HEAD
[0.1.0-rc.1]: https://github.com/MarcoPorcellato/commit-ci-preflight/releases/tag/v0.1.0-rc.1
