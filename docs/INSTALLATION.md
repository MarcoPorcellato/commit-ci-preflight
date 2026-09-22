# Installation and artifact verification

## Current prerelease status

A [published GitHub prerelease](https://github.com/MarcoPorcellato/commit-ci-preflight/releases)
is available for macOS arm64. Start by verifying the desired archive's adjacent
`SHA256SUMS` and its in-archive `RELEASE_MANIFEST.json`. GitHub also provides
source archives for each exact tag. There is no crate, Homebrew formula, Winget
package, container image, or signed artifact.

The unsigned macOS arm64 archive is byte-integrity-verifiable, not signed.
Build from a reviewed source commit instead when the prerelease artifact does
not fit your platform or trust requirements.

## Use the published macOS arm64 prerelease

This path needs only macOS arm64, `tar`, and `shasum`. From
[GitHub Releases](https://github.com/MarcoPorcellato/commit-ci-preflight/releases),
choose one prerelease and download both
`commit-ci-preflight-<release-label>-aarch64-apple-darwin.tar.gz` and its
adjacent `SHA256SUMS` into an empty directory you control.

```console
cd /absolute/download/directory
release_label=v0.1.0-rc.N
archive="commit-ci-preflight-${release_label}-aarch64-apple-darwin.tar.gz"
if ! shasum -a 256 -c SHA256SUMS; then
  echo "Checksum verification failed; refusing to extract or execute the archive." >&2
  exit 1
fi
tar -xzf "$archive"
cd "${archive%.tar.gz}"
sed -n '1,120p' RELEASE_MANIFEST.json
./commit-ci-preflight --version
```

The manifest identifies the release label, source commit, Cargo package
version, target, and archive name. A matching checksum proves only byte
integrity relative to the separately obtained checksum file; it does not sign
the asset or prove producer identity.

To make the verified binary available from a user-controlled directory, copy
only that extracted binary after the check succeeds:

```console
install -m 0755 ./commit-ci-preflight /absolute/user-controlled/bin/commit-ci-preflight
/absolute/user-controlled/bin/commit-ci-preflight --version
```

This does not start a daemon, alter repository settings, register a GitHub
runner, or execute project checks. Do not replace a separately qualified CCP
installation without its own rollback and authorization procedure.

## Prerequisites for source builds and complete local runs

- Git;
- Rust 1.87 or newer, with the repository-pinned toolchain recommended;
- a Docker-compatible runtime for `doctor`, `dry-run`, and `run`;
- macOS arm64 with OrbStack for the currently qualified complete local path.

The CLI itself is Rust and builds on macOS, Linux, and Windows. Native benchmark
qualification is recorded in
[`evidence/pr09/`](evidence/pr09/README.md). That benchmark evidence does not
claim that every runtime path is qualified on every platform.

## Alternative: install from a reviewed source checkout

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

The full test command above is the current hosted contract on Linux and macOS.
On Windows, compile every test target without executing the still-pending
native runtime and cache paths:

```powershell
cargo test --locked --workspace --all-targets --all-features --no-run
```

That compile-only result proves source portability, not Windows runtime
qualification. The current platform boundary is recorded in
[`BETA_SUPPORT.md`](BETA_SUPPORT.md).

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
scripts/build_release_candidate.sh --release-label v0.1.0-rc.N /absolute/output/directory
```

It creates:

- one `commit-ci-preflight-<release-label>-<target>.tar.gz` archive;
- `SHA256SUMS` for the maintainer-uploaded archive asset; and
- `RELEASE_MANIFEST.json` inside the archive, binding its label, source commit,
  Cargo package version, target, and archive name.

The archive contains the host binary, `LICENSE`, `NOTICE`, `README.md`, the SPDX
SBOM, third-party notices, adoption, installation, troubleshooting, rollback,
threat-model, support, tutorial, economic, and time-to-feedback documents, plus the inactive
cross-repository GitHub gate template. The script refuses a relative output
path or a non-empty output directory, checks that release metadata is current,
builds with `--locked`, and does not tag, push, upload, sign, or publish.

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
Get-FileHash .\commit-ci-preflight-<release-label>-<target>.tar.gz -Algorithm SHA256
```

A matching checksum proves only byte integrity relative to the separately
obtained checksum file. Extract the archive and inspect its source binding:

```console
tar -xzf commit-ci-preflight-<release-label>-<target>.tar.gz
sed -n '1,120p' commit-ci-preflight-<release-label>-<target>/RELEASE_MANIFEST.json
```

The manifest binds the selected source commit and target but does not sign the
asset. `SHA256SUMS` covers maintainer-uploaded archives only; GitHub-generated
source archives are identified by their exact public tag and repository
reference, not by this local checksum file. Release signing remains intentionally
out of scope until key custody has its own ADR and authorization.

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
