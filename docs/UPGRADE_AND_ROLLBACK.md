# Upgrade, rollback, and uninstall

## Scope

This runbook covers source-built and local release-candidate installations of
Commit CI Preflight 0.1.x. No public package channel exists yet. An upgrade is a
binary replacement; configuration, receipt, and cache schemas remain separately
versioned and must fail closed when unsupported.

## Before an upgrade

1. Record the current binary and source commit:

   ```console
   commit-ci-preflight --version
   git rev-parse HEAD
   ```

2. Verify the candidate checksum using
   [the installation guide](INSTALLATION.md).
3. Save the current executable as a rollback artifact outside the repository.
4. Keep the current configuration and policy under version control.
5. Inventory the managed cache without deleting anything:

   ```console
   commit-ci-preflight cache path
   commit-ci-preflight cache inventory --json
   ```

6. Run `plan`, `doctor`, and `dry-run` with the candidate before replacing
   the current binary.

Never upgrade while another `run` uses the same managed cache root.

## Source-built upgrade

Build and test the reviewed candidate:

```console
git status --short --branch
git rev-parse HEAD
cargo test --locked --workspace --all-targets --all-features
cargo build --locked --release
./target/release/commit-ci-preflight --version
```

Install to an isolated prefix first:

```console
cargo install --locked --path . --root /absolute/candidate/prefix
/absolute/candidate/prefix/bin/commit-ci-preflight --version
```

Only replace an operator-visible binary after the isolated smoke test passes.
Preserve the previous executable under a distinct absolute path. Do not edit a
binary in place while it is running.

## Configuration compatibility

The candidate currently supports configuration, receipt, policy, verification
report, and benchmark schema version `1.0`. Unknown versions and fields fail
closed. Before upgrading:

- compare the candidate's `plan --json` output with the accepted plan;
- update the repository policy only after reviewing the new configuration
  digest;
- regenerate a receipt for the exact new commit;
- never reuse an older receipt as evidence for a newer commit.

The GitHub Actions migration report is inert and cannot be promoted directly
into executable configuration.

## Rollback

If the candidate fails its smoke test or changes an accepted plan:

1. stop the candidate process;
2. restore the preserved previous executable atomically;
3. confirm its version;
4. restore the previous committed configuration and policy;
5. run `plan` and `doctor`;
6. generate a fresh receipt for the current source commit.

Example with explicit paths:

```console
mv /absolute/bin/commit-ci-preflight /absolute/quarantine/failed-candidate
mv /absolute/backup/commit-ci-preflight /absolute/bin/commit-ci-preflight
/absolute/bin/commit-ci-preflight --version
```

Do not delete the failed candidate until the incident is understood. Never
reinterpret a candidate receipt as rollback evidence.

## Cache behavior during rollback

The managed cache is deliberately independent of the installed binary and is
not removed during rollback. Versioned ownership and completion markers make
unsupported or incomplete entries fail closed. If a prior binary cannot read a
newer marker, choose a new explicit cache root instead of editing marker bytes.

The default build-cache namespace is `commit-ci-preflight-build-v1`. Earlier
pre-release layouts may have left admission coordination files under the old
`commit-ci-preflight` cache directory. Upgrades do not adopt, move or delete
that legacy state. Use an explicitly selected old root only after confirming
that it carries a valid CCP ownership marker; otherwise keep it untouched and
use the versioned default.

Cleanup remains preview-only in 0.1.0:

```console
commit-ci-preflight cache cleanup --dry-run
```

Do not recursively remove a cache root unless its exact path, ownership marker,
active-run state, retention decision, and recovery impact have been reviewed.
See [the cache contract](CACHE_AND_WORKSPACE.md).

## Uninstall

Default Cargo installation:

```console
cargo uninstall commit-ci-preflight
```

Isolated installation:

```console
cargo uninstall --root /absolute/prefix commit-ci-preflight
```

Uninstall does not remove:

- repository configuration or policy;
- `.ccp/receipt.json`;
- evidence branches;
- managed cache data;
- locally built candidate archives.

These remain independent operator-owned artifacts and require separate,
explicit retention decisions.

## Qualification receipt

PR10 tests the following without publishing anything:

- isolated `cargo install --path` and version smoke test;
- local candidate archive creation;
- SHA-256 verification;
- required archive member inventory;
- generator `--check` parity for the SPDX SBOM and third-party notices.

A public tag, package, signature, or GitHub Release remains a separate
authorization gate.
