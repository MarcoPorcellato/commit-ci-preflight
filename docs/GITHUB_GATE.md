# Lightweight GitHub receipt gate

## Purpose

The repository-native workflow verifies locally produced evidence without
re-running the project's heavy checks on GitHub-hosted compute. It retains only
the remote facts that the local runner cannot assert: the pull-request event,
the exact head commit, the base repository, and publication of a GitHub commit
status.

The gate builds the Rust verifier from the trusted base revision. It never
checks out, builds, imports, sources, or executes pull-request code. The
evidence checkout is treated as untrusted data and is limited to the canonical
receipt path. The verifier rejects malformed, oversized, stale, digest-invalid,
or policy-invalid input.

## Why the receipt uses a separate branch

A receipt for commit X cannot be added to commit X: adding the file creates a
new commit. Receipt transport is therefore out-of-band. Version 1 uses one
branch per attested commit:

~~~text
ccp-evidence/<40-character-head-sha>
└── .ccp/receipt.json
~~~

The evidence commit may descend from the attested source commit, but the
receipt must remain bound to the source commit, not the evidence commit.
Evidence branches are append-once by convention. Configure a GitHub ruleset for
ccp-evidence/** that blocks force-pushes and deletion when the repository's
plan supports it.

This transport provides integrity and repository-policy evidence, not producer
identity. A repository writer could replace or fabricate unsigned evidence.
Identity-bound signing remains a later plan tranche and must not be inferred
from a green v1 status.

## Repository setup

The active workflow is
[receipt-gate.yml](../.github/workflows/receipt-gate.yml). It uses:

- pull_request_target, so the workflow definition comes from the default
  branch;
- contents: read to retrieve the trusted base and evidence branch;
- statuses: write only to publish commit-ci-preflight/receipt on the exact
  pull-request head SHA;
- a six-minute timeout and per-PR concurrency cancellation;
- actions/checkout pinned to a full commit SHA;
- no Actions cache, secret, deployment credential, Docker invocation, or
  project test command.

After this bootstrap workflow is merged, make
commit-ci-preflight/receipt a required status only after one successful
end-to-end trial has proved that the status is attached to the latest PR head
commit. Keep review, permission, secret-backed, deployment, and uncovered
platform checks as separate GitHub rules or workflows.

The active workflow is specific to this repository because its trusted base
contains the CCP Rust verifier source. An adopting repository must use the
[cross-repository template](../examples/github/receipt-gate.yml.example), pin
an exact reviewed CCP source commit, and keep its own policy in the adopting
repository's trusted base. Follow the [adoption guide](ADOPTION_GUIDE.md);
copying this repository-native workflow unchanged will not work safely.

This boundary follows GitHub's official guidance for
[`pull_request_target`](https://docs.github.com/en/actions/reference/security/securely-using-pull_request_target):
do not execute untrusted pull-request content, restrict token permissions, and
pin reusable actions to full commit SHAs. The event is used here because the
gate needs trusted base policy and permission to publish an exact-head status;
it is never used to test the pull-request checkout.

## Produce the repository receipt

Start from a clean source commit and a working Docker-compatible runtime. The
repository configuration intentionally enables network access because a clean
Rust dependency cache and pinned toolchain may need an initial download. Cargo
and Rustup paths are persistent managed caches. The bounded format timeout also
covers that first toolchain provisioning; warm format runs remain sub-second.

~~~console
export CARGO_HOME=.ccp-mounts/cargo-home
export CARGO_TARGET_DIR=.ccp-mounts/cargo-target
export RUSTUP_HOME=.ccp-mounts/rustup-home
cargo run --locked -- run \
  --config .commit-ci-preflight.toml \
  --repository . \
  --generation 1
cargo run --locked -- verify \
  --receipt .ccp/receipt.json \
  --policy .commit-ci-policy.toml \
  --expected-commit "$(git rev-parse HEAD)"
~~~

The root policy currently accepts genuine Apple Silicon macOS execution
through the Docker-compatible adapter. It does not claim Linux-host-native,
Windows-native, GitHub-hosted, or identity evidence.

The source checkout stays read-only, and Docker's integrated init process
reaps terminated descendants so process-supervisor checks remain truthful in a
container. The test command sets `CCP_TEST_ROOT` to a dedicated managed cache
mount. Nested CLI fixtures also derive their Linux `XDG_CACHE_HOME` from that
test root so the read-only container never needs a writable root filesystem.
These overrides exist only in test helpers and never weaken production
cache-path validation. The four persistent managed cache locations live below
the operator-selected cache root and may be inventoried with
`commit-ci-preflight cache inventory --json`.

## Publish evidence before opening or updating the PR

Use a separate worktree so the source checkout and index remain untouched.
These commands create a new evidence branch without force-pushing:

~~~console
source_sha=$(git rev-parse HEAD)
evidence_dir=$(mktemp -d)
git worktree add --detach "$evidence_dir" "$source_sha"
git -C "$evidence_dir" switch -c "ccp-evidence/$source_sha"
mkdir -p "$evidence_dir/.ccp"
cp .ccp/receipt.json "$evidence_dir/.ccp/receipt.json"
git -C "$evidence_dir" add -f .ccp/receipt.json
git -C "$evidence_dir" commit -m "evidence: local preflight for $source_sha"
git -C "$evidence_dir" push origin \
  "HEAD:refs/heads/ccp-evidence/$source_sha"
git worktree remove "$evidence_dir"
~~~

Publish this evidence branch before pushing the source branch update or opening
the pull request. The workflow then reads only .ccp/receipt.json from the exact
SHA-derived branch.

The final git worktree remove is cleanup of the explicitly created temporary
worktree. If an earlier command fails, inspect and remove only that exact
worktree after confirming it contains no work that must be preserved.

## Fork and untrusted-contributor policy

Fork authors cannot publish evidence into the base repository. Their pull
requests therefore fail closed until a maintainer:

1. reviews the source without running it in a privileged workflow;
2. reproduces the local checks on an accepted machine;
3. independently publishes the evidence branch in the base repository.

Never accept a fork-provided executable, workflow, cache, policy, or receipt as
trusted merely because it is attached to a pull request. The gate never exposes
secrets and never executes fork code under pull_request_target.

## Failure behavior and cost boundary

Missing branches, missing or symbolic-link receipts, inputs over one MiB,
trusted-checkout failures, verifier build failures, and any verifier rejection
produce a failing commit status. The job summary contains only the verifier's
bounded, non-sensitive findings. No raw project output or environment value is
uploaded.

The only cold remote compilation is the small trusted verifier. Project tests,
Docker checks, and local dependency caches remain off GitHub. A future signed
release can replace this bootstrap build with a pinned verifier download after
release publication is separately authorized and qualified.
