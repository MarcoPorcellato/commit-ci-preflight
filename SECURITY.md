# Security Policy

## Supported versions

Commit CI Preflight 0.1.0 is a source-built release candidate. No public
package or signed release is currently supported for security-sensitive
production use. Review the beta support matrix and threat model before treating
a receipt as an enforcement signal.

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

The complete implemented-control and residual-risk review is in
[`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md). Platform claims and pending
qualification are in [`docs/BETA_SUPPORT.md`](docs/BETA_SUPPORT.md).

