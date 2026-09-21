# M2 Historical Manifest Verification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve the immutable M2 capability-pack closure while proving its
declared bytes against its recorded Git commit instead of the evolving checkout.

**Architecture:** Keep `m2-manifest.json` unchanged as a schema-`1.0`
historical record. Add a test-only, bounded Git-object reader and make the M2
contract test use it for each fixed manifest path. The reader rejects malformed
identity, traversal, oversized declarations, failed Git reads, and mismatches;
it never falls back to the working tree.

**Tech Stack:** Rust test support, `std::process::Command`, `serde_json`,
`sha2`, Cargo offline tests.

**Spec:** `docs/superpowers/specs/2026-09-21-m2-historical-manifest-design.md`

## Global Constraints

- Leave `m2-manifest.json` byte-for-byte unchanged: schema, base commit,
  declared paths, lengths, and digests remain M2 historical evidence.
- Read only fixed test paths from commit
  `2e6286cc23584d5e82842aacf106c3bb5e7462df`.
- Reject a declared size above `65_536` bytes before spawning Git; capture at
  most `65_537` stdout bytes and kill the child on overflow.
- A missing Git executable, non-repository root, bad revision, missing blob,
  nonzero Git status, malformed output, or any mismatch is a test failure.
- Do not change production source, CLI, schemas, receipts, policies, Docker,
  CCP behavior, dependencies, or the stable executable.
- All commands use `rtk`; no network, CCP guard, Docker, push, PR, merge, or
  installation is in scope.

## Review Focus

- A later `CHANGELOG.md` change must not rewrite or invalidate the M2 closure;
  Task 2 replaces the observed live-tree RED with historical-object validation.
- A missing historical Git object must fail rather than use a live file; Task 2
  mutates the in-memory base revision to forty zeroes.
- An oversized declaration must fail before object output is read; Task 1
  exercises `65_537` bytes.
- A traversal or separator-injection path must fail before command construction;
  Task 1 exercises `../CHANGELOG.md` and `CHANGELOG.md:other`.
- Git output larger than its declared bound must stop the child and fail;
  Task 1 exercises the bounded reader with a `Cursor` containing `65_537`
  bytes and an overflow callback.

---

## File Structure

| Path | Responsibility |
|---|---|
| `tests/support/historical_git_object.rs` | Test-only validated, bounded Git blob reader and deterministic command seam. |
| `tests/capability_pack_contract.rs` | M2 manifest parsing plus historical-closure and fail-closed tests. |
| `docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/progress.md` | Corrected statement that M2 binds historical Git objects, not live files. |

### Task 1: Bounded historical Git-object reader

**Files:**

- Create: `tests/support/historical_git_object.rs`
- Modify: `tests/capability_pack_contract.rs:13-18`

**Interfaces:**

- Produces `MAX_HISTORICAL_BLOB_BYTES: usize = 65_536`.
- Produces `HistoricalGitObjectError` with
  `InvalidCommit`, `InvalidPath`, `DeclaredSizeTooLarge`, `Spawn`, `GitFailed`,
  `OutputTooLarge`, and `SizeMismatch` variants.
- Produces `read_blob(repo_root, commit, relative_path, declared_bytes)` that
  returns the exact blob bytes or a typed error.
- Produces test-visible `capture_bounded(reader, maximum, on_overflow)`, which
  returns `OutputTooLarge` after invoking `on_overflow` when it reads byte
  `maximum + 1`.

- [ ] **Step 1: Add RED tests for untrusted reader inputs**

Add this module declaration near the current imports and these tests before any
reader implementation:

```rust
#[path = "support/historical_git_object.rs"]
mod historical_git_object;

#[test]
fn historical_reader_rejects_invalid_identity_before_git() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(matches!(
        historical_git_object::read_blob(root, "not-a-commit", "CHANGELOG.md", 1),
        Err(historical_git_object::HistoricalGitObjectError::InvalidCommit)
    ));
    assert!(matches!(
        historical_git_object::read_blob(root, "2e6286cc23584d5e82842aacf106c3bb5e7462df", "../CHANGELOG.md", 1),
        Err(historical_git_object::HistoricalGitObjectError::InvalidPath)
    ));
    assert!(matches!(
        historical_git_object::read_blob(root, "2e6286cc23584d5e82842aacf106c3bb5e7462df", "CHANGELOG.md:other", 1),
        Err(historical_git_object::HistoricalGitObjectError::InvalidPath)
    ));
}

#[test]
fn historical_reader_stops_at_the_capture_bound() {
    let mut overflowed = false;
    let result = historical_git_object::capture_bounded(
        std::io::Cursor::new(vec![0_u8; historical_git_object::MAX_HISTORICAL_BLOB_BYTES + 1]),
        historical_git_object::MAX_HISTORICAL_BLOB_BYTES,
        || overflowed = true,
    );
    assert!(matches!(result, Err(historical_git_object::HistoricalGitObjectError::OutputTooLarge)));
    assert!(overflowed);
}

#[test]
fn historical_reader_rejects_oversized_declaration_before_git() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(matches!(
        historical_git_object::read_blob(
            root,
            "2e6286cc23584d5e82842aacf106c3bb5e7462df",
            "CHANGELOG.md",
            historical_git_object::MAX_HISTORICAL_BLOB_BYTES + 1,
        ),
        Err(historical_git_object::HistoricalGitObjectError::DeclaredSizeTooLarge { .. })
    ));
}
```

- [ ] **Step 2: Verify RED**

