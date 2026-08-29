# Case study: Commit CI Preflight PR #71

## What problem this demonstrates

CCP's `dry-run` output is a planning surface, not a replay bundle. A copied
Docker command can reference lifecycle-managed cache sources that no longer
exist; the diagnostic must therefore use its own writable, independently
validated mounts.

## Exact public anchors

This case study concerns [PR #71](https://github.com/MarcoPorcellato/commit-ci-preflight/pull/71)
at head `f3fb14a0329cf031d08f474115a8b56fe1cffcf6`, merged by commit
`820a7fa6ce83a7ac8593c2800f8be4f44ab82ebc`.

The append-only evidence ref resolves to commit
`382ba81a6869777a92f987068e2814f542e7ec8b`. Its public receipt has SHA-256
`624477439b1bdb7b397876a7e5f8434057f8805835162c5fccb21beac015000b` and
receipt ID `sha256:3536dce784a646f6450f6a228230e09ffff8d4d5bd45f89dfd285223a9a235d1`.
The associated [workflow run](https://github.com/MarcoPorcellato/commit-ci-preflight/actions/runs/33220815044)
completed successfully.

## What happened

The workflow followed a bounded lifecycle: local qualification produced the
receipt; evidence was published append-only; GitHub checked the receipt against
the exact pull-request head; and the pull request was merged. The public
[evidence ref](https://github.com/MarcoPorcellato/commit-ci-preflight/tree/ccp-evidence/f3fb14a0329cf031d08f474115a8b56fe1cffcf6)
and [receipt specification](RECEIPT_SPEC.md) describe the inspectable artifacts.

## What this proves — and does not prove

The receipt integrity and policy checks, together with the exact-head gate,
prove the recorded local evidence for this exact commit. They do not prove
producer identity, a signature, cost savings, arbitrary hosted-CI parity, or
qualification of other platforms. They also do not turn a receipt into an
identity attestation.

The [verification policy](VERIFICATION_POLICY.md) defines these boundaries.
