# M0 Compatibility Baseline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create a durable, executable baseline that makes CCP's strict CLI, exit-code, canonical-byte, verification, and public Rust API compatibility contract falsifiable before internal refactoring begins.

**Architecture:** The baseline is test-only evidence. Existing golden fixtures remain authoritative; a small integration contract binds them, exact CLI help bytes, deterministic read-only command outputs, and a downstream compile fixture to base commit `5fed7c443504969e62980141048f9279f9fa1dfe`. No production module, schema, receipt, runtime, or CLI behavior changes.

**Tech Stack:** Rust 2024 integration tests, `std::process::Command`, existing `sha2` and `serde_json`, Markdown/JSON fixtures, Cargo offline mode.

**Spec:** `docs/superpowers/specs/2026-08-30-capability-packs-clean-architecture-design.md`

## Global Constraints

- Base is exact commit `5fed7c443504969e62980141048f9279f9fa1dfe`; design-only commit is `5ef9707930f7095a2f57bc3e38e53bfeac06aaf2`.
- Every shell command begins with `rtk`.
- Preserve existing CLI syntax, exit codes, valid configuration bytes and digests, receipt bytes and IDs, policy schemas, JSON shapes, and public Rust facade.
- This milestone executes no Docker workload, CCP heavy command, network operation, publication, or external mutation.
- Fixture generation may execute only read-only CCP commands whose current integration tests already prove they do not execute project checks: help, `plan`, and `verify` against checked-in fixtures.
- Store no usernames, absolute local paths, environment dumps, source contents, secrets, container IDs, or raw project logs.
- Use RED/GREEN TDD for every executable contract.

---

### Task 1: Durable goal and progress ledger

**Files:**
- Create: `.superpowers/sdd/2026-08-30-capability-packs-clean-architecture/goal.txt`
- Create: `.superpowers/sdd/2026-08-30-capability-packs-clean-architecture/progress.md`

**Interfaces:**
- Consumes: the canonical specification and exact design commit.
- Produces: the restart-safe execution pointer used by every later task.

- [ ] **Step 1: Create the concise persistent goal**

Write exactly:

```text
Complete the Clean Architecture and Advanced Linux Capability Packs programme
according to `docs/superpowers/specs/2026-08-30-capability-packs-clean-architecture-design.md`.

First verify the live worktree, exact branch/HEAD/base, active operator policy,
and the progress ledger. Preserve strict CLI, exit-code, plan, receipt, policy,
JSON, and public Rust API compatibility. Use deterministic tools first and
delegate bounded inventory, mechanical edits, tests, and review to Luna or
Spark; retain architecture, security, integration, qualification, release, and
merge judgment centrally.

Proceed through dependency-ordered milestones until the canonical checklist is
proved. Do not run CCP heavy work for this public repository without an exact
approved non-economic exception. Treat push, PR, ready transition, merge, tag,
release, stable installation, and external publication as separate exact-state
gates.
```

- [ ] **Step 2: Create the progress ledger**

Write:

```markdown
# Capability Packs and Clean Architecture progress

- Base: `5fed7c443504969e62980141048f9279f9fa1dfe`
- Branch: `codex/capability-packs-clean-architecture-v1`
- Specification commit: `5ef9707930f7095a2f57bc3e38e53bfeac06aaf2`
- Current milestone: M0 compatibility baseline
- Completed evidence: design review READY; `git diff --check` PASS
- Unproven: baseline tests, application seam, pack contract, reference packs,
  hosted CI, publication, release
- Heavy processes: none
- External mutations: none
- Next action: execute Task 2 of the M0 plan with RED/GREEN TDD
```

- [ ] **Step 3: Verify the files are bounded and path-free**

Run:

```console
rtk rg -n "(/Users/|TODO|TBD|FIXME|container ID|secret)" .superpowers/sdd/2026-08-30-capability-packs-clean-architecture
```

Expected: no match. The relative specification path is permitted; no local
absolute path is stored.

- [ ] **Step 4: Commit Task 1**

```console
rtk git add .superpowers/sdd/2026-08-30-capability-packs-clean-architecture/goal.txt .superpowers/sdd/2026-08-30-capability-packs-clean-architecture/progress.md
rtk git commit -m "docs: add capability programme checkpoint"
```

### Task 2: Pin CLI and canonical evidence bytes

