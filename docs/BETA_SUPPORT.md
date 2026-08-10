# Beta limitations and support policy

## Current status

Commit CI Preflight 0.1.0 is a source-available release candidate under
Apache-2.0. It is not yet a published release and is not approved for
security-sensitive production enforcement without an operator review of the
threat model, policy, and local runtime.

## Qualification matrix

| Surface | Status | Meaning |
|---|---|---|
| Rust format, test, Clippy, and docs on macOS arm64 through OrbStack | `QUALIFIED` | Complete repository preflight and commit-bound receipt passed locally |
| Fixed benchmark on native macOS arm64 | `QUALIFIED` | Deterministic correctness digest matched |
| Fixed benchmark on native Linux x86_64 | `QUALIFIED` | Standard `ubuntu-24.04` runner receipt independently verified |
| Fixed benchmark on native Windows x86_64 | `QUALIFIED` | Standard `windows-2025` runner receipt independently verified |
| Complete project `run` path on Linux x86_64 | `PENDING` | Benchmark evidence is not full runtime qualification |
| Complete project `run` path on Windows x86_64 | `PENDING` | Benchmark evidence is not full runtime qualification |
| Docker Desktop and other Docker-compatible engines | `COMPATIBLE_UNQUALIFIED` | Adapter contract exists; no complete platform receipt is claimed |
| Identity-bound attestation or signing | `NOT_IMPLEMENTED` | Receipts prove integrity and policy, not producer identity |
| Public packages and signed release artifacts | `NOT_PUBLISHED` | Separate authorization and key-custody decisions are required |

Exact native receipts and claim boundaries are in
[the PR09 evidence matrix](evidence/pr09/README.md).

## Supported beta behavior

The beta candidate implements:

- strict TOML configuration and deterministic plan normalization;
- bounded Docker-compatible capability probing and explicit argv rendering;
- clean-commit local execution in a pinned container image;
- process timeout, cancellation, output bounds, and stale-generation guards;
- canonical commit-bound receipts;
- independent integrity and repository-policy verification;
- persistent cache ownership, inventory, and preview-only cleanup;
- a lightweight GitHub receipt gate;
- a read-only, non-executing GitHub Actions migration assistant;
- deterministic benchmark and native evidence contracts.

## Intentional limitations

The beta does not:

- emulate all GitHub Actions syntax or expression semantics;
- download or execute marketplace actions;
- mount the Docker socket inside project checks;
- claim that containers are a complete sandbox for hostile code;
- upload logs, source, environment values, or secrets by default;
- establish who produced a receipt;
- delete cache entries;
- sign artifacts;
- publish packages, tags, or releases automatically;
- replace remote review, permissions, protected-branch policy, deployment
  environments, trusted secrets, or genuinely uncovered native checks.

The GitHub gate trusts reviewed verifier code from the base branch and treats
the evidence branch as untrusted data. Fork contributors cannot directly
publish trusted evidence into the base repository.

## Data and privacy

Receipts omit command output bodies, environment values, absolute home paths,
usernames, email addresses, hostnames, IP addresses, and repository contents.
Only bounded status and digests cross the remote gate. Operators must still
review custom artifact paths and must not attach proprietary logs to public
issues.

## Compatibility policy

Schema version `1.0` is fail-closed. A future schema or behavior change must
ship with:

- an explicit version transition;
- deterministic fixtures and migration guidance;
- updated threat model and support matrix;
- fresh native evidence where platform behavior changes.

Pre-release compatibility may change, but silent reinterpretation of an
existing receipt is prohibited.

## Support channels

Use public GitHub issues for reproducible, non-sensitive bugs and documentation
requests. Follow `SECURITY.md` for vulnerabilities and never post exploit
details, credentials, proprietary source, private receipts, or personal data in
a public issue.

Support is best-effort during the beta. No uptime, response-time, savings,
parity, fitness-for-purpose, or legal-compliance warranty is offered beyond the
Apache-2.0 license.

## Exit from beta

A public 0.1.0 release requires all Definition-of-Done evidence in
[`IMPLEMENTATION_PLAN.md`](IMPLEMENTATION_PLAN.md), a final dependency and
license review, tested install/rollback artifacts, and separate owner
authorization for any tag publication, package upload, signature, or GitHub
Release.
