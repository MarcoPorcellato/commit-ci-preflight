# Commit CI Preflight roadmap

## Public roadmap (compact)

- Current objective: publish repository-hygiene and contributor-entry artifacts so new users can understand what Commit CI Preflight covers and what it does not.
- Trust boundary today: A0 integrity and repository-policy evidence are in scope; identity-bound or execution-attestation claims are out of scope.
- Planned, non-implementation order:
  - Independent verifier and architecture split.
  - Slim GitHub receipt gate.
  - Distribution, installer, and package publishing controls.
  - One-command adoption and migration path.
  - Evidence publication and pre-push safety.
  - Cost analysis and broader qualification.

### Current PR focus

- PR1 repository hygiene: issue templates, PR template trust checklist, public roadmap entrypoint, and deterministic checks that keep repo templates safe from unsupported claims.

For implementation-level details, acceptance gates, and exact evidence requirements, see
[docs/PRODUCT_ROADMAP.md](docs/PRODUCT_ROADMAP.md).

Last updated: 2026-08-13.