**Files:**
- Create: `tests/compatibility_baseline.rs`
- Create: `tests/fixtures/compatibility/root-help.stdout.txt`
- Create: `tests/fixtures/compatibility/plan-help.stdout.txt`
- Create: `tests/fixtures/compatibility/verify-help.stdout.txt`
- Create: `tests/fixtures/compatibility/plan-v1.stdout.json`
- Create: `tests/fixtures/compatibility/plan-v2-legacy.stdout.json`
- Create: `tests/fixtures/compatibility/dry-run-v1.normalized.json`
- Create: `tests/fixtures/compatibility/dry-run-v2-current.normalized.json`
- Create: `tests/fixtures/compatibility/dry-run-v2-legacy.normalized.json`
- Create: `tests/fixtures/compatibility/verify-v1-pass.stdout.json`
- Create: `tests/fixtures/compatibility/verify-v1-fail.stdout.json`
- Reuse: `tests/fixtures/plan-v2-current-default.stdout.json`
- Reuse: `tests/fixtures/receipt-v1-pass.json`
- Reuse: `tests/fixtures/receipt-v2-pass.json`
- Reuse: `tests/fixtures/matrix-v2-legacy-plan-044697.json`
- Reuse: `tests/fixtures/policy-v1.toml`
- Reuse: `tests/fixtures/policy-v1_1-trusted-plan.toml`
- Reuse: `tests/fixtures/policy-v2-legacy-compatible.toml`

**Interfaces:**
- Consumes: `CARGO_BIN_EXE_commit-ci-preflight` and existing read-only fixtures.
- Produces: exact byte comparisons for public CLI/canonical evidence plus
  privacy-normalized dry-run projections used by M1.

- [ ] **Step 1: Write a deliberately incomplete failing contract**

Create `tests/compatibility_baseline.rs` with imports, constants, and the first
test. Do not create the snapshot files yet:

```rust
use std::process::{Command, Output};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const EVALUATED_AT: &str = "2026-08-08T12:30:00Z";

fn ccp(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args(args)
        .output()
        .expect("execute compatibility command")
}

#[test]
fn root_help_bytes_match_the_baseline() {
    let output = ccp(&["--help"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        include_bytes!("fixtures/compatibility/root-help.stdout.txt")
    );
}
```

- [ ] **Step 2: Run the focused test and verify RED**

Run:

```console
rtk cargo test --locked --offline --test compatibility_baseline root_help_bytes_match_the_baseline
```

Expected: compile failure because
`tests/fixtures/compatibility/root-help.stdout.txt` does not exist.

- [ ] **Step 3: Capture exact read-only baseline bytes**

Build the test binary once with the focused test command, then run only these
read-only commands from the repository root and save stdout byte-for-byte under
the named fixture paths:

```console
rtk cargo run --locked --offline -- --help
rtk cargo run --locked --offline -- plan --help
rtk cargo run --locked --offline -- verify --help
rtk cargo run --locked --offline -- plan --config tests/fixtures/config-v1-read-only.toml --json
rtk cargo run --locked --offline -- plan --config tests/fixtures/config-v2-matrix.toml --json
rtk cargo run --locked --offline -- plan --config tests/fixtures/config-v2-legacy-compatible.toml --matrix-plan-profile matrix-v2-legacy-v1 --json
rtk cargo run --locked --offline -- verify --receipt tests/fixtures/receipt-v1-pass.json --policy tests/fixtures/policy-v1.toml --expected-commit 0123456789abcdef0123456789abcdef01234567 --evaluated-at-utc 2026-08-08T12:30:00Z --json
```

For the failing verification snapshot, use the same command with expected
commit `bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb`; require exit `3` and save
stdout only. Use `apply_patch` for every fixture write; do not use shell
redirection.

- [ ] **Step 4: Complete the byte and exit-code contract**

Add tests that:

