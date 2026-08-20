# Adoption guide for another repository

## Purpose

This is the operator runbook for introducing Commit CI Preflight (CCP) into an
existing Git repository. It connects installation, configuration, local
execution, receipt verification, evidence publication, and the lightweight
GitHub gate into one reviewable path.

CCP moves reproducible, heavy checks to developer-owned hardware. It does not
replace code review, branch protection, trusted-secret jobs, deployments, or
native-platform checks that the local environment did not execute.

## Adoption outcome

A complete adoption adds these reviewed files to the target repository:

```text
.commit-ci-preflight.toml          local execution contract
.commit-ci-policy.toml             receipt acceptance policy
.github/workflows/receipt-gate.yml small remote verification gate
.gitignore                         local receipt/cache exclusions
```

The local operator also keeps a persistent CCP cache outside temporary
directories. Each tested source commit gets an out-of-band evidence branch:

```text
ccp-evidence/<40-character-source-sha>/.ccp/receipt.json
```

## 1. Decide what remains on GitHub

Classify every existing CI job before migrating it.

| Workload | Recommended location |
|---|---|
| Format, lint, compile, unit/integration tests in a pinned Linux image | Local CCP |
| Tests that need a GitHub event, repository permission, or protected environment | GitHub |
| Secret-backed tests, signing, release, deployment, or provenance publication | GitHub or another trusted control plane |
| Genuine Windows/macOS behavior | Native machine or retained native runner |
| Third-party actions with no deterministic command equivalent | Keep remote until separately redesigned |

Use the inert migration assistant for orientation:

```console
commit-ci-preflight migrate-github-actions \
  --workflow .github/workflows/ci.yml \
  --json
```

The report never executes the workflow and is not an executable conversion.
Review every command, image, secret, platform, and trust assumption manually.
See [GitHub Actions compatibility](GITHUB_ACTIONS_COMPATIBILITY.md).

## 2. Install a reviewed CCP binary

The current prerelease has no signed package channel. Build and test a reviewed
source revision as documented in [installation](INSTALLATION.md):

```console
git clone https://github.com/MarcoPorcellato/commit-ci-preflight.git
cd commit-ci-preflight
git switch --detach <reviewed-ccp-commit>
cargo test --locked --workspace --all-targets --all-features
cargo install --locked --path .
commit-ci-preflight --version
```

Record the exact CCP commit. Use the same reviewed revision in the GitHub gate
template; a moving branch or unpinned binary weakens the trust boundary.

## 3. Author the local execution contract

Copy the closest public fixture as a starting point:

- [Rust fixture](../examples/projects/rust/.commit-ci-preflight.toml)
- [Python fixture](../examples/projects/python/.commit-ci-preflight.toml)
- [Node fixture](../examples/projects/node/.commit-ci-preflight.toml)

Save the reviewed result as `.commit-ci-preflight.toml`. At minimum, replace:

- `project` with a stable logical `owner/repository` identity;
- `runtime.image` with an image pinned by `sha256` digest;
- resource limits with bounded values appropriate for the project;
- `network` with the narrowest truthful setting;
- checks with explicit, shell-free argv arrays and timeouts;
- cache and artifact paths with repository-relative, non-overlapping paths.

CCP mounts the repository read-only. Every declared cache destination must
already exist as a real directory in the checkout, and every declared artifact
destination as a real file. Ignore these generated placeholders in Git where
appropriate; do not make the repository mount writable to avoid this contract.

The complete field contract is in [configuration](CONFIGURATION.md). Resolve
the immutable image digest through the image publisher or registry tooling;
never replace the digest with `latest` or another mutable tag.

## 4. Select persistent local storage

Do not put the cache under `/tmp`, `/private/tmp`, or another reboot-cleaned
directory. Either use CCP's platform default or an explicit persistent path:

```console
commit-ci-preflight cache path
commit-ci-preflight cache init --repository .
commit-ci-preflight cache inventory --json
```

With an explicit location, pass the same absolute path to `dry-run`, `run`, and
cache commands:

```console
commit-ci-preflight cache init \
  --repository . \
  --cache-dir /absolute/persistent/ccp-cache
```

