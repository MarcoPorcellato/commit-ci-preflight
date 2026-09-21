# M2 Historical Manifest Verification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve immutable M2 closure evidence while verifying declared bytes
from its recorded Git commit, never from an evolving checkout.

**Architecture:** Keep `m2-manifest.json` byte-for-byte unchanged. Replace its
live-tree test with a test-only verifier in `tests/capability_pack_contract.rs`
that uses CCP's existing bounded `ProcessSupervisor` abstraction to query raw
Git objects. The verifier accepts only the pinned commit and declared blobs,
clears inherited process environment, disables Git replacement/lazy fetch, and
rejects every abnormal terminal result before comparing bytes and SHA-256.

**Tech Stack:** Rust, existing `commit_ci_preflight::process` test seam,
`serde_json`, `sha2`, Cargo offline tests, Git object database.

**Spec:** `docs/superpowers/specs/2026-09-21-m2-historical-manifest-design.md`

## Global Constraints

- Leave `m2-manifest.json` byte-for-byte unchanged: it is preserved evidence
  of invalid v1.0. Add `m2-manifest-v1.1.json` as immutable corrected evidence.
- Only fixed commit `2e6286cc23584d5e82842aacf106c3bb5e7462df` may supply
  historical blobs. No working-tree fallback exists.
- Reuse CCP's `ProcessSupervisor` with two-second wall-clock deadline and
  `65_536` byte stdout/stderr capture ceiling. Do not spawn `Command` directly.
- Each Git request has exactly this cleared-environment allowlist:
  `GIT_CONFIG_NOSYSTEM=1`, `GIT_NO_REPLACE_OBJECTS=1`,
  `GIT_NO_LAZY_FETCH=1`, `GIT_OPTIONAL_LOCKS=0`, and
  `GIT_LITERAL_PATHSPECS=1`; pass `--no-replace-objects` and
  `--no-lazy-fetch` explicitly too.
- Require raw object type `commit` for base, raw type `blob` for every declared
  path, successful completion, verified cleanup, nontruncated output, and
  output within declared byte bound.
- Missing local history, shallow/partial/archive checkout, missing Git,
  malformed manifest input, process/capture/cleanup error, timeout, nonzero
  exit, or mismatch is deterministic failure.
- Modify production source, CLI, receipt, schema, policy, Docker, CCP runtime,
  dependencies, and stable executable: never.
- All commands begin with `rtk`. No network, CCP guard, Docker, push, PR,
  merge, or installation is in scope.

## Review Focus

- Later `CHANGELOG.md` changes must not rewrite M2 evidence; Task 2 proves
  bytes come from historical blobs, not `root.join(path)`.
- Historical base must be raw `commit`, not tree or replacement object; Task 1
  injects `tree` response and checks Git hardening flags.
- Invalid identity/path must result in zero Git calls; Task 1 counts calls for
  malformed identity and traversal/colon paths.
- Timeout, truncation, read/cleanup error, nonzero exit all fail closed; Task 1
  injects every terminal outcome through `SupervisorPort`.
- CI must retain M2 base object; Task 3 sets full history only in Linux/macOS
  test matrix, then explicitly fetches fixed object from canonical
  `${{ github.repository }}` because it is not reachable from `main`, and
  documents archive/shallow failure.

---

## File Structure

| Path | Responsibility |
|---|---|
| `tests/capability_pack_contract.rs` | Test-only verifier, hardened Git requests, injected-process tests, legacy-invalid and corrected historical contracts. |
| `docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/m2-manifest-v1.1.json` | Corrected immutable M2 evidence for pinned historical base. |
| `.github/workflows/rust-ci.yml` | Full Git history plus exact canonical-base provisioning in hosted matrix executing historical-object test. |
| `docs/TESTING_AND_FAULT_INJECTION.md` | Deterministic-environment prerequisite for M2 historical objects. |
| `docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/progress.md` | Corrected historical—not live checkout—closure semantics. |

### Task 1: Bounded test-only historical Git reader

**Files:**

- Modify: `tests/capability_pack_contract.rs:14-120`

**Interfaces:**

- Produces `const M2_BASE_COMMIT: &str` and
  `const HISTORICAL_CAPTURE_BYTES: usize = 65_536`.
- Produces test-only `HistoricalManifestError` and
  `HistoricalGitReader<R: SupervisorPort>`.
- `HistoricalGitReader::object_type(&self, object: &str) -> Result<String, HistoricalManifestError>`.
- `HistoricalGitReader::blob(&self, path: &str, declared_bytes: u64) -> Result<Vec<u8>, HistoricalManifestError>`.
- Every request uses distinct `RunIdentity`, `GenerationGuard`, and
  `CancellationToken`; no shared state or admission.

