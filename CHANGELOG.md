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