The platform-wide admission coordinator is intentionally separate from the
build cache. Multiple repositories share its single heavy-work slot. See
[cache and workspace](CACHE_AND_WORKSPACE.md). If several agent activities or
repositories use the same machine, read the [cross-activity coordination
runbook](COORDINATION_RUNBOOK.md) before starting a heavy command. It is the
authoritative procedure for status interpretation, owner handoff, worktree
isolation, and safe recovery.

## 5. Inspect without executing project checks

Run the read-only gates first:

```console
commit-ci-preflight plan --config .commit-ci-preflight.toml --json
commit-ci-preflight doctor --config .commit-ci-preflight.toml --json
commit-ci-preflight dry-run \
  --config .commit-ci-preflight.toml \
  --repository . \
  --json
```

Review the normalized plan and its configuration digest. In the dry run,
confirm the exact image, command argv, read-only source mount, declared writable
paths, CPU/memory/PID limits, and network mode.

`doctor` probes the Docker-compatible runtime only. On Apple Silicon macOS,
OrbStack is the qualified complete local path for the current prerelease. Other
Docker-compatible engines may work but remain compatible-unqualified unless
separately evidenced. See [beta support](BETA_SUPPORT.md).

## 6. Run on an exact clean commit

CCP refuses an uncommitted source state because the receipt must bind exact
bytes represented by a Git commit:

```console
git status --short --branch
git rev-parse HEAD
commit-ci-preflight admission status --json
commit-ci-preflight resource status --json
commit-ci-preflight run \
  --config .commit-ci-preflight.toml \
  --repository . \
  --generation 1 \
  --admission-timeout-seconds 21600
```

Increase `generation` monotonically when an orchestrator supersedes an older
attempt. Do not use generation to disguise a changed commit or configuration.

On success, the configured output (normally `.ccp/receipt.json`) is created
atomically. CCP does not overwrite an unrelated valid result silently and does
not place raw command output, environment values, source contents, personal
identity, or absolute home paths in the receipt.

Do not start this command from an activity that sees another active or queued
owner. `resource status` is only a point-in-time admission sample; the
host-wide CCP slot is acquired by the command itself. Repeat both resource and
admission checks immediately before the run and record the exact source SHA.
Never infer global inactivity from a missing process in one terminal.

## 7. Create and pin repository policy

Generate or copy a strict `.commit-ci-policy.toml` only after reviewing the
normalized plan. The policy pins:

- logical project identity;
- configuration digest;
- exact required-check set;
- immutable image reference;
- accepted host OS, architecture, and runtime kind;
- maximum receipt age.

For a new trusted receipt-v2 integration, use policy `1.1` and additionally
pin a safe policy-relative `trusted_config`, one source-snapshot strategy, and
the exact supported producer name/version. Policy `1.1` rejects a receipt v1
instead of silently treating it as a trusted plan. The v1 public fixture remains
useful only as a historical compatibility example; see
[verification policy](VERIFICATION_POLICY.md) for the normative v1.1 form.

Use [the public policy fixture](../examples/projects/rust/.commit-ci-policy.toml)
as syntax reference and [verification policy](VERIFICATION_POLICY.md) as the
normative field guide. Values must come from the reviewed configuration and
truthful local receipt, not from assumptions.

Verify independently:

```console
source_sha="$(git rev-parse HEAD)"
commit-ci-preflight verify \
  --receipt .ccp/receipt.json \
  --policy .commit-ci-policy.toml \
  --expected-commit "$source_sha" \
  --json
```

Exit code `0` means receipt integrity and repository policy passed. It does not
prove who controlled the producer machine. See [receipt specification](RECEIPT_SPEC.md)
and [threat model](THREAT_MODEL.md).

## 8. Install the cross-repository GitHub gate

The workflow active in the CCP repository compiles the verifier from its own
trusted base and therefore must not be copied unchanged into another project.
For an adopting repository, copy the inactive template:

```console
mkdir -p .github/workflows
cp /path/to/commit-ci-preflight/examples/github/receipt-gate.yml.example \
  .github/workflows/receipt-gate.yml
```

Before committing it:

1. replace `REPLACE_WITH_REVIEWED_CCP_COMMIT` with the 40-character CCP source
   commit reviewed in step 2;
2. verify the pinned `actions/checkout` commit and update it only through a
   separate dependency review;
3. retain `pull_request_target`, least-privilege permissions, and the rule that
   no pull-request code is executed;
4. keep `.commit-ci-policy.toml` in the target repository's trusted base;
5. review branch protection and fork behavior in [GitHub gate](GITHUB_GATE.md).

