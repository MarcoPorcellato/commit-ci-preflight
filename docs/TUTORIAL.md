# End-to-end tutorial

This tutorial uses the repository's clean-room Rust fixture. It demonstrates
the local plan, runtime probe, dry run, real container execution, receipt, and
independent policy verification without connecting a self-hosted runner or
executing a marketplace action.

## 1. Build the CLI

From a reviewed Commit CI Preflight checkout:

```console
cargo build --locked
./target/debug/commit-ci-preflight --version
```

## 2. Create a disposable Git repository

Copy the public fixture to a directory of your choice:

```console
mkdir -p /absolute/demo-parent
cp -R examples/projects/rust /absolute/demo-parent/ccp-rust-demo
cd /absolute/demo-parent/ccp-rust-demo
mkdir -p target
git init
git add .
git commit -m "demo: initial fixture"
```

`target/` is ignored by Git, but it must exist before `run` because it is the
destination of the nested writable cache binding beneath the read-only
`/workspace` mount. Commit CI Preflight validates this condition before
starting Docker and never creates a path inside the source checkout.

The fixture has no proprietary data and pins an official multi-platform Rust
image by OCI digest. Its check container has network disabled. Choose a
persistent cache outside the repository and outside temporary directories, for
example `/absolute/persistent/ccp-demo-cache`.

Set the CLI path explicitly in the commands below:

```console
CCP_BIN=/absolute/path/to/commit-ci-preflight/target/debug/commit-ci-preflight
```

## 3. Inspect before execution

Normalize the configuration:

```console
"$CCP_BIN" plan --config .commit-ci-preflight.toml --json
```

Probe the local Docker-compatible runtime without executing project code:

```console
"$CCP_BIN" doctor --config .commit-ci-preflight.toml --json
```

Render exact container argv and mounts:

```console
"$CCP_BIN" dry-run   --config .commit-ci-preflight.toml   --repository .   --cache-dir /absolute/persistent/ccp-demo-cache   --json
```

Confirm that the repository mount is read-only and only declared cache or
artifact paths are writable.

## 4. Execute the local preflight

```console
"$CCP_BIN" run   --config .commit-ci-preflight.toml   --repository .   --cache-dir /absolute/persistent/ccp-demo-cache   --generation 1
```

The command requires a clean commit, runs `cargo test --locked` in the pinned
container, and writes
`.ccp/receipt.json`. Raw command output is bounded locally and is not embedded
in the receipt.

Inspect only the minimized evidence:

```console
"$CCP_BIN" cache inventory   --cache-dir /absolute/persistent/ccp-demo-cache   --json
sed -n '1,120p' .ccp/receipt.json
```

## 5. Verify independently

Capture the exact demo commit:

```console
git rev-parse HEAD
```

Then verify with the fixture policy:

```console
"$CCP_BIN" verify   --receipt .ccp/receipt.json   --policy .commit-ci-policy.toml   --expected-commit <paste-the-exact-commit>
```

A `PASS` means the receipt has valid integrity and satisfies the declared
project, commit, configuration, checks, image, freshness, and platform policy.
It does not prove who ran the command.

The fixture policy accepts the explicitly listed demo platforms. Acceptance is
an operator policy, not a project qualification claim; consult
[`BETA_SUPPORT.md`](BETA_SUPPORT.md) for the narrower evidence matrix.

## 6. Connect the lightweight GitHub gate

For a real repository:

1. commit a reviewed configuration and policy;
2. run the complete local preflight on the exact source commit;
3. verify the receipt locally;
4. publish only `.ccp/receipt.json` to
   `ccp-evidence/<exact-source-sha>` without force-pushing;
5. push the source branch and open the pull request;
6. retain GitHub review, permissions, secrets, deployment, and uncovered
   platform checks remotely.

The exact threat model and workflow are documented in
[`GITHUB_GATE.md`](GITHUB_GATE.md). Never run a fork's unreviewed executable,
workflow, cache, or receipt as trusted code.

## 7. Safe cleanup

The 0.1.0 cleanup command is preview-only:

```console
"$CCP_BIN" cache cleanup   --cache-dir /absolute/persistent/ccp-demo-cache   --dry-run
```

Remove the demo checkout only when you no longer need its receipt. Cache
deletion is a separate operator decision; see
[`CACHE_AND_WORKSPACE.md`](CACHE_AND_WORKSPACE.md). The product never deletes
a broad or unresolved path.