```rust
#[test]
fn command_help_bytes_match_the_baseline() {
    for (command, expected) in [
        ("plan", include_bytes!("fixtures/compatibility/plan-help.stdout.txt").as_slice()),
        ("verify", include_bytes!("fixtures/compatibility/verify-help.stdout.txt").as_slice()),
    ] {
        let output = ccp(&[command, "--help"]);
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stderr.is_empty());
        assert_eq!(output.stdout, expected, "{command} help drifted");
    }
}

#[test]
fn plan_and_verification_bytes_and_exit_codes_match_the_baseline() {
    let plan = ccp(&[
        "plan", "--config", "tests/fixtures/config-v1-read-only.toml", "--json",
    ]);
    assert_eq!(plan.status.code(), Some(0));
    assert!(plan.stderr.is_empty());
    assert_eq!(plan.stdout, include_bytes!("fixtures/compatibility/plan-v1.stdout.json"));

    let pass = ccp(&[
        "verify", "--receipt", "tests/fixtures/receipt-v1-pass.json",
        "--policy", "tests/fixtures/policy-v1.toml", "--expected-commit", COMMIT,
        "--evaluated-at-utc", EVALUATED_AT, "--json",
    ]);
    assert_eq!(pass.status.code(), Some(0));
    assert!(pass.stderr.is_empty());
    assert_eq!(pass.stdout, include_bytes!("fixtures/compatibility/verify-v1-pass.stdout.json"));

    let fail = ccp(&[
        "verify", "--receipt", "tests/fixtures/receipt-v1-pass.json",
        "--policy", "tests/fixtures/policy-v1.toml", "--expected-commit",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "--evaluated-at-utc",
        EVALUATED_AT, "--json",
    ]);
    assert_eq!(fail.status.code(), Some(3));
    assert_eq!(fail.stdout, include_bytes!("fixtures/compatibility/verify-v1-fail.stdout.json"));
}

#[test]
fn matrix_plan_profiles_match_the_baseline() {
    let current = ccp(&[
        "plan", "--config", "tests/fixtures/config-v2-matrix.toml", "--json",
    ]);
    assert_eq!(current.status.code(), Some(0));
    assert!(current.stderr.is_empty());
    assert_eq!(current.stdout, include_bytes!("fixtures/plan-v2-current-default.stdout.json"));

    let legacy = ccp(&[
        "plan", "--config", "tests/fixtures/config-v2-legacy-compatible.toml",
        "--matrix-plan-profile", "matrix-v2-legacy-v1", "--json",
    ]);
    assert_eq!(legacy.status.code(), Some(0));
    assert!(legacy.stderr.is_empty());
    assert_eq!(legacy.stdout, include_bytes!("fixtures/compatibility/plan-v2-legacy.stdout.json"));
}

fn normalize_mount_argv(argument: &str, sources: &std::collections::BTreeMap<String, String>) -> String {
    let Some(rest) = argument.strip_prefix("type=bind,src=") else {
        return argument.to_owned();
    };
    let (source, suffix) = rest.split_once(",dst=").expect("bind destination");
    let token = sources.get(source).expect("declared mount source");
    format!("type=bind,src={token},dst={suffix}")
}

fn normalize_one_dry_run(mut value: serde_json::Value) -> (serde_json::Value, Vec<String>) {
    let workspace = value["workspace"].as_object().expect("workspace object");
    let repository = workspace["repository"].as_str().expect("repository path").to_owned();
    let run_root = workspace["run_root"].as_str().expect("run root").to_owned();
    let mut sources = std::collections::BTreeMap::new();
    sources.insert(repository.clone(), "$REPOSITORY".to_owned());
    for mount in workspace["mounts"].as_array().expect("mounts") {
        let source = mount["source"].as_str().expect("mount source").to_owned();
        let purpose = mount["purpose"].as_str().expect("mount purpose");
        let logical_id = mount.get("logical_id").and_then(|item| item.as_str());
        let token = match (purpose, logical_id) {
            ("repository", _) => "$REPOSITORY".to_owned(),
            ("cache", Some(id)) => format!("$CACHE:{id}"),
            ("artifact", Some(id)) => format!("$ARTIFACT:{id}"),
            _ => panic!("unsupported mount identity"),
        };
        sources.insert(source, token);
    }

    value["workspace"]["repository"] = serde_json::json!("$REPOSITORY");
    value["workspace"]["run_root"] = serde_json::json!("$RUN_ROOT");
    for mount in value["workspace"]["mounts"].as_array_mut().expect("mounts") {
        let source = mount["source"].as_str().expect("mount source");
        mount["source"] = serde_json::json!(sources.get(source).expect("mount token"));
    }
    for check in value["checks"].as_array_mut().expect("checks") {
        for argument in check["argv"].as_array_mut().expect("argv") {
            let raw = argument.as_str().expect("argv string");
            *argument = serde_json::json!(normalize_mount_argv(raw, &sources));
        }
    }

    let mut host_paths = sources.into_keys().collect::<Vec<_>>();
    host_paths.push(run_root);
    (value, host_paths)
}

fn normalize_dry_run(mut value: serde_json::Value) -> (serde_json::Value, Vec<String>) {
    let mut host_paths = Vec::new();
    if let Some(runtimes) = value.get_mut("runtimes").and_then(|value| value.as_array_mut()) {
        for runtime in runtimes {
            let dry_run = runtime.get_mut("dry_run").expect("matrix dry-run").take();
            let (normalized, paths) = normalize_one_dry_run(dry_run);
            runtime["dry_run"] = normalized;
            host_paths.extend(paths);
        }
        (value, host_paths)
    } else {
        normalize_one_dry_run(value)
    }
}

#[test]
fn dry_run_profiles_match_the_baseline_without_execution() {
    for (args, expected) in [
        (
            vec!["dry-run", "--config", "tests/fixtures/config-v1-read-only.toml", "--json"],
            include_bytes!("fixtures/compatibility/dry-run-v1.normalized.json").as_slice(),
        ),
        (
            vec!["dry-run", "--config", "tests/fixtures/config-v2-matrix.toml", "--json"],
            include_bytes!("fixtures/compatibility/dry-run-v2-current.normalized.json").as_slice(),
        ),
        (
            vec![
                "dry-run", "--config", "tests/fixtures/config-v2-legacy-compatible.toml",
                "--matrix-plan-profile", "matrix-v2-legacy-v1", "--json",
            ],
            include_bytes!("fixtures/compatibility/dry-run-v2-legacy.normalized.json").as_slice(),
        ),
    ] {
        let output = ccp(&args);
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stderr.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("dry-run JSON");
        let original = value.clone();
        let (normalized, host_paths) = normalize_dry_run(value);
        assert_dry_run_normalization_preserves_contract(&original, &normalized);
        let serialized = serde_json::to_vec(&normalized).expect("normalized JSON");
        for path in host_paths {
            assert!(!serialized.windows(path.len()).any(|window| window == path.as_bytes()));
        }
        assert_eq!(serialized, expected);
        if let Some(executed) = value.get("executed") {
            assert_eq!(executed, false);
        } else {
            for runtime in value["runtimes"].as_array().expect("matrix runtimes") {
                assert_eq!(runtime["dry_run"]["executed"], false);
            }
        }
    }
}

#[test]
fn usage_error_exit_code_remains_two() {
    let output = ccp(&[
        "plan", "--config", "tests/fixtures/does-not-exist.toml",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}
```

