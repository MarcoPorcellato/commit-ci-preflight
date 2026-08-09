# Benchmark and parity evidence v1

## Status and claim boundary

The native benchmark contract is implemented in Rust. Platform qualification is
evidence-driven: a platform is `PASS` only after the fixed workload ran in a
native process on that operating system and architecture and the resulting
receipt passed independent verification.

Emulation, Rosetta, QEMU, and a Linux container on a non-Linux host are useful
compatibility observations, but they are never relabelled as native evidence.
Timings describe one bounded run; they are not universal performance promises.

## Fixed workload

`commit-ci-preflight benchmark` runs five samples of a 4,096-iteration canonical
JSON and SHA-256 chain. Every platform must produce the pinned final digest
`sha256:29ac09de518a019bd8c663b411f77bbab466c7cf7236b2f56c2dbb6b105c69dc`.

Correctness is deterministic. Nanosecond samples and their median are observed
measurements and are expected to vary. They never affect the expected result
digest. Receipts bind the exact source commit, process OS/architecture, optional
CI environment, optional Docker-compatible runtime probe, workload contract,
timings, and integrity ID.

The structural schema is pinned at
[`../schema/benchmark-v1.schema.json`](../schema/benchmark-v1.schema.json).
Semantic limits and the golden workload digest are enforced by the Rust
validator in addition to that schema.

The receipt is evidence, not producer identity. `ci_environment` is environment
metadata, not a cryptographic assertion that GitHub created the bytes.

## Native commands

macOS or Linux:

```console
scripts/run_native_benchmark.sh \
  "$(git rev-parse HEAD)" benchmark-receipts/native.json
```

Windows PowerShell 7:

```powershell
./scripts/run_native_benchmark.ps1 `
  -Commit (git rev-parse HEAD) `
  -Output benchmark-receipts/windows-x86_64.json
```

Mac Apple Silicon plus OrbStack capability evidence adds the existing runtime
probe without claiming that the native workload ran inside Linux:

```console
cargo run --locked -- benchmark \
  --commit "$(git rev-parse HEAD)" \
  --runtime-config .commit-ci-preflight.toml \
  --output benchmark-receipts/macos-arm64-orbstack.json \
  --json
```

Independent verification requires expectations from the calling trust boundary:

```console
cargo run --locked -- verify-benchmark \
  --receipt benchmark-receipts/macos-arm64-orbstack.json \
  --expected-commit "$(git rev-parse HEAD)" \
  --expected-os macos \
  --expected-arch aarch64 \
  --expected-runtime-flavor orbstack \
  --json
```

Output uses create-new publication. An existing receipt is never overwritten.
Unknown fields, oversized inputs, timing tampering, integrity drift, commit
mismatch, and platform mismatch fail closed.

## Opt-in public GitHub comparison

The `Native benchmark evidence` workflow runs only through an explicit manual
dispatch on the default branch or when a maintainer applies the
`native-benchmark` label to a same-repository pull request. It has
`contents: read`, checks out the exact event commit with credentials disabled, uses no
secret or cache, and uploads one short-lived JSON receipt per platform for one
day.

GitHub requires an event-triggered workflow file to exist on the default branch.
Therefore the first qualification is intentionally split: merge the reviewed
contract and workflow while every platform remains `PENDING`, manually dispatch
that exact default-branch commit, independently verify the downloaded receipts,
and add only those receipts in a follow-up evidence pull request. No bootstrap
run is inferred from a pull request that merely introduces this workflow.

The matrix is fixed to standard `ubuntu-24.04` x64 and `windows-2025` x64
runners. GitHub documents standard hosted-runner use as free for public
repositories and documents these labels as x64 platforms:

- <https://docs.github.com/en/billing/concepts/product-billing/github-actions>
- <https://docs.github.com/en/actions/reference/runners/github-hosted-runners>

This billing fact is current documentation, not a promise by this project.
Larger runners remain excluded. The workflow is opt-in to avoid needless runs
even when the monetary price is zero.

## Cost model

The repository uses an assumptions-first model:

```text
remote_monthly_minutes = pull_requests_per_month
                       * remote_runs_per_pull_request
                       * average_remote_minutes_per_run

remote_monthly_cost = remote_monthly_minutes
                    * billed_price_per_runner_minute

estimated_savings = baseline_remote_monthly_cost
                  - retained_control_plane_monthly_cost
                  - local_operating_cost_entered_by_operator
```

For this public comparison repository, the documented billed price of standard
hosted runners is zero. For a private production repository, operators must use
their current plan, runner multiplier, included quota, taxes, and local energy
or hardware assumptions. Commit CI Preflight does not hard-code a currency or
claim guaranteed savings.

Quality-critical remote responsibilities remain separate: event identity,
review/permission policy, secret-backed integration, deployment environments,
and native platforms not covered by accepted local receipts.

## Evidence directory

Actual PR09 receipts and GitHub run metadata are recorded under
[`evidence/pr09/`](evidence/pr09/) only after native execution. The matrix uses
`PASS`, `PENDING`, or `NOT_RUN` literally and never infers one platform from
another.
