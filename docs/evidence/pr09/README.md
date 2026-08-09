# PR09 native evidence matrix

This directory contains immutable receipts from real executions of the fixed
benchmark contract at source commit
`15f858403b19ade38373176879fb518ef167580d`. Each receipt passed the Rust
verifier with externally supplied commit, operating-system, architecture, and
runtime or CI expectations.

| Evidence | Status | Receipt or record | Receipt ID / run |
|---|---|---|---|
| macOS arm64 + OrbStack probe | `PASS` | [`macos-arm64-orbstack.json`](macos-arm64-orbstack.json) | `sha256:3c809d0a86d893973d1dc4071a5b1cff0d7e34031bb24ea1e185fef668fc8f71` |
| Linux x86_64 | `PASS` | [`linux-x86_64-github.json`](linux-x86_64-github.json) | `sha256:0ec9de873491004694a4ec3af38de5de34828efac8e2a53260149f9ae75ce7da` |
| Windows x86_64 | `PASS` | [`windows-x86_64-github.json`](windows-x86_64-github.json) | `sha256:0487ea77449ba220b282d975b095030f0cd7b6d518cb470a9f6385643126d7b9` |
| GitHub-hosted comparison metadata | `PASS` | [`github-run-31342216377.json`](github-run-31342216377.json) | [run `31342216377`](https://github.com/MarcoPorcellato/commit-ci-preflight/actions/runs/31342216377) |

## File integrity

| File | SHA-256 of exact file bytes |
|---|---|
| `macos-arm64-orbstack.json` | `1b1089926b370a24e1788f294b56d3e652c4de9a387c1e2f6308771bb7d6b483` |
| `linux-x86_64-github.json` | `c758dd2d106240b7387f77a753509b67b982323b6e2035ada43231e57714b0b7` |
| `windows-x86_64-github.json` | `e145dc4f9bf0a56cb6ffc793aae59aeec1441d2e4dfa18f80eccaf0f570994d2` |

The Mac benchmark ran as a native macOS process and separately probed the
OrbStack Docker-compatible runtime. It is not labelled Linux-native. Linux and
Windows receipts came from the standard runner labels recorded in the GitHub
run metadata. `ci_environment: github_actions` is metadata, not a cryptographic
producer-identity claim.

## Claim boundary

All three native processes produced the pinned correctness digest
`sha256:29ac09de518a019bd8c663b411f77bbab466c7cf7236b2f56c2dbb6b105c69dc`.
Timings are observations from one bounded run and may vary. This evidence does
not claim arbitrary GitHub Actions parity, platform identity attestation, or
universal performance.

The documentation commit containing these files necessarily differs from the
qualified source commit. It preserves the receipt bytes and states their source
commit rather than pretending they describe the later documentation commit.