Implement `assert_dry_run_normalization_preserves_contract` alongside the
normalizer. For each single or matrix runtime it must assert identical
top-level key sets, check count, check IDs, programs, dependencies, argv count,
workspace key sets, mount count, `target`, `access`, `purpose`, and
`logical_id`. Every argv element must be byte-identical unless the original
starts with `type=bind,src=`; for those elements, split both values at `,dst=`
and require identical suffixes plus a normalized prefix beginning
`type=bind,src=$`. This assertion runs before every snapshot comparison.

```rust
fn object_keys(value: &serde_json::Value) -> std::collections::BTreeSet<&str> {
    value
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect()
}

fn assert_one_dry_run_contract(original: &serde_json::Value, normalized: &serde_json::Value) {
    assert_eq!(object_keys(original), object_keys(normalized));
    for field in ["schema_version", "plan_digest", "runtime", "program", "workspace_mount_policy", "executed"] {
        assert_eq!(original[field], normalized[field], "{field}");
    }
    let original_checks = original["checks"].as_array().expect("original checks");
    let normalized_checks = normalized["checks"].as_array().expect("normalized checks");
    assert_eq!(original_checks.len(), normalized_checks.len());
    for (before, after) in original_checks.iter().zip(normalized_checks) {
        assert_eq!(object_keys(before), object_keys(after));
        for field in ["id", "program", "depends_on"] {
            assert_eq!(before[field], after[field], "{field}");
        }
        let before_argv = before["argv"].as_array().expect("original argv");
        let after_argv = after["argv"].as_array().expect("normalized argv");
        assert_eq!(before_argv.len(), after_argv.len());
        for (raw, scrubbed) in before_argv.iter().zip(after_argv) {
            let raw = raw.as_str().expect("raw argv");
            let scrubbed = scrubbed.as_str().expect("scrubbed argv");
            if let Some(raw_mount) = raw.strip_prefix("type=bind,src=") {
                let (_, raw_suffix) = raw_mount.split_once(",dst=").expect("raw destination");
                let scrubbed_mount = scrubbed.strip_prefix("type=bind,src=$").expect("tokenized source");
                let (_, scrubbed_suffix) = scrubbed_mount.split_once(",dst=").expect("scrubbed destination");
                assert_eq!(raw_suffix, scrubbed_suffix);
            } else {
                assert_eq!(raw, scrubbed);
            }
        }
    }

    let before_workspace = &original["workspace"];
    let after_workspace = &normalized["workspace"];
    assert_eq!(object_keys(before_workspace), object_keys(after_workspace));
    assert_eq!(before_workspace["schema_version"], after_workspace["schema_version"]);
    assert_eq!(before_workspace["source_snapshot_digest"], after_workspace["source_snapshot_digest"]);
    let before_mounts = before_workspace["mounts"].as_array().expect("original mounts");
    let after_mounts = after_workspace["mounts"].as_array().expect("normalized mounts");
    assert_eq!(before_mounts.len(), after_mounts.len());
    for (before, after) in before_mounts.iter().zip(after_mounts) {
        assert_eq!(object_keys(before), object_keys(after));
        for field in ["target", "access", "purpose", "logical_id"] {
            assert_eq!(before.get(field), after.get(field), "{field}");
        }
    }
}

fn assert_dry_run_normalization_preserves_contract(
    original: &serde_json::Value,
    normalized: &serde_json::Value,
) {
    assert_eq!(object_keys(original), object_keys(normalized));
    match (original.get("runtimes"), normalized.get("runtimes")) {
        (Some(before), Some(after)) => {
            let before = before.as_array().expect("original runtimes");
            let after = after.as_array().expect("normalized runtimes");
            assert_eq!(before.len(), after.len());
            for (before, after) in before.iter().zip(after) {
                assert_eq!(object_keys(before), object_keys(after));
                assert_eq!(before["runtime_id"], after["runtime_id"]);
                assert_eq!(before["configuration_digest"], after["configuration_digest"]);
                assert_one_dry_run_contract(&before["dry_run"], &after["dry_run"]);
            }
        }
        (None, None) => assert_one_dry_run_contract(original, normalized),
        _ => panic!("matrix shape changed"),
    }
}
```