The template checks out three independent inputs: target repository trusted
base, pinned CCP verifier source, and the exact SHA-derived evidence branch.
Only the trusted verifier is built. Neither pull-request code nor evidence data
is executed.

GitHub's official guidance warns that `pull_request_target` has elevated trust
and must not build or execute untrusted pull-request content. It also recommends
least-privilege tokens and full-length commit pinning for actions. CCP uses this
event only to read trusted base policy, treat the receipt as bounded data, and
publish status on the exact PR head:

- [Securely using `pull_request_target`](https://docs.github.com/en/actions/reference/security/securely-using-pull_request_target)
- [Secure use reference](https://docs.github.com/en/actions/reference/security/secure-use)
- [GitHub Actions policy and SHA pinning](https://docs.github.com/en/enterprise-cloud/latest/admin/enforcing-policies/enforcing-policies-for-your-enterprise/enforcing-policies-for-github-actions-in-your-enterprise)

Do not make `commit-ci-preflight/receipt` required until one end-to-end trial
has proved the status is attached to the latest pull-request head SHA.

## 9. Publish evidence without changing the source commit

Run and verify locally before publishing. Then use an isolated worktree:

```console
source_sha="$(git rev-parse HEAD)"
evidence_dir="$(mktemp -d)"
git worktree add --detach "$evidence_dir" "$source_sha"
git -C "$evidence_dir" switch -c "ccp-evidence/$source_sha"
mkdir -p "$evidence_dir/.ccp"
cp .ccp/receipt.json "$evidence_dir/.ccp/receipt.json"
git -C "$evidence_dir" add -f .ccp/receipt.json
git -C "$evidence_dir" commit -m "evidence: local preflight for $source_sha"
git -C "$evidence_dir" push origin \
  "HEAD:refs/heads/ccp-evidence/$source_sha"
git worktree remove "$evidence_dir"
```

Never force-push an evidence branch. If publication stops midway, preserve and
inspect the exact worktree before removing it. Configure a GitHub ruleset for
`ccp-evidence/**` that blocks force-pushes and deletion where practical.

Publish the evidence branch before opening or updating the source pull request.
The gate derives the expected commit from the trusted GitHub event and fails
closed if exact evidence is absent, stale, malformed, or policy-invalid.

## 10. Repository hygiene

Recommended ignore entries for local-only state are:

```gitignore
.ccp/receipt.json
.ccp-mounts/
```

Do not ignore `.commit-ci-preflight.toml`, `.commit-ci-policy.toml`, or the
workflow. Do not commit dependency caches, runtime layers, raw logs, secrets,
or machine-specific absolute paths.

## 11. Rollout gates

Adopt CCP incrementally:

1. **Observe:** run locally while existing GitHub CI remains authoritative.
2. **Compare:** require repeatable local receipts and compare failures for
   several representative commits.
3. **Gate:** enable the lightweight receipt status but retain remote heavy CI.
4. **Reduce:** remove only remote jobs fully covered by reviewed local checks;
   retain trust- and platform-specific jobs.
5. **Review:** periodically audit image digests, CCP revision, policies,
   freshness, native coverage, cache size, and GitHub cost.

Rollback is simple: stop requiring the receipt status and re-enable the prior
remote jobs. Preserve receipts and configurations for incident analysis. See
[upgrade and rollback](UPGRADE_AND_ROLLBACK.md).

## Adoption checklist

- [ ] Exact CCP source commit reviewed, tested, and recorded.
- [ ] Existing workflows classified into local, remote-trust, and native gates.
- [ ] Runtime image pinned by digest.
- [ ] Configuration passes `plan`, `doctor`, and reviewed `dry-run`.
- [ ] Cache uses persistent storage and has valid ownership.
- [ ] Exact source commit is clean.
- [ ] Local `run` completes and writes a receipt.
- [ ] Strict policy matches the reviewed plan and accepted platform.
- [ ] Independent local `verify` returns exit code 0.
- [ ] Cross-repository workflow pins CCP and checkout revisions.
- [ ] Evidence branch is exact-SHA, append-once, and published first.
- [ ] GitHub status is observed on the latest PR head.
- [ ] Review, secrets, deployments, and uncovered native gates remain remote.
- [ ] Rollback path is documented and tested.
