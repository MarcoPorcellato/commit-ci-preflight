# macOS-v4 compound watchdog restart handoff

## Persistent checkpoint

- Repository: `MarcoPorcellato/commit-ci-preflight`
- Worktree: persistent, non-temporary checkout of the branch below
- Branch: `codex/macos-v4-compound-watchdog`
- Base: `origin/main` at `9c506890880b89747462c0d21087e49abe78b8ee`
- Scope: macOS resource policy and local resource-history evidence only

The worktree must remain outside temporary directories, so a normal Mac restart
does not remove it. The operator's pre-existing source checkout was not
modified.

## Implemented state

The candidate versions the policy as `macos-v4`. Pre-start retains the 20%
available, 3 GiB reclaimable and bounded-swap limits, but compression alone is
advisory. Compression can drive an immediate denial only at 70% or more with a
companion pressure signal. During an admitted run:

- compressor occupancy alone cannot cancel the workload;
- soft pressure requires at least two independent signals for 15 consecutive
  two-second samples;
- the soft signals are available memory below 10%, reclaimable memory below
  1.5 GiB, compressor occupancy at least 55%, swap at least 4 GiB, and swap
  growth of at least 1 GiB over the bounded 16-sample trend window;
- hard pressure remains immediate at 3% available memory, below 512 MiB
  reclaimable memory, 8 GiB swap, or at least 70% compressor occupancy together
  with a companion pressure signal;
- hard and soft cancellations append the exact bounded trip sample to local
  resource-history v2 without repository, command, commit, user, or machine
  identity.

Current fail-closed receipt and outer-result semantics are unchanged. In
particular, a late soft trip does not become PASS merely because an inner check
finished. That possible rule requires a separate assurance decision.

## Evidence completed before restart

- exact branch/base and allowlist inspected;
- measured local history reviewed: 84 schema-v2 records, 14 pressure outcomes,
  10 compressor-only false-positive candidates, and 8 pressure outcomes with
  zero swap;
- the latest observed false-positive sample is encoded as a deterministic unit
  fixture: 42% available, 8,126,005,248 reclaimable bytes,
  16,777,625,600 compressed bytes, and zero swap;
- source and documentation diff reviewed;
- `git diff --check`: PASS;
- `cargo fmt --all -- --check`: PASS;
- `cargo test --locked resource --lib`: PASS, 30 tests;
- `cargo test --locked --all-targets --all-features`: the unfiltered run reached
  159 passing tests before a host-bound test received exit code 6;
- a second full run with only the two admission-dependent end-to-end tests
  explicitly skipped passed all 220 remaining tests across every target;
- after restart, both admission-dependent end-to-end tests passed when executed
  individually on exact commit `911c89092a884af8ce3a63360f37ee687a6f75a2`;
- a subsequent unfiltered suite raised compressor occupancy above the installed
  `macos-v3` 40% pre-start boundary, causing the same test to receive exit code
  6 despite 40% available memory, 7,594,246,144 reclaimable bytes and zero swap;
- because the normative default suite must not depend on live host pressure,
  the two contracts are now explicit native opt-in tests with documented exact
  commands; they are not deleted, mocked or counted as default-suite PASS;
- after that separation, the complete deterministic default passed with 218
  tests PASS and the two native tests truthfully reported as ignored;
- `cargo +stable clippy --locked --all-targets --all-features -- -D warnings`:
  PASS using the already-installed stable toolchain without downloads.
- after the restart exposed the same false positive at pre-start, the policy
  was further simplified so compression alone cannot deny admission; focused
  resource tests pass 32/32, including the observed host fixture and a hard
  compound-compression denial.

## Current candidate evidence

After the compound pre-start correction:

- `cargo fmt --all -- --check`: PASS;
- `cargo test --locked resource --lib`: PASS, 32 tests;
- `cargo test --locked --all-targets --all-features`: PASS, 218 tests, with the
  two native admission contracts explicitly ignored by the deterministic
  default;
- both documented native admission contracts: PASS when run individually;
- `cargo +stable clippy --locked --all-targets --all-features -- -D warnings`:
  PASS;
- `cargo doc --locked --workspace --no-deps`: PASS;
- `git diff --check`: PASS.

No OrbStack qualification, GitHub workflow, push, pull request or merge is
claimed by these local checks.

The first OrbStack attempt on `ca58ac584c34b02fb0a1666aadfc84b819d47d54`
failed only `format` because the repository-required Cargo/Rustup cache
variables were not exported. The failed receipt was preserved locally. With
the documented variables set, generation 2 passed all five required stages and
independent verification returned integrity, policy and decision PASS. This
receipt qualifies `ca58ac5` only; the documentation correction that records the
operator prerequisite requires a fresh exact-head run after commit.

Before restart, no Docker container, OrbStack workload, guarded CCP run, GitHub
workflow, or native qualification was started under resource denial. After
restart, only the two documented native CLI contracts ran while admission was
`Admit`; no OrbStack workload or GitHub workflow was started.

## Host condition at checkpoint

The last pre-restart sample was `macos-v3` `Deny`: 35% available memory,
6,619,529,216 reclaimable bytes, 19,725,615,104 compressed bytes, and
22,838,970,941 swap bytes used. Admission was inactive with an empty queue and
Docker reported no running container. Recheck these facts after restart; they
are not durable evidence.

## Resume and qualification gates

Run these commands from the persistent worktree after the Mac and OrbStack are
stable:

```console
cd "<persistent-macos-v4-worktree>"
git status --short --branch
commit-ci-preflight resource status --json
commit-ci-preflight admission status --json
docker --context orbstack ps
cargo fmt --all -- --check
cargo test --locked --all-targets --all-features
cargo +stable clippy --locked --all-targets --all-features -- -D warnings
git diff --check
```

The focused, full deterministic and native CLI gates now pass. The remaining
qualification step is the repository-native CCP/OrbStack run on the exact
committed candidate. Only a terminal outer PASS with complete exact-commit
evidence qualifies that gate. Do not treat this handoff, formatting, or static
review as OrbStack qualification.
