## Summary

Briefly explain what changed and why.

## Trust claim checklist

- [ ] Public claims in this pull request do **not** imply:
  - native execution where only contract-level qualification is available,
  - signed identity or full execution attestation when only A0 evidence is present,
  - zero-cost operation or equivalent absolute cost reductions.
- [ ] I described any source, runtime, workflow, permission, secret, policy, schema, or dependency impact below; write `none` only after checking the diff.
- [ ] Roadmap text is presented as planned work, not as implemented or qualified behavior.

## Impact and rollback

- Runtime or trust-boundary impact:
- Dependency or supply-chain impact:
- Data, secret, permission, or network impact:
- Rollback path:

## Evidence checklist

- [ ] I listed the exact focused checks run for this diff.
- [ ] I ran `cargo fmt --all -- --check` when Rust files changed.
- [ ] I ran the relevant contract tests and recorded any intentionally not-run gate.
- [ ] `git diff --check` passes.
- [ ] User-visible changes update `CHANGELOG.md`.
- [ ] New or changed links point to an existing, authoritative target.

## Validation notes

List commands, result counts, receipts, platform evidence, and explicit
`PENDING` or `NOT-RUN` items. Do not convert contract tests into native,
signed, billing, or deployment evidence.