Run:

```bash
rtk cargo test --locked --offline --test capability_pack_contract historical_reader_ -- --nocapture
```

Expected: compilation failure because `tests/support/historical_git_object.rs`
does not exist.

- [ ] **Step 3: Implement the minimum validated reader**

Create `tests/support/historical_git_object.rs`. Accept only a 40-character
lowercase hexadecimal commit. Accept a nonempty relative path containing no
`..`, colon, NUL, backslash, or absolute prefix. Reject declared bytes above
the constant before creating a `Command`.

Use this exact command shape after validation:

```rust
Command::new("git")
    .arg("-C")
    .arg(repo_root)
    .arg("cat-file")
    .arg("blob")
    .arg(format!("{commit}:{relative_path}"))
```

Pipe stdout, set `stdin(Stdio::null())`, and discard stderr. Implement
`capture_bounded` with `Read::take(maximum + 1)`: it invokes its overflow
closure and returns `OutputTooLarge` when the extra byte exists. Call it over
the child stdout with a closure that kills the child. Always wait for the child
after the bounded read. Otherwise require success, then require
`bytes.len() == declared_bytes`. Return the exact bytes only after those checks.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
rtk cargo test --locked --offline --test capability_pack_contract historical_reader_ -- --nocapture
```

Expected: 3 passed.

- [ ] **Step 5: Commit Task 1**

```bash
rtk git add tests/support/historical_git_object.rs tests/capability_pack_contract.rs
rtk git commit -m "test: bound historical M2 Git object reads"
```

### Task 2: Historical M2 closure contract

**Files:**

- Modify: `tests/capability_pack_contract.rs:54-120`
- Modify: `docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/progress.md`

**Interfaces:**

- Consumes `historical_git_object::read_blob` from Task 1.
- Produces `verify_m2_manifest_historical(root, manifest) -> Result<(), String>`
  plus `m2_manifest_matches_historical_git_objects`; the manifest file itself
  is not modified.

- [ ] **Step 1: Preserve the observed RED and rename the contract**

The currently committed `m2_manifest_matches_exact_file_bytes` has already
failed on the changed live `CHANGELOG.md`; retain that terminal output as the
RED evidence. Rename the test to `m2_manifest_matches_historical_git_objects`
and move its parsing/shape/path checks into this helper signature:

```rust
fn verify_m2_manifest_historical(
    root: &Path,
    manifest: &serde_json::Value,
) -> Result<(), String>
```

The helper returns `Err` for every malformed top-level field, unexpected path,
non-array `files`, oversized byte declaration, reader failure, byte mismatch,
or SHA-256 mismatch. The public test parses the checked-in manifest and calls
the helper with `expect("historical M2 closure")`.

- [ ] **Step 2: Implement historical verification**

For each manifest entry inside `verify_m2_manifest_historical`, replace:

```rust
let bytes = std::fs::read(root.join(relative)).expect("read manifested file");
```

with:

```rust
let bytes = historical_git_object::read_blob(
    root,
    manifest["base_commit"].as_str().expect("M2 base commit"),
    relative,
    entry["bytes"].as_u64().expect("M2 declared bytes") as usize,
)
.unwrap_or_else(|error| panic!("read historical M2 object {relative}: {error:?}"));
```

Keep the existing byte-length and lowercase SHA-256 assertions unchanged.
Add this in-memory mutation test:

```rust
#[test]
fn m2_manifest_rejects_an_unresolvable_historical_commit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest_path = root.join(
        "docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/m2-manifest.json",
    );
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(manifest_path).expect("read M2 manifest"))
            .expect("parse M2 manifest");
    manifest["base_commit"] = serde_json::Value::String("0".repeat(40));
    assert!(verify_m2_manifest_historical(root, &manifest).is_err());
}
```

It must not read any working-tree path.

- [ ] **Step 3: Verify focused GREEN**

Run:

```bash
rtk cargo test --locked --offline --test capability_pack_contract m2_manifest_matches_historical_git_objects -- --exact
rtk cargo test --locked --offline --test capability_pack_contract historical_reader_ -- --nocapture
```

Expected: the historical M2 test and all three reader tests pass.

- [ ] **Step 4: Document the semantic correction**

Replace the M2 closure sentence in `progress.md` with normal prose stating:

```markdown
M2 closure evidence is immutable and is verified from the declared blobs at
its recorded `base_commit`; it is not a mutable inventory of the checkout.
```

Do not alter `m2-manifest.json` or its recorded commit/hash values.

- [ ] **Step 5: Run the full deterministic gate**

```bash
rtk cargo fmt --all -- --check
rtk cargo test --workspace --all-targets --locked --offline
rtk cargo clippy --workspace --all-targets --locked --offline -- -D warnings
rtk git diff --check
```

Expected: all deterministic tests pass, no Clippy warnings, and no diff errors.

- [ ] **Step 6: Commit Task 2**

```bash
rtk git add tests/capability_pack_contract.rs docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/progress.md
rtk git commit -m "test: verify M2 closure from historical Git objects"
```

## Final Review Gate

- [ ] Confirm `m2-manifest.json` has no diff.
- [ ] Confirm `CHANGELOG.md` contains the `macos-v5` entry and differs from
  the M2 historical blob without invalidating its proof.
- [ ] Request an independent review of test-only process spawning, path
  validation, output bounds, and fail-closed error handling.
- [ ] Do not rebuild, qualify, install, push, open a PR, or run CCP until a
  new exact head and binary envelope are authorized.