Add one ignored test that runs the same three command arrays, calls
`normalize_dry_run`, and prints `serde_json::to_string` for each named fixture.
Run it once with:

```console
rtk cargo test --locked --offline --test compatibility_baseline print_normalized_dry_run_baselines -- --ignored --nocapture
```

Use `apply_patch` to save only the normalized one-line JSON values. The test's
exact host-path absence assertions are authoritative across `/Users`, `/tmp`,
`/private`, `/var/folders`, `/Volumes`, or any other absolute host root. Raw
dry-run JSON is never committed.

- [ ] **Step 5: Verify GREEN and existing neighboring contracts**

```console
rtk cargo test --locked --offline --test compatibility_baseline
rtk cargo test --locked --offline --test plan_cli
rtk cargo test --locked --offline --test verify_cli
rtk cargo test --locked --offline --test receipt_contract
rtk cargo test --locked --offline --test matrix_contract
rtk cargo test --locked --offline --test verification_contract
```

Expected: every named test target passes without Docker or network.

- [ ] **Step 6: Commit Task 2**

```console
rtk git add tests/compatibility_baseline.rs tests/fixtures/compatibility
rtk git commit -m "test: pin compatibility baseline bytes"
```

### Task 3: Compile the supported public Rust facade downstream

**Files:**
- Create: `tests/fixtures/public-api-compat/Cargo.toml`
- Create: `tests/fixtures/public-api-compat/src/main.rs`
- Modify: `tests/compatibility_baseline.rs`

**Interfaces:**
- Consumes: modules and public symbols exported through `src/lib.rs`.
- Produces: a downstream compile gate that later internal refactors must preserve.

- [ ] **Step 1: Write the failing downstream check**

Add:

