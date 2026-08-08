# Security Policy

## Supported versions

Commit CI Preflight is pre-alpha. No release is currently supported for
security-sensitive production use.

## Reporting a vulnerability

Use GitHub private vulnerability reporting when it is enabled for this
repository. Do not open a public issue containing exploit details, secrets,
private repository data, receipt contents from proprietary projects, or
personal information.

If private reporting is temporarily unavailable, open a public issue that only
states that a private security contact is needed. Do not include technical
details until a private channel has been established.

## Security boundaries

- A successful local check is evidence, not an identity guarantee.
- Receipts must not contain environment values, tokens, repository contents,
  absolute home-directory paths, or unredacted command output by default.
- Container execution is not assumed to be a complete hostile-code sandbox.
- Remote verification must fail closed on malformed, unsupported, stale, or
  commit-mismatched receipts.
- Signing, key custody, and hosted attestations require a dedicated threat
  model and ADR before implementation.

