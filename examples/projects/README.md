# Clean-room example projects

These three minimal projects exercise the public `run` contract without any
proprietary source, fixture, or dependency. Each configuration pins an official
multi-platform image by OCI index digest as observed on 2026-08-09.

Copy one directory to its own Git repository, commit every file, then run the
locally built binary with an explicit persistent cache root. The Rust fixture
also needs an ignored `target/` directory before `run` because that is its
nested writable cache destination:

```console
commit-ci-preflight run \
  --repository /absolute/path/to/copied-project \
  --config /absolute/path/to/copied-project/.commit-ci-preflight.toml \
  --cache-dir /absolute/persistent/cache
```

The first invocation may download the pinned image. Network access is disabled
inside the check container. `.ccp/receipt.json` is ignored by each example so a
second run can inspect the same committed checkout.

The Rust fixture also contains a policy for independently verifying its test
receipt on explicitly accepted demo platforms. Policy acceptance is
not a platform qualification claim. Follow [`docs/TUTORIAL.md`](../../docs/TUTORIAL.md)
for the complete copy, commit, inspect, run, and verify sequence.