- [ ] **Step 1: Write RED tests for request construction and no-call validation**

Add `ScriptedSupervisor` in this integration-test file. It owns
`Mutex<Vec<ProcessRequest>>` plus scripted `Result<ProcessResult, ProcessError>`
responses. Build results with `CleanupStatus::Verified`,
`ProcessTermination::Completed`, and `CapturedStream::from_captured`.

```rust
#[test]
fn historical_reader_rejects_invalid_input_without_calling_git() {
    let supervisor = ScriptedSupervisor::default();
    let reader = HistoricalGitReader::new(repo_root(), &supervisor);

    assert!(reader.object_type("not-a-commit").is_err());
    assert!(reader.blob("../CHANGELOG.md", 1).is_err());
    assert!(reader.blob("CHANGELOG.md:other", 1).is_err());
    assert_eq!(supervisor.call_count(), 0);
}

#[test]
fn historical_reader_builds_hardened_git_request() {
    let supervisor = ScriptedSupervisor::with_success_stdout(b"commit\n");
    let reader = HistoricalGitReader::new(repo_root(), &supervisor);

    assert_eq!(reader.object_type(M2_BASE_COMMIT).unwrap(), "commit");
    let request = supervisor.only_request();
    assert_eq!(request.program, "git");
    assert_eq!(request.timeout, Duration::from_secs(2));
    assert_eq!(request.max_capture_bytes, HISTORICAL_CAPTURE_BYTES);
    assert_eq!(request.environment, historical_git_environment());
}
```

- [ ] **Step 2: Verify RED**

Run:

```bash
rtk cargo test --locked --offline --test capability_pack_contract historical_reader_ -- --nocapture
```

Expected: FAIL because reader, fixture supervisor, hardened request builder do
not yet exist.

- [ ] **Step 3: Implement minimum typed reader using existing supervisor**

Import public process types:

```rust
use commit_ci_preflight::process::{
    CancellationToken, CleanupStatus, GenerationGuard, ProcessRequest,
    ProcessResult, ProcessSupervisor, ProcessTermination, RunIdentity,
    StdProcessSpawner, SupervisorPort,
};
```

Use this environment and request construction. `StdProcessSpawner` already
uses `env_clear`; pass no other environment values.

```rust
fn historical_git_environment() -> BTreeMap<OsString, OsString> {
    BTreeMap::from([
        ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
        ("GIT_NO_REPLACE_OBJECTS".into(), "1".into()),
        ("GIT_NO_LAZY_FETCH".into(), "1".into()),
        ("GIT_OPTIONAL_LOCKS".into(), "0".into()),
        ("GIT_LITERAL_PATHSPECS".into(), "1".into()),
    ])
}

fn historical_request(root: &Path, args: Vec<OsString>) -> ProcessRequest {
    ProcessRequest {
        identity: RunIdentity {
            project: "m2-historical-contract".to_owned(),
            commit: Some(M2_BASE_COMMIT.to_owned()),
            config_digest: "sha256:m2-historical-contract".to_owned(),
            generation: "test".to_owned(),
        },
        program: "git".into(), argv: args, current_dir: root.to_path_buf(),
        environment: historical_git_environment(), timeout: Duration::from_secs(2),
        max_capture_bytes: HISTORICAL_CAPTURE_BYTES,
    }
}
```

Validate strict lowercase 40-hex IDs, pin `object_type` input to
`M2_BASE_COMMIT`, validate nonempty relative paths without `..`, colon, NUL,
backslash, or absolute prefix, and reject declared `u64` above capture limit
using `usize::try_from`. Execute through `SupervisorPort::execute`; map every
`ProcessError` to typed test error. Require `Completed`, `Verified`, successful
exit, nontruncated stdout/stderr before consuming output. `object_type` accepts
only UTF-8 `commit\n` or `blob\n`; `blob` checks base commit, then `git
cat-file -t <base>:<path>`, then `git cat-file blob <base>:<path>`, exact size.
No `expect`, `unwrap`, unchecked cast, or panic in helpers.

- [ ] **Step 4: Add injected terminal-failure tests**

Use scripted results/errors to prove each returns `Err`:

