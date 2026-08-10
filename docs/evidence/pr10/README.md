# PR10 beta-candidate hardening evidence

## Clean-room Rust tutorial

The end-to-end tutorial was executed on 2026-08-10 against an independently
initialized Git repository copied from `examples/projects/rust`.

| Field | Evidence |
|---|---|
| Demo commit | `f7efbe230aaad1392f362974d83fc52b889074e9` |
| Configuration digest | `sha256:cff29e073937f7bd51d611fbd407c94e2ce5915f01ee018f7fb3d8f35819ea98` |
| Pinned image | `docker.io/library/rust@sha256:a41f7740f8b45d45795624eec13a8b42263cc700f19f7e4e86e04d3dda08a479` |
| Host/runtime | macOS arm64, OrbStack Docker-compatible runtime 29.4.0 |
| Container network | Disabled |
| Required check | `cargo test --locked`: PASS |
| Run ID | `sha256:7b00d28f7cd8eef5aa2b7db7bb10c3f7976c87d5876ea0733ed0a183dcf2f75c` |
| Receipt ID | `sha256:8a392eaf4a4d4a268cbf5cdba54e4f12d90981781769c9af463e63862dd58855` |
| Original run file SHA-256 | `459bf8e58116ed8f592827a87d57b38ee0922f908a3863869899a618d525735f` |
| Repository copy SHA-256 | `4c7c1b74b179a1b631d3e912a6de33b2d74d9f8c51e36fe8f41cfa1c1391813e` |
| Independent verification | Integrity PASS, policy PASS, decision PASS |

The source repository was mounted read-only. Its ignored `target/` directory
existed before execution and received the nested writable managed-cache bind.
The persistent cache was outside the checkout and outside temporary storage.
No network, LLM, marketplace action, secret, proprietary fixture, or fabricated
timing was used.

Two diagnostic attempts preceded the recorded PASS. The original two-check
fixture discovered that the pinned minimal Rust image has no `rustfmt`
component, so the public demo was narrowed truthfully to its supported locked
test. A subsequent Docker exit 125 exposed that a nested mount destination must
exist beneath the read-only repository binding. PR10 adds a fail-fast
repository-target check and documents the ignored `target/` setup. Failed
diagnostic receipts are intentionally not presented as qualification evidence.

The canonical receipt is committed as
[`demo-rust-receipt.json`](demo-rust-receipt.json). The repository copy adds a
single trailing line feed; independent verification reconstructs the same
canonical payload and receipt ID. This proves the bounded
tutorial path on the stated host/runtime. It is not producer-identity evidence
and does not qualify complete Linux-native or Windows-native execution.

## Supply-chain and repository security

On 2026-08-10 the release-candidate change left `Cargo.toml`, `Cargo.lock`, and
`rust-toolchain.toml` byte-identical to the merged PR09 baseline. Their SHA-256
digests were respectively:

- `d2c834cf39a69846121c05dbe5a76d31843382cf5fba48d87d095888dfb998ce`;
- `633b40b9c531fb6a6846772ed85969576d00a4da694cf7a9c1a5d35434604f47`;
- `c5cfbcfb9b2db63c41ae54bb1b954ed66882ce4a44c50b2fe3fbaa6e161c04bc`.

GitHub Dependabot vulnerability alerts were enabled without enabling automatic
security-update pull requests, and the live open-alert query returned zero.
Secret scanning, push protection, private vulnerability reporting, issues,
squash-only merging, and post-merge branch deletion were enabled. This is a
dated repository-state receipt, not a permanent guarantee; release decisions
must refresh the query.

## Local candidate archive, rollback, and uninstall

The local candidate builder was executed from clean source commit
`8629813b45e98263837ea469b689602bfc5b5f1c`. It reran the deterministic release
metadata check, all seven hardening contract tests, and a locked optimized
build before creating an unpublished macOS arm64 archive.

| Field | Evidence |
|---|---|
| Archive | `commit-ci-preflight-v0.1.0-aarch64-apple-darwin.tar.gz` |
| Archive bytes | `1106537` |
| Archive SHA-256 | `a858b6bd30f91843382401d01b8cba6fae317eda1f89ae7fba61cff352519da8` |
| Checksum verification | `shasum -a 256 -c SHA256SUMS`: PASS |
| Packaged binary SHA-256 | `34ca38802977f326e733f9d34cfdf3419514a545ea3282ca207a23634930f342` |
| Packaged binary smoke test | `commit-ci-preflight 0.1.0` |

The archive inventory contained exactly one top-level versioned directory, the
executable, `LICENSE`, `NOTICE`, `README.md`, `SBOM.spdx.json`,
`THIRD_PARTY_NOTICES.md`, and the five documented installation, rollback,
threat-model, beta-support, and tutorial files. Byte comparisons proved the
packaged license, notice, SBOM, and third-party notices matched the reviewed
source files.

The rollback runbook was exercised with two distinct executable hashes. The
candidate replaced a preserved executable, matched the expected candidate
hash, was moved to quarantine, and the preserved executable was restored
byte-for-byte and returned version `0.1.0`. An isolated locked `cargo install`
then returned version `0.1.0`; `cargo uninstall --root` removed that exact
binary and left the isolated `bin/` directory empty. No receipt, cache, archive,
tag, signature, package, release, or unrelated data was deleted or published.