```rust
#[test]
fn supported_public_facade_compiles_downstream() {
    let status = Command::new(env!("CARGO"))
        .args([
            "check", "--locked", "--offline", "--quiet",
            "--manifest-path", "tests/fixtures/public-api-compat/Cargo.toml",
        ])
        .status()
        .expect("check downstream facade fixture");
    assert!(status.success());
}
```

- [ ] **Step 2: Run the focused test and verify RED**

```console
rtk cargo test --locked --offline --test compatibility_baseline supported_public_facade_compiles_downstream
```

Expected: FAIL because the downstream manifest does not exist.

- [ ] **Step 3: Add the downstream fixture manifest**

```toml
[package]
name = "ccp-public-api-compat"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
commit-ci-preflight = { path = "../../.." }

[workspace]
```

- [ ] **Step 4: Add the compile-only facade consumer**

The fixture imports the supported modules and takes function pointers without
performing I/O:

```rust
use commit_ci_preflight::{config, matrix, receipt, run, runtime, verify};

fn main() {
    let _schema: fn() -> Result<String, config::ConfigError> = config::config_schema_json;
    let _canonical = canonical_u8;
}

fn canonical_u8(value: &u8) -> Result<Vec<u8>, receipt::ReceiptError> {
    receipt::canonical_json(value)
}

fn verify_v1(
    bytes: &[u8],
    policy: &verify::VerificationPolicyV1,
    commit: &str,
    evaluated_at: &str,
) -> Result<verify::VerificationReportV1, verify::VerificationError> {
    verify::verify_receipt_document(bytes, policy, commit, evaluated_at)
}

fn verify_v2(
    bytes: &[u8],
    policy: &matrix::MatrixVerificationPolicyV2,
    commit: &str,
    evaluated_at: &str,
) -> Result<verify::VerificationReportV1, matrix::MatrixError> {
    matrix::verify_matrix_receipt_document(bytes, policy, commit, evaluated_at)
}

fn execute<'a>(
    request: &run::RunRequest<'a>,
    runtime_port: &dyn runtime::RuntimePort,
    supervisor: &dyn commit_ci_preflight::process::SupervisorPort,
    cancellation: &commit_ci_preflight::process::CancellationToken,
    clock: &dyn run::Clock,
) -> Result<run::RunOutcome, run::RunError> {
    run::execute_local_run(request, runtime_port, supervisor, cancellation, clock)
}

fn select_runtime(
    kind: config::RuntimeKind,
) -> Result<Box<dyn runtime::RuntimePort>, runtime::RuntimeError> {
    runtime::runtime_for(kind)
}
```

The unused helper functions are intentional compile-only bindings. Add
`#![allow(dead_code)]` at the top; do not execute them.

- [ ] **Step 5: Lock and verify the downstream fixture offline**

Generate its lockfile from already cached dependencies, then verify:

```console
rtk cargo generate-lockfile --offline --manifest-path tests/fixtures/public-api-compat/Cargo.toml
rtk cargo test --locked --offline --test compatibility_baseline supported_public_facade_compiles_downstream
```

Expected: PASS and no network access.

- [ ] **Step 6: Commit Task 3**

```console
rtk git add tests/compatibility_baseline.rs tests/fixtures/public-api-compat
rtk git commit -m "test: compile supported public facade downstream"
```

### Task 4: Hash manifest and M0 closure

**Files:**
- Create: `.superpowers/sdd/2026-08-30-capability-packs-clean-architecture/compatibility/manifest.json`
- Create: `.superpowers/sdd/2026-08-30-capability-packs-clean-architecture/compatibility/README.md`
- Modify: `.superpowers/sdd/2026-08-30-capability-packs-clean-architecture/progress.md`

**Interfaces:**
- Consumes: all M0 fixtures and tests.
- Produces: stable file inventory and regeneration commands for M1 comparison.

- [ ] **Step 1: Write the failing manifest validation test**

Extend the integration test imports with `std::fs`, `std::path::{Component,
Path}`, and `sha2::{Digest, Sha256}`. Add:

