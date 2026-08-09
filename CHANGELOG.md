# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project intends to follow
Semantic Versioning after its first public release.

## [Unreleased]

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
