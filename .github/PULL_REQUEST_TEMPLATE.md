## Summary

Briefly explain what changed and why.

## Trust claim checklist

- [ ] This pull request does **not** claim:
  - native execution where only contract-level qualification is available,
  - signed identity or full execution attestation when only A0 evidence is present,
  - zero-cost operation or equivalent absolute cost reductions.
- [ ] This pull request does not alter source, runtime, workflow, secrets, or policy behavior.
- [ ] If the proposal references `docs/PRODUCT_ROADMAP.md`, it does so for implementation details and not as a final customer statement.

## Evidence checklist

- [ ] Changed files are present and deterministic:
  - `.github/ISSUE_TEMPLATE/bug_report.yml`
  - `.github/ISSUE_TEMPLATE/feature_request.yml`
  - `.github/ISSUE_TEMPLATE/adoption_report.yml`
  - `.github/ISSUE_TEMPLATE/config.yml`
  - `.github/PULL_REQUEST_TEMPLATE.md`
  - `ROADMAP.md`
- [ ] `cargo test --test repository_hygiene_contract` passes.
- [ ] `git diff --check` has no whitespace or whitespace-related issues.
- [ ] The templates include links only to repository docs or implementation-neutral claims.

## Validation notes

Mention exact output, if any.
