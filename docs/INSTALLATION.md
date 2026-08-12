# Installation and artifact verification

## Release status

Commit CI Preflight `v0.1.0-rc.1` is published as a GitHub prerelease. Its
reviewed assets are an unsigned macOS arm64 archive and `SHA256SUMS`; GitHub
also generates source archives from the exact tag. There is no crate, Homebrew
formula, Winget package, container image, or signed artifact. Download only
from the
[`v0.1.0-rc.1` release page](https://github.com/MarcoPorcellato/commit-ci-preflight/releases/tag/v0.1.0-rc.1)
or build from a reviewed source commit.

## Prerequisites

- Git;
- Rust 1.87 or newer, with the repository-pinned toolchain recommended;
- a Docker-compatible runtime for `doctor`, `dry-run`, and `run`;
- macOS arm64 with OrbStack for the currently qualified complete local path.

The CLI itself is Rust and builds on macOS, Linux, and Windows. Native benchmark
qualification is recorded in
[`evidence/pr09/`](evidence/pr09/README.md). That benchmark evidence does not
claim that every runtime path is qualified on every platform.

## Install from a reviewed source checkout

Clone and inspect the exact commit before installing:

```console
git clone https://github.com/MarcoPorcellato/commit-ci-preflight.git
cd commit-ci-preflight
git status --short --branch
git rev-parse HEAD
cargo test --locked --workspace --all-targets --all-features
cargo install --locked --path .
commit-ci-preflight --version
```

`cargo install --path .` installs only the `commit-ci-preflight` binary.
It does not register a GitHub runner, start a daemon, alter repository settings,
or upload a receipt.

To isolate a test installation:

```console
cargo install --locked --path . --root /absolute/test/prefix
/absolute/test/prefix/bin/commit-ci-preflight --version
```

On Windows PowerShell, the binary is under
`C:\absolute\test\prefix\bin\commit-ci-preflight.exe`.

## Build a local release candidate

The bounded packaging script builds the current host target and never publishes
anything:

```console
scripts/build_release_candidate.sh /absolute/output/directory
```

It creates:

- one `commit-ci-preflight-v0.1.0-<target>.tar.gz` archive;
- `SHA256SUMS` for that exact archive.

The archive contains the host binary, `LICENSE`, `NOTICE`, `README.md`, the SPDX
SBOM, third-party notices, adoption, installation, troubleshooting, rollback,
threat-model, support, and tutorial documents, plus the inactive
cross-repository GitHub gate template. The script refuses a relative output
path, checks that release metadata is current, builds with `--locked`, and does
not tag, push, upload, sign, or publish.

## Verify checksums

macOS:

```console
cd /absolute/output/directory
shasum -a 256 -c SHA256SUMS
```

Linux:

```console
cd /absolute/output/directory
sha256sum -c SHA256SUMS
```

Windows PowerShell can compare the expected first field in `SHA256SUMS` with:

```powershell
Get-FileHash .\commit-ci-preflight-v0.1.0-<target>.tar.gz -Algorithm SHA256
```

A matching checksum proves only byte integrity relative to the separately
obtained checksum file. It does not establish publisher identity. Release
signing remains intentionally out of scope until key custody has its own ADR
and authorization.

## First smoke test

```console
commit-ci-preflight --version
commit-ci-preflight plan --config .commit-ci-preflight.toml
commit-ci-preflight doctor --config .commit-ci-preflight.toml
commit-ci-preflight dry-run --config .commit-ci-preflight.toml
```

`doctor` is a bounded read-only runtime probe. `dry-run` renders explicit
argv and mounts but does not execute checks. Follow the
[end-to-end tutorial](TUTORIAL.md) before using the tool on an important
repository.

## Uninstall

For the default Cargo installation:

```console
cargo uninstall commit-ci-preflight
```

For an isolated root:

```console
cargo uninstall --root /absolute/test/prefix commit-ci-preflight
```

Uninstalling the binary does not delete project receipts or the managed cache.
See [upgrade and rollback](UPGRADE_AND_ROLLBACK.md) and
[cache ownership](CACHE_AND_WORKSPACE.md) before removing any data.