```rust
#[test]
fn historical_reader_rejects_non_commit_base() {
    let supervisor = ScriptedSupervisor::with_success_stdout(b"tree\n");
    let reader = HistoricalGitReader::new(repo_root(), &supervisor);
    assert!(reader.object_type(M2_BASE_COMMIT).is_err());
}
#[test]
fn historical_reader_rejects_missing_or_non_blob_path() {
    let supervisor = ScriptedSupervisor::with_responses([
        scripted_success(b"commit\n"), scripted_nonzero(b"missing\n"),
    ]);
    let reader = HistoricalGitReader::new(repo_root(), &supervisor);
    assert!(reader.blob("CHANGELOG.md", 1).is_err());
}
#[test]
fn historical_reader_rejects_timeout_and_truncated_capture() {
    for result in [scripted_timeout(), scripted_truncated_stdout()] {
        let supervisor = ScriptedSupervisor::with_result(result);
        let reader = HistoricalGitReader::new(repo_root(), &supervisor);
        assert!(reader.object_type(M2_BASE_COMMIT).is_err());
    }
}
#[test]
fn historical_reader_rejects_supervisor_and_cleanup_errors() {
    for result in [Err(scripted_output_error()), Err(scripted_cleanup_error())] {
        let supervisor = ScriptedSupervisor::with_result(result);
        let reader = HistoricalGitReader::new(repo_root(), &supervisor);
        assert!(reader.object_type(M2_BASE_COMMIT).is_err());
    }
}
#[test]
fn historical_reader_rejects_blob_length_mismatch() {
    let supervisor = ScriptedSupervisor::with_responses([
        scripted_success(b"commit\n"), scripted_success(b"blob\n"),
        scripted_success(b"short"),
    ]);
    let reader = HistoricalGitReader::new(repo_root(), &supervisor);
    assert!(reader.blob("CHANGELOG.md", 6).is_err());
}
```

Each test asserts no `root.join(path)` read. Existing production supervisor
unit tests remain lifecycle proof; these tests prove M2 propagates bounded
terminal states fail-closed.

- [ ] **Step 5: Verify GREEN and commit Task 1**

```bash
rtk cargo test --locked --offline --test capability_pack_contract historical_reader_ -- --nocapture
rtk git diff --check
rtk git add tests/capability_pack_contract.rs
rtk git commit -m "test: harden historical M2 object verification"
```

Expected: focused reader tests pass; no diff errors.

### Task 2: Replace live M2 assertion with immutable-object contract

**Files:**

- Create: `docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/m2-manifest-v1.1.json`
- Modify: `tests/capability_pack_contract.rs:54-120`
- Modify: `docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/progress.md:25`

**Interfaces:**

- Consumes `HistoricalGitReader<R>` for any injected `R: SupervisorPort`, with
  production test using `R = ProcessSupervisor<StdProcessSpawner>`.
- Produces `verify_m2_manifest_historical(root, manifest, reader) -> Result<(), HistoricalManifestError>`.
- Public corrected-record test may use `expect` only at final test assertion.

- [ ] **Step 1: Write RED tests for manifest semantics and no fallback**

```rust
#[test]
fn m2_manifest_rejects_missing_historical_blob_without_live_tree_fallback() {
    let manifest = parsed_m2_manifest();
    let supervisor = ScriptedSupervisor::with_nonzero_exit();
    let reader = HistoricalGitReader::new(repo_root(), &supervisor);

    assert!(verify_m2_manifest_historical(repo_root(), &manifest, &reader).is_err());
    assert!(supervisor.call_count() > 0);
}

#[test]
fn m2_manifest_rejects_bad_declared_length_and_digest() {
    let mut manifest = parsed_m2_manifest();
    manifest["files"][0]["bytes"] = serde_json::json!(65_537_u64);
    let supervisor = ScriptedSupervisor::default();
    let reader = HistoricalGitReader::new(repo_root(), &supervisor);
    assert!(verify_m2_manifest_historical(repo_root(), &manifest, &reader).is_err());
    assert_eq!(supervisor.call_count(), 0);
}
```

Also cover unexpected top-level/entry keys, unexpected or reordered path list,
bad SHA-256 format, and non-commit raw base through injected reader.

- [ ] **Step 2: Verify RED**

```bash
rtk cargo test --locked --offline --test capability_pack_contract m2_manifest_ -- --nocapture
```

Expected: live-tree test fails on `CHANGELOG.md`; helper symbols absent until
implementation. Add RED proving legacy v1.0 file hash is preserved and its
historical verification returns an error, while v1.1 positive contract is not
yet present.

- [ ] **Step 3: Implement strict manifest verifier and real contract**