```rust
#[test]
fn manifest_paths_and_hashes_match() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest_path = root.join(
        ".superpowers/sdd/2026-08-30-capability-packs-clean-architecture/compatibility/manifest.json",
    );
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(&manifest_path).expect("read compatibility manifest"),
    )
    .expect("parse compatibility manifest");
    assert_eq!(manifest["schema_version"], "1.0");
    assert_eq!(
        manifest["base_commit"],
        "5fed7c443504969e62980141048f9279f9fa1dfe"
    );
    assert_eq!(manifest["hash_algorithm"], "sha256");

    let object = manifest.as_object().expect("manifest object");
    let keys = object
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        keys,
        std::collections::BTreeSet::from([
            "base_commit",
            "files",
            "hash_algorithm",
            "non_executed_surfaces",
            "schema_version",
        ])
    );
    assert_eq!(
        manifest["non_executed_surfaces"],
        serde_json::json!([
            "admission and resource decisions",
            "benchmark",
            "doctor runtime probe",
            "dry-run runtime rendering",
            "guard exec",
            "project run",
            "receipt publication"
        ])
    );

    let entries = manifest["files"].as_array().expect("file entries");
    let mut previous = None::<String>;
    for entry in entries {
        let entry_object = entry.as_object().expect("file entry object");
        let entry_keys = entry_object
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            entry_keys,
            std::collections::BTreeSet::from(["digest", "path"])
        );
        let relative = entry["path"].as_str().expect("relative path");
        let path = Path::new(relative);
        assert!(!path.is_absolute());
        assert!(path.components().all(|component| matches!(
            component,
            Component::Normal(_) | Component::CurDir
        )));
        if let Some(previous) = &previous {
            assert!(previous.as_str() < relative, "paths must be unique and sorted");
        }
        previous = Some(relative.to_owned());

        let bytes = fs::read(root.join(path)).expect("read manifested file");
        let digest = format!("sha256:{:x}", Sha256::digest(bytes));
        assert_eq!(entry["digest"].as_str(), Some(digest.as_str()), "{relative}");
    }
}
```

- [ ] **Step 2: Run the focused test and verify RED**

```console
rtk cargo test --locked --offline --test compatibility_baseline manifest_paths_and_hashes_match
```

Expected: FAIL because `manifest.json` does not exist.

- [ ] **Step 3: Generate complete SHA-256 evidence**

Run `rtk sha256sum` over every file explicitly listed in Tasks 2 and 3 plus:

```text
schema/config-v1.schema.json
schema/config-v2.schema.json
schema/receipt-v1.schema.json
schema/receipt-v2.schema.json
schema/policy-v1.schema.json
schema/policy-v1_1.schema.json
schema/policy-v2.schema.json
schema/benchmark-v1.schema.json
schema/verification-report-v1.schema.json
```

Include the now-final `tests/compatibility_baseline.rs`. Exclude
`manifest.json` itself to avoid a recursive hash. Require `rtk rg --files
schema` to return exactly this set before hashing. A
missing or additional schema stops the task and requires an explicit plan
review rather than silent omission.

- [ ] **Step 4: Write the manifest**

Write one valid JSON object with exactly five top-level members:
`schema_version`, `base_commit`, `hash_algorithm`, `files`, and
`non_executed_surfaces`. Their first three string values are respectively
`1.0`, `5fed7c443504969e62980141048f9279f9fa1dfe`, and `sha256`. `files` is a
lexicographically ordered array of objects containing exactly `path` and
`digest`; every digest is the string `sha256:` followed by the complete
lowercase value measured in Step 1.

The seven strings are `admission and resource decisions`, `benchmark`,
`doctor runtime probe`, `dry-run runtime rendering`, `guard exec`, `project
run`, and `receipt publication`. The completed manifest contains only hashes
actually emitted in Step 1; no sentinel, ellipsis, or template token is valid.

- [ ] **Step 5: Document regeneration and evidence limits**

The README records every capture command from Task 2, every focused test target
from Task 2, the downstream check from Task 3, the exact base, and these limits:
no Docker/runtime/admission or project-run behavior is proved; fixture hashes
prove byte stability only.

- [ ] **Step 6: Validate manifest paths and hashes**

Run:

```console
rtk cargo test --locked --offline --test compatibility_baseline manifest_paths_and_hashes_match
rtk git diff --check
```

Expected: PASS.

- [ ] **Step 7: Update progress and commit M0**

Record terminal commands and exact commit predecessor in `progress.md`, then:

```console
rtk git add .superpowers/sdd/2026-08-30-capability-packs-clean-architecture tests/compatibility_baseline.rs
rtk git commit -m "docs: close compatibility baseline milestone"
```

M0 is complete only after an independent review confirms every manifest path,
hash, command, and non-executed boundary.
