# GitHub Actions compatibility contract v1

## Purpose

`commit-ci-preflight migrate-github-actions` is a read-only migration assistant.
It parses one public GitHub Actions workflow as untrusted data and emits a
versioned compatibility report. It never downloads an action, evaluates an
expression, reads a secret, executes a workflow command, or emits executable
Commit CI Preflight configuration.

```console
commit-ci-preflight migrate-github-actions \
  --workflow .github/workflows/ci.yml --json
```

Exit code `0` means the report contains no unsupported feature. It does not mean
parity is proven or that the proposed checks are ready to execute. Exit code `4`
means at least one feature is unsupported and migration is blocked. Malformed,
oversized, multi-document, duplicate-keyed, anchored, tagged, or aliased YAML
exits with usage code `2`.

## Normative classification table

| Workflow surface | v1 result | Safety rule |
|---|---|---|
| `actions/checkout@<40 lowercase hex>` | `translated` metadata | Recognized only; never executed |
| Mutable checkout reference | `manual_review` | Pin before translation |
| Literal environment name | `translated` name | Value is never serialized |
| Literal environment value | `manual_review` | Operator supplies the value explicitly |
| Environment expression or secret reference | `unsupported` | No expression evaluation or secret access |
| Plain `run` with literal `sh` or `bash` | `translated` proposal | Proposal remains inert data |
| Implicit shell | `manual_review` | Shell must be explicit |
| Other shell, conditional, timeout, or continue-on-error | `unsupported` | Semantics are not approximated |
| Literal `ubuntu-*` runner | `manual_review` | Operator selects a pinned Linux image |
| macOS, Windows, or runner expression | `unsupported` | No cross-platform relabelling |
| Recognized setup action metadata | `manual_review` | Reported but never downloaded or executed |
| Matrix, service, job dependency, or container | `manual_review` | Reported but not translated |
| Permissions, reusable workflow, action secrets, or outputs | `unsupported` | Trust semantics are fail-closed |
| Marketplace, Docker, or local action | `unsupported` | Arbitrary action execution is forbidden |
| Unknown key | `unsupported` | Schema drift is fail-closed |

Recognized setup metadata is limited to public setup families named in the Rust
implementation. Recognition is not endorsement and does not inspect action
source or inputs.

## Report contract

The JSON report has schema version `1.0`, deterministic ordering, a summary,
global environment names, inert check proposals, and one finding per observed
compatibility decision. `executable_config_emitted` is always `false` in v1.

`readiness` has three values:

- `ready_for_config_authoring`: every observed feature was translated;
- `manual_review_required`: no unsupported feature exists, but at least one
  explicit operator decision remains;
- `blocked`: at least one unsupported feature exists.

The report is not a receipt, signature, security attestation, or parity proof.
An operator must author and validate `.commit-ci-preflight.toml`, select a pinned
image, declare writable caches/artifacts, and run native evidence gates.

## Parser and resource boundary

Input is limited to one UTF-8 YAML document of at most 1 MiB, 64 jobs, and 256
steps per job. Duplicate mapping keys, complex keys, YAML anchors, tags, and
aliases are rejected through parser events before object loading. The
maintained `saphyr` parser family is used with no network or execution
capability. Public fixtures cover a supported subset, high-risk mixed input,
and reusable workflows.