Keep exact top-level `{base_commit, files, schema_version}`, schema `"1.0"`,
fixed `M2_BASE_COMMIT`, exact sorted `EXPECTED_PATHS`, exact entry
`{bytes, path, sha256}`. Create v1.1 by retaining all v1.0 entries except
the three byte/SHA-256 values proven inconsistent with base `2e6286…`. Parse
bytes as `u64`; validate lower-case `sha256:` plus 64 hex characters before
reads. Each entry invokes only:

```rust
let bytes = reader.blob(relative_path, declared_bytes)?;
if u64::try_from(bytes.len()).map_err(|_| HistoricalManifestError::Length)? != declared_bytes {
    return Err(HistoricalManifestError::Length);
}
if sha256_prefixed(&bytes) != declared_digest {
    return Err(HistoricalManifestError::Digest);
}
```

Instantiate real reader with `ProcessSupervisor::new(StdProcessSpawner)`.
Replace `m2_manifest_matches_exact_file_bytes` with
`m2_legacy_manifest_is_preserved_and_rejected` and
`m2_corrected_manifest_matches_historical_git_objects`. Both read manifest
metadata from checkout; manifested bytes come only from raw Git blobs. Never
edit legacy manifest.

- [ ] **Step 4: Document corrected evidence semantics**

Replace M2 closure line in `progress.md` with:

```markdown
M2 closure evidence is immutable and is verified from the declared raw blobs
at its recorded `base_commit`; it is not a mutable inventory of the checkout.
```

- [ ] **Step 5: Verify GREEN and commit Task 2**

```bash
rtk cargo test --locked --offline --test capability_pack_contract m2_manifest_ -- --nocapture
rtk cargo test --locked --offline --test capability_pack_contract historical_reader_ -- --nocapture
rtk git diff --check
rtk git add tests/capability_pack_contract.rs docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/progress.md
rtk git commit -m "test: verify M2 closure from historical Git objects"
```

Expected: corrected historical contract and injected fail-closed tests pass;
legacy v1.0 preservation test proves typed rejection.

### Task 3: Make deterministic test environments history-complete

**Files:**

- Modify: `.github/workflows/rust-ci.yml:58-67`
- Modify: `docs/TESTING_AND_FAULT_INJECTION.md:after "Deterministic seams already available"`

**Interfaces:**

- Hosted Linux/macOS `test` checkout has `fetch-depth: 0`.
- Quality and Windows compile checkouts remain `fetch-depth: 1` because they do
  not run historical-object test.

- [ ] **Step 1: Write RED inspection assertion**

Before editing, verify test matrix currently has `fetch-depth: 1`; record it
cannot guarantee object `2e6286cc23584d5e82842aacf106c3bb5e7462df`.

- [ ] **Step 2: Change only matrix checkout history**

Set test job checkout as:

```yaml
# M2 historical-object contract reads its fixed base commit.
fetch-depth: 0
```

Do not change quality or Windows depth. Add testing-document section: M2 needs
`git` and all objects for fixed base locally; archives, shallow checkouts,
partial clones missing objects fail; `GIT_NO_LAZY_FETCH=1` prevents network
recovery.

- [ ] **Step 3: Verify configuration and full offline gate**

```bash
rtk rg -n -U "name: Test|M2 historical-object|fetch-depth" .github/workflows/rust-ci.yml
rtk cargo fmt --all -- --check
rtk cargo test --workspace --all-targets --locked --offline
rtk cargo clippy --workspace --all-targets --locked --offline -- -D warnings
rtk git diff --check
```

Expected: only Linux/macOS test matrix uses depth zero; suite, formatting,
strict Clippy pass.

- [ ] **Step 4: Commit Task 3**

```bash
rtk git add .github/workflows/rust-ci.yml docs/TESTING_AND_FAULT_INJECTION.md
rtk git commit -m "ci: retain M2 historical verification objects"
```

## Final Review Gate

- [ ] Confirm `m2-manifest.json` has no diff, `m2-manifest-v1.1.json` is the
  only new evidence record, and `CHANGELOG.md` retains unrelated `macos-v5`
  entry.
- [ ] Confirm every historical Git request has bounded supervisor lifecycle,
  exact environment allowlist, `--no-replace-objects`, `--no-lazy-fetch`, raw
  commit/blob checks, no current-tree fallback.
- [ ] Confirm invalid inputs make zero Git calls; all injected abnormal process
  outcomes return `Err`.
- [ ] Request independent review of test-only process use, parser strictness,
  Git identity, CI history depth, fail-closed errors.
- [ ] Do not rebuild, qualify, install, push, open PR, or run CCP until new
  exact head and binary envelope receive separate authorization.
