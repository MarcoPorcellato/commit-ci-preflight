# Opaque Cache-Payload Symbolic Links Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make completed Unix cache generations containing ordinary symbolic
links safely reusable while preserving CCP's strict link-free control plane and
cleaning every newly created failed staging generation.

**Architecture:** Add one private payload-tree module that owns bounded
no-follow traversal, accounting, and Unix link-preserving copy. Keep cache
layout, promotion, recovery, and ownership decisions in `cache.rs`, and call
the payload module only after validating an exact plain `data` root. Move the
prepared-generation RAII owner before fallible reuse so cleanup covers both
pre-manifest and manifest-backed staging phases.

**Tech Stack:** Rust 2024, Rust 1.87+, standard-library filesystem APIs,
existing `fs2`, `serde`, and `sha2` dependencies; deterministic unit and
contract tests with no new dependency.

**Spec:**
`docs/superpowers/specs/2026-08-29-cache-payload-symlink-design.md`

## Global Constraints

- The implementation baseline is
  `820a7fa6ce83a7ac8593c2800f8be4f44ab82ebc` plus approved design commit
  `7adc340ca39c18a5ea31e7a1df054f97629876f9`.
- Symbolic links remain forbidden in the cache root, fixed root directories,
  entry root, locks, markers, manifests, journals, generation roots, payload
  roots, cache-pin sources, and mount sources.
- Only descendants of exact plain `entries/<key>/data`,
  `entries/<key>/.staging-*/data`, or
  `entries/<key>/.backup-*/data` roots may be opaque links.
- Host code must never stat, open, canonicalize, traverse, copy, or otherwise
  follow a payload link target.
- Relative, absolute, broken, recursive, and outside-root targets are preserved
  as target text on Unix.
- Windows link-bearing payload reuse remains fail-closed; link-free Windows
  behavior remains unchanged.
- No configuration, receipt, policy, cache-key, inventory JSON,
  generation-manifest, or promotion-journal schema changes.
- No new dependency, network operation, Docker invocation, admission mutation,
  host-cache mutation, or CCP heavy command in deterministic development.
- Each task follows red-green-refactor and ends in one reviewable commit.
- Installed producer replacement, adopter execution, receipt publication,
  push, PR, merge, and release remain separate authorization gates.

## File Structure

- Create `src/cache_payload.rs`: private bounded payload traversal, accounting,
  trace seam, Unix link recreation, and focused unit tests.
- Modify `src/lib.rs`: declare the private `cache_payload` module.
- Modify `src/cache.rs`: map payload errors, classify exact payload roots,
  integrate inventory and reuse, move staging ownership earlier, and validate
  payloads before promotion.
- Modify `tests/repository_hygiene_contract.rs`: pin the public documentation
  boundary and its exact deterministic test references.
- Modify `docs/CACHE_AND_WORKSPACE.md`: document control-plane versus payload-
  plane behavior and inventory accounting.
- Modify `docs/THREAT_MODEL.md`: add the host no-follow threat and residual
  container/runtime boundary.
- Modify `docs/TESTING_AND_FAULT_INJECTION.md`: record focused deterministic
  link, cleanup, and platform tests.
- Modify `CHANGELOG.md`: describe the user-visible cache-reuse correction.
- Modify `docs/adr/0006-opaque-cache-payload-symlinks.md`: already accepted;
  change it only if implementation discovers a decision-level contradiction.

---

### Task 1: Private bounded payload traversal

**Files:**

- Create: `src/cache_payload.rs`
- Modify: `src/lib.rs:18-31`
- Modify: `src/cache.rs:1697-1785`
- Test: `src/cache_payload.rs` inline unit-test module

**Interfaces:**

- Consumes: `crate::cache::CacheError`, `std::path::{Path, PathBuf}`, and a
  caller-supplied `&mut usize` node counter plus node limit.
- Produces:
  `PayloadTreeStats { pub(crate) bytes: u64, pub(crate) files: u64 }`,
  `measure_payload_tree(&Path, &mut usize, usize)`,
  `validate_payload_tree(&Path, &mut usize, usize)`, and the private test trace
  seam `take_payload_operations()`.

- [ ] **Step 1: Declare the module and the typed cache errors**

Add the private module to `src/lib.rs`:

```rust
pub mod cache;
mod cache_payload;
pub mod config;
```

Add these variants to `CacheError` in `src/cache.rs`:

```rust
PayloadSymlinkUnsupported(PathBuf),
PayloadSymlinkRead {
    path: PathBuf,
    source: io::Error,
},
PayloadSymlinkCreate {
    path: PathBuf,
    source: io::Error,
},
```

Map all three to exit code `70`. Keep paths out of `Display`: use
`"cache payload symbolic links are unsupported on this platform"`,
`"cache payload symbolic-link target could not be read"`, and
`"cache payload symbolic link could not be created"`. Return the nested
`io::Error` from `std::error::Error::source` for the read/create variants.

- [ ] **Step 2: Write the failing Unix traversal tests**

Create `src/cache_payload.rs` with the imports, type signature, and tests first.
The main fixture must use all five link classes without reading their targets:

```rust
#[cfg(test)]
static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
fn payload_fixture(name: &str) -> PathBuf {
    let path = std::env::current_dir()
        .unwrap()
        .parent()
        .unwrap()
        .join(format!(
            ".ccp-payload-test-{}-{}-{name}",
            std::process::id(),
            TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir(&path).unwrap();
    path
}

#[cfg(test)]
fn remove_fixture(root: &Path, outside: &Path) {
    if root.exists() {
        fs::remove_dir_all(root).unwrap();
    }
    if outside.exists() {
        fs::remove_file(outside).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn payload_measurement_counts_links_without_following_targets() {
    use std::os::unix::{ffi::OsStrExt, fs::symlink};

    let fixture = payload_fixture("measure-links");
    let outside = fixture.parent().unwrap().join("outside-sentinel");
    fs::write(&outside, b"do not read or change").unwrap();
    fs::write(fixture.join("regular"), b"abc").unwrap();
    symlink("regular", fixture.join("relative")).unwrap();
    symlink("missing", fixture.join("broken")).unwrap();
    symlink(&outside, fixture.join("absolute-external")).unwrap();
    symlink("self", fixture.join("self")).unwrap();
    symlink("cycle-b", fixture.join("cycle-a")).unwrap();
    symlink("cycle-a", fixture.join("cycle-b")).unwrap();
    fs::create_dir(fixture.join("nested")).unwrap();
    symlink("../relative", fixture.join("nested/recursive")).unwrap();

    clear_payload_operations();
    let mut nodes = 0;
    let stats = measure_payload_tree(&fixture, &mut nodes, 100).unwrap();
    let target_bytes = [
        "regular",
        "missing",
        "self",
        "cycle-b",
        "cycle-a",
        "../relative",
    ]
        .into_iter()
        .map(|target| target.as_bytes().len() as u64)
        .sum::<u64>()
        + outside.as_os_str().as_bytes().len() as u64;

    assert_eq!(stats.files, 8);
    assert_eq!(stats.bytes, 3 + target_bytes);
    assert_eq!(fs::read(&outside).unwrap(), b"do not read or change");
    assert!(take_payload_operations().iter().all(|operation| {
        !operation.filesystem_path().starts_with(&outside)
    }));
    remove_fixture(&fixture, &outside);
}
```

Add three more tests with exact names:

```rust
#[cfg(unix)]
#[test]
fn payload_root_itself_must_be_a_plain_directory() {
    use std::os::unix::fs::symlink;

    let real = payload_fixture("plain-root-real");
    let link = real.parent().unwrap().join("plain-root-link");
    let _ = fs::remove_file(&link);
    symlink(&real, &link).unwrap();
    let mut nodes = 0;
    assert!(matches!(
        measure_payload_tree(&link, &mut nodes, 100),
        Err(CacheError::SymlinkInManagedRoot(path)) if path == link
    ));
    fs::remove_file(link).unwrap();
    fs::remove_dir(real).unwrap();
}

#[cfg(unix)]
#[test]
fn unsupported_payload_object_fails_without_traversal() {
    use std::os::unix::net::UnixListener;

    let fixture = payload_fixture("unsupported-object");
    let socket = fixture.join("listener.socket");
    let listener = UnixListener::bind(&socket).unwrap();
    let mut nodes = 0;
    assert!(matches!(
        measure_payload_tree(&fixture, &mut nodes, 100),
        Err(CacheError::UnexpectedEntry(path)) if path == socket
    ));
    drop(listener);
    fs::remove_file(socket).unwrap();
    fs::remove_dir(fixture).unwrap();
}

#[test]
fn payload_node_limit_is_fail_closed() {
    let fixture = payload_fixture("node-limit");
    fs::write(fixture.join("a"), b"a").unwrap();
    fs::write(fixture.join("b"), b"b").unwrap();
    let mut nodes = 0;
    assert!(matches!(
        measure_payload_tree(&fixture, &mut nodes, 2),
        Err(CacheError::InventoryLimitExceeded)
    ));
    fs::remove_dir_all(fixture).unwrap();
}
```

- [ ] **Step 3: Run the tests to verify the red state**

Run:

```console
cargo test --locked cache_payload::tests:: -- --nocapture
```

Expected: compilation fails because `PayloadTreeStats`,
`measure_payload_tree`, and the trace helpers are not defined.

- [ ] **Step 4: Implement the bounded no-follow walker**

Use this exact public-to-crate surface:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PayloadTreeStats {
    pub(crate) bytes: u64,
    pub(crate) files: u64,
}

pub(crate) fn measure_payload_tree(
    root: &Path,
    nodes: &mut usize,
    node_limit: usize,
) -> Result<PayloadTreeStats, CacheError> {
    let metadata = traced_symlink_metadata(root)?;
    if metadata.file_type().is_symlink() {
        return Err(CacheError::SymlinkInManagedRoot(root.to_path_buf()));
    }
    if !metadata.is_dir() {
        return Err(CacheError::UnexpectedEntry(root.to_path_buf()));
    }
    walk_payload(root, nodes, node_limit)
}

pub(crate) fn validate_payload_tree(
    root: &Path,
    nodes: &mut usize,
    node_limit: usize,
) -> Result<(), CacheError> {
    measure_payload_tree(root, nodes, node_limit).map(|_| ())
}
```

`walk_payload` must increment and check the node counter before inspecting each
object, use `symlink_metadata`, sort `read_dir` entries by `file_name`, and use
checked addition for bytes/files. Its link arm on Unix is:

```rust
if metadata.file_type().is_symlink() {
    let target = traced_read_link(path).map_err(|source| {
        CacheError::PayloadSymlinkRead {
            path: path.to_path_buf(),
            source,
        }
    })?;
    use std::os::unix::ffi::OsStrExt;
    return Ok(PayloadTreeStats {
        bytes: target.as_os_str().as_bytes().len() as u64,
        files: 1,
    });
}
```

Implement the test-only operation recorder used by the tests above:

```rust
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum PayloadOperation {
    SymlinkMetadata(PathBuf),
    ReadDirectory(PathBuf),
    ReadLink(PathBuf),
}

#[cfg(test)]
impl PayloadOperation {
    fn filesystem_path(&self) -> &Path {
        match self {
            Self::SymlinkMetadata(path)
            | Self::ReadDirectory(path)
            | Self::ReadLink(path) => path,
        }
    }
}

#[cfg(test)]
thread_local! {
    static PAYLOAD_OPERATIONS: RefCell<Vec<PayloadOperation>> = const {
        RefCell::new(Vec::new())
    };
}

#[cfg(test)]
fn record_payload_operation(operation: PayloadOperation) {
    PAYLOAD_OPERATIONS.with(|operations| operations.borrow_mut().push(operation));
}

#[cfg(test)]
fn clear_payload_operations() {
    PAYLOAD_OPERATIONS.with(|operations| operations.borrow_mut().clear());
}

#[cfg(test)]
fn take_payload_operations() -> Vec<PayloadOperation> {
    PAYLOAD_OPERATIONS.with(|operations| std::mem::take(&mut *operations.borrow_mut()))
}
```

`traced_symlink_metadata`, `traced_read_directory`, and `traced_read_link`
record the matching operation immediately before calling the standard-library
function. The `record_payload_operation` call is enclosed in `#[cfg(test)]`, so
production traversal allocates no trace path.

The corresponding `#[cfg(not(unix))]` arm returns
`CacheError::PayloadSymlinkUnsupported(path.to_path_buf())` without reading the
target. Wrap every filesystem operation used by the walker in a local helper
that appends a `PayloadOperation` under `#[cfg(test)]`; production builds make
the recorder an empty inline operation. No operation helper accepts a link
target as its filesystem path.

- [ ] **Step 5: Run the focused tests and Clippy**

Run:

```console
cargo test --locked cache_payload::tests:: -- --nocapture
cargo clippy --locked --lib --tests -- -D warnings
```

Expected: all four payload traversal tests pass; Clippy reports no warnings.

- [ ] **Step 6: Commit Task 1**

```console
git add src/lib.rs src/cache.rs src/cache_payload.rs
git commit -m "feat: add bounded opaque cache payload traversal"
```

### Task 2: Position-aware inventory accounting

**Files:**

- Modify: `src/cache.rs:708-746`
- Modify: `src/cache.rs:1598-1640`
- Test: `src/cache.rs:2070-2130`

**Interfaces:**

- Consumes: Task 1's `measure_payload_tree` and `PayloadTreeStats`.
- Produces: private `bounded_entry_size(&Path, &mut usize)` and
  `is_payload_root(&Path, &Path)`, while retaining the public
  `CacheInventory` JSON shape.

- [ ] **Step 1: Write the failing inventory tests**

Replace the broad intent of `inventory_never_follows_symlinks` with two exact
boundaries while keeping its existing entry-root rejection assertion:

```rust
#[cfg(unix)]
#[test]
fn inventory_counts_payload_links_without_following_targets() {
    use std::os::unix::{ffi::OsStrExt, fs::symlink};

    let fixture = completed_entry_fixture("inventory-payload-links");
    let before = fixture.cache.inventory().unwrap().entries.remove(0);
    let outside = fixture.repo.join("inventory-sentinel");
    fs::write(&outside, b"sentinel").unwrap();
    symlink(&outside, fixture.data_path.join("external-link")).unwrap();

    let after = fixture.cache.inventory().unwrap().entries.remove(0);
    assert_eq!(after.files, before.files + 1);
    assert_eq!(
        after.bytes,
        before.bytes + outside.as_os_str().as_bytes().len() as u64
    );
    assert_eq!(fs::read(&outside).unwrap(), b"sentinel");
    finish_fixture(fixture);
}

#[cfg(unix)]
#[test]
fn inventory_rejects_a_symlink_at_the_payload_root() {
    use std::os::unix::fs::symlink;

    let fixture = completed_entry_fixture("inventory-payload-root-link");
    let real = fixture.entry_path.join("real-data");
    fs::rename(&fixture.data_path, &real).unwrap();
    symlink(&real, &fixture.data_path).unwrap();
    assert!(matches!(
        fixture.cache.inventory(),
        Err(CacheError::SymlinkInManagedRoot(_))
    ));
    finish_fixture(fixture);
}

#[cfg(unix)]
#[test]
fn inventory_switches_mode_only_at_exact_generation_data_roots() {
    use std::os::unix::fs::symlink;

    let (repo, resolved) = resolved_fixture("inventory-generation-payloads");
    let cache = ManagedCache::initialize(resolved.clone()).unwrap();
    let plan = envelope();
    let key = CacheKey::for_plan_cache(&plan, &plan.plan.caches[0]).unwrap();
    let entry = cache.entry_path(&key);
    let staging = entry.join(".staging-1-1");
    let backup = entry.join(".backup-1-1");
    fs::create_dir_all(staging.join("data")).unwrap();
    fs::create_dir_all(backup.join("data")).unwrap();
    let outside = repo.join("generation-sentinel");
    fs::write(&outside, b"sentinel").unwrap();
    symlink(&outside, staging.join("data/external")).unwrap();
    symlink(&outside, backup.join("data/external")).unwrap();

    let accepted = cache.inventory().unwrap();
    assert_eq!(accepted.entries.len(), 1);
    assert!(accepted.entries[0].files >= 2);
    symlink(&outside, staging.join("control-link")).unwrap();
    assert!(matches!(
        cache.inventory(),
        Err(CacheError::SymlinkInManagedRoot(_))
    ));
    clean(&resolved.path);
    clean(&repo);
}
```

- [ ] **Step 2: Run the tests to verify they fail for the intended reason**

Run:

```console
cargo test --locked cache::tests::inventory_counts_payload_links_without_following_targets -- --exact
cargo test --locked cache::tests::inventory_rejects_a_symlink_at_the_payload_root -- --exact
cargo test --locked cache::tests::inventory_switches_mode_only_at_exact_generation_data_roots -- --exact
```

Expected: the first and third tests fail with `SymlinkInManagedRoot`; the
second passes, confirming the existing payload-root guard.

- [ ] **Step 3: Implement exact payload-root dispatch**

Change `inventory` to call `bounded_entry_size` instead of
`bounded_tree_size`. Preserve strict recursion everywhere except these relative
component shapes:

```rust
fn is_payload_root(entry_root: &Path, candidate: &Path) -> bool {
    let Ok(relative) = candidate.strip_prefix(entry_root) else {
        return false;
    };
    let components: Vec<_> = relative.components().collect();
    match components.as_slice() {
        [Component::Normal(data)] => *data == "data",
        [Component::Normal(generation), Component::Normal(data)] => {
            let generation = generation.to_string_lossy();
            *data == "data"
                && (generation.starts_with(".staging-")
                    || generation.starts_with(".backup-"))
        }
        _ => false,
    }
}
```

In the strict walker, inspect the candidate itself with `symlink_metadata`
first. If it is the exact payload root, require a plain directory and delegate
the complete subtree to `measure_payload_tree`. Otherwise retain the existing
link rejection, regular-file accounting, sorted recursion, node bound, and
checked addition. Do not treat an arbitrary nested directory named `data` as a
payload root.

- [ ] **Step 4: Run focused inventory regression tests**

Run:

```console
cargo test --locked cache::tests::inventory_ -- --nocapture
cargo test --locked cache::tests::completed_source_pin_rejects_symlink_component_and_wrong_type -- --exact
```

Expected: payload-descendant links are counted, payload-root and entry-root
links remain rejected, and cache-pin validation remains strict.

- [ ] **Step 5: Commit Task 2**

```console
git add src/cache.rs
git commit -m "fix: account for opaque links in cache payload inventory"
```

### Task 3: Link-preserving clone and fallback copy

**Files:**

- Modify: `src/cache_payload.rs`
- Modify: `src/cache.rs:1243-1301`
- Test: `src/cache_payload.rs` inline tests
- Test: `src/cache.rs:2160-2234`

**Interfaces:**

- Consumes: Task 1's traversal and cache error variants.
- Produces:
  `copy_payload_tree(&Path, &Path, &mut usize, usize)` and an updated
  `try_clone_tree` that preflights with `validate_payload_tree`.

- [ ] **Step 1: Write the failing fallback-copy test**

Add this Unix test to `src/cache_payload.rs`:

```rust
#[cfg(unix)]
#[test]
fn fallback_copy_preserves_each_link_target_and_external_sentinel() {
    use std::os::unix::fs::symlink;

    let source = payload_fixture("copy-source");
    let destination = source.parent().unwrap().join("copy-destination");
    let outside = source.parent().unwrap().join("copy-sentinel");
    fs::write(source.join("regular"), b"payload").unwrap();
    fs::write(&outside, b"outside").unwrap();
    symlink("regular", source.join("relative")).unwrap();
    symlink("missing", source.join("broken")).unwrap();
    symlink(&outside, source.join("absolute")).unwrap();
    symlink("self", source.join("self")).unwrap();

    let mut nodes = 0;
    copy_payload_tree(&source, &destination, &mut nodes, 100).unwrap();

    for name in ["relative", "broken", "absolute", "self"] {
        assert_eq!(
            fs::read_link(destination.join(name)).unwrap(),
            fs::read_link(source.join(name)).unwrap()
        );
    }
    assert_eq!(fs::read(destination.join("regular")).unwrap(), b"payload");
    assert_eq!(fs::read(&outside).unwrap(), b"outside");
    remove_fixture(&source, &outside);
    fs::remove_dir_all(destination).unwrap();
}
```

Add a platform helper test:

```rust
#[cfg(not(unix))]
#[test]
fn payload_link_recreation_is_explicitly_unsupported() {
    assert!(matches!(
        recreate_payload_link(Path::new("source"), Path::new("destination")),
        Err(CacheError::PayloadSymlinkUnsupported(_))
    ));
}
```

- [ ] **Step 2: Run the copy tests to verify the red state**

Run:

```console
cargo test --locked cache_payload::tests::fallback_copy_preserves_each_link_target_and_external_sentinel -- --exact
```

Expected: compilation fails because `copy_payload_tree` is missing.

- [ ] **Step 3: Implement link-preserving copy**

Use this crate-private signature:

```rust
pub(crate) fn copy_payload_tree(
    source: &Path,
    destination: &Path,
    nodes: &mut usize,
    node_limit: usize,
) -> Result<(), CacheError>;
```

Require the source root to be a plain directory and the destination to be
absent. For each source child, inspect with `symlink_metadata` and then:

```rust
#[cfg(unix)]
fn recreate_payload_link(source: &Path, destination: &Path) -> Result<(), CacheError> {
    use std::os::unix::fs::symlink;
    let target = fs::read_link(source).map_err(|source_error| {
        CacheError::PayloadSymlinkRead {
            path: source.to_path_buf(),
            source: source_error,
        }
    })?;
    match fs::symlink_metadata(destination) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(CacheError::Io(error)),
        Ok(_) => return Err(CacheError::UnexpectedEntry(destination.to_path_buf())),
    }
    symlink(&target, destination).map_err(|source_error| {
        CacheError::PayloadSymlinkCreate {
            path: destination.to_path_buf(),
            source: source_error,
        }
    })
}
```

The non-Unix implementation returns `PayloadSymlinkUnsupported` without
calling `read_link`. Files use `fs::copy`; directories use `fs::create_dir` and
sorted recursion. Apply the same node limit to every visited source object.

- [ ] **Step 4: Integrate clone preflight and fallback**

Import `copy_payload_tree` and `validate_payload_tree` in `src/cache.rs`.
Replace the `bounded_tree_size` call in `try_clone_tree` with:

```rust
let mut nodes = 0;
validate_payload_tree(source, &mut nodes, MAX_INVENTORY_NODES)?;
```

Replace the old recursive `copy_tree` call in `prepare_entry` with:

```rust
let mut nodes = 0;
copy_payload_tree(&source, &data_path, &mut nodes, MAX_INVENTORY_NODES)?;
```

Delete the old symlink-rejecting `copy_tree` function only after no caller
remains. Keep `clonefile` as a macOS-only optimization and preserve its current
fallback error set.

- [ ] **Step 5: Write the failing two-generation reuse test**

Add to `src/cache.rs`:

```rust
#[cfg(unix)]
#[test]
fn complete_payload_symlinks_are_preserved_across_generation_reuse() {
    use std::os::unix::fs::symlink;

    let (repo, resolved) = resolved_fixture("payload-link-reuse");
    let cache = ManagedCache::initialize(resolved.clone()).unwrap();
    let plan = envelope();
    let key = CacheKey::for_plan_cache(&plan, &plan.plan.caches[0]).unwrap();
    let outside = repo.join("reuse-sentinel");
    fs::write(&outside, b"unchanged").unwrap();

    let first = cache.prepare_entry(&key, &plan.plan_digest, 1).unwrap();
    fs::write(first.data_path.join("regular"), b"value").unwrap();
    symlink("regular", first.data_path.join("relative")).unwrap();
    symlink("missing", first.data_path.join("broken")).unwrap();
    symlink(&outside, first.data_path.join("external")).unwrap();
    cache.promote_entry(&first).unwrap();
    drop(first);

    let second = cache.prepare_entry(&key, &plan.plan_digest, 2).unwrap();
    for name in ["relative", "broken", "external"] {
        assert_eq!(
            fs::read_link(second.data_path.join(name)).unwrap(),
            fs::read_link(cache.entry_data_path(&key).join(name)).unwrap()
        );
    }
    assert_eq!(fs::read(&outside).unwrap(), b"unchanged");
    drop(second);
    clean(&resolved.path);
    clean(&repo);
}
```

- [ ] **Step 6: Run copy, reuse, and link-free regressions**

Run:

```console
cargo test --locked cache_payload::tests:: -- --nocapture
cargo test --locked cache::tests::complete_payload_symlinks_are_preserved_across_generation_reuse -- --exact
cargo test --locked cache::tests::failed_generation_does_not_mutate_last_known_good -- --exact
```

Expected: all tests pass and the external sentinels remain unchanged.

- [ ] **Step 7: Commit Task 3**

```console
git add src/cache.rs src/cache_payload.rs
git commit -m "fix: preserve symbolic links when reusing cache payloads"
```

### Task 4: Preparation ownership before fallible reuse

**Files:**

- Modify: `src/cache.rs:399-463`
- Modify: `src/cache.rs:925-968`
- Test: `src/cache.rs:2194-2288`

**Interfaces:**

- Consumes: Task 3's fallible clone/copy path.
- Produces: private `PREPARED_PHASE_PREPARING`,
  `PREPARED_PHASE_STAGING`, and `PREPARED_PHASE_PROMOTED` states stored in an
  `AtomicU8`, plus `remove_owned_generation_directory`.

- [ ] **Step 1: Write the failing preparation-leak regression**

Add this Unix test to `src/cache.rs`:

```rust
#[cfg(unix)]
#[test]
fn failed_payload_preflight_removes_the_new_staging_generation() {
    use std::os::unix::net::UnixListener;

    let fixture = completed_entry_fixture("failed-payload-preflight-cleanup");
    let plan = envelope();
    let key = CacheKey::for_plan_cache(&plan, &plan.plan.caches[0]).unwrap();
    let socket_path = fixture.data_path.join("unsupported.socket");
    let listener = UnixListener::bind(&socket_path).unwrap();

    assert!(matches!(
        fixture.cache.prepare_entry(&key, &plan.plan_digest, 2),
        Err(CacheError::UnexpectedEntry(_))
    ));
    let staging: Vec<_> = fs::read_dir(&fixture.entry_path)
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".staging-"))
        .collect();
    assert!(staging.is_empty(), "failed preparation leaked {staging:?}");

    drop(listener);
    fs::remove_file(socket_path).unwrap();
    finish_fixture(fixture);
}
```

- [ ] **Step 2: Run the regression to prove the observed leak**

Run:

```console
cargo test --locked cache::tests::failed_payload_preflight_removes_the_new_staging_generation -- --exact
```

Expected: FAIL because one `.staging-*` directory remains after
`prepare_entry` returns the preflight error.

- [ ] **Step 3: Add phase-aware generation ownership**

Introduce exact internal states without changing a persistent schema:

```rust
const PREPARED_PHASE_PREPARING: u8 = 0;
const PREPARED_PHASE_STAGING: u8 = 1;
const PREPARED_PHASE_PROMOTED: u8 = 2;

struct PreparedCacheGenerationOwner {
    entry_path: PathBuf,
    staging_path: PathBuf,
    key_digest: String,
    plan_digest: String,
    generation: u64,
    phase: AtomicU8,
    _entry_lock: Arc<File>,
}
```

Construct its `Arc` immediately after creating the plain staging and plain
`data` directories, before `remove_if_present`, `try_clone_tree`, or
`copy_payload_tree`. After `write_generation_manifest` succeeds, execute:

```rust
owner.phase.store(PREPARED_PHASE_STAGING, Ordering::Release);
```

After one entry's promotion marker is durably written, execute:

```rust
prepared
    ._generation_owner
    .phase
    .store(PREPARED_PHASE_PROMOTED, Ordering::Release);
```

The `Drop` implementation loads with `Ordering::Acquire`. In preparing phase it
requires exact parent equality, a valid `.staging-` name, and a plain staging
root. In staging phase it additionally requires the current matching manifest.
In promoted phase it returns without deletion.

- [ ] **Step 4: Add the no-follow whole-generation cleanup test**

Add a test that prepares a fresh generation, creates nested payload links to an
external directory containing two sentinel files, then drops the preparation
without promotion:

```rust
#[cfg(unix)]
#[test]
fn staging_cleanup_unlinks_payload_links_without_touching_targets() {
    use std::os::unix::fs::symlink;

    let (repo, resolved) = resolved_fixture("staging-cleanup-links");
    let cache = ManagedCache::initialize(resolved.clone()).unwrap();
    let plan = envelope();
    let key = CacheKey::for_plan_cache(&plan, &plan.plan.caches[0]).unwrap();
    let outside = repo.join("cleanup-sentinel");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("first"), b"first").unwrap();
    fs::write(outside.join("second"), b"second").unwrap();

    let prepared = cache.prepare_entry(&key, &plan.plan_digest, 1).unwrap();
    fs::create_dir(prepared.data_path.join("nested")).unwrap();
    symlink(&outside, prepared.data_path.join("external")).unwrap();
    symlink(&outside, prepared.data_path.join("nested/external")).unwrap();
    let staging = prepared.staging_path.clone();
    drop(prepared);

    assert!(!staging.exists());
    assert_eq!(fs::read(outside.join("first")).unwrap(), b"first");
    assert_eq!(fs::read(outside.join("second")).unwrap(), b"second");
    clean(&resolved.path);
    clean(&repo);
}
```

Keep whole-generation removal behind
`remove_owned_generation_directory(&Path)`: validate the leaf with
`symlink_metadata`, reject a link or non-directory, then use
`fs::remove_dir_all`. The test above is the required supported-Unix no-follow
qualification for the standard-library primitive.

- [ ] **Step 5: Run ownership, lock, and cleanup regressions**

Run:

```console
cargo test --locked cache::tests::failed_payload_preflight_removes_the_new_staging_generation -- --exact
cargo test --locked cache::tests::staging_cleanup_unlinks_payload_links_without_touching_targets -- --exact
cargo test --locked cache::tests::prepared_entry_clones_share_cleanup_and_lock_until_final_drop -- --exact
cargo test --locked cache::tests::active_entry_lock_blocks_a_second_preparation_until_release -- --exact
```

Expected: all four tests pass; final-drop cleanup precedes lock release.

- [ ] **Step 6: Commit Task 4**

```console
git add src/cache.rs
git commit -m "fix: own cache staging before fallible preparation"
```

### Task 5: Promotion and recovery payload validation

**Files:**

- Modify: `src/cache.rs:472-670`
- Modify: `src/cache.rs:671-708`
- Test: `src/cache.rs:2289-2408`

**Interfaces:**

- Consumes: Task 1's `validate_payload_tree` and Task 4's phase-aware owner.
- Produces: promotion-time bounded payload validation and no-follow recovery
  regressions with no journal schema change.

- [ ] **Step 1: Write the failing unsupported-object promotion test**

Add this Unix test:

```rust
#[cfg(unix)]
#[test]
fn promotion_rejects_an_unsupported_payload_object_before_journaling() {
    use std::os::unix::net::UnixListener;

    let (repo, resolved) = resolved_fixture("promotion-payload-object");
    let cache = ManagedCache::initialize(resolved.clone()).unwrap();
    let plan = envelope();
    let key = CacheKey::for_plan_cache(&plan, &plan.plan.caches[0]).unwrap();
    let prepared = cache.prepare_entry(&key, &plan.plan_digest, 1).unwrap();
    let listener = UnixListener::bind(prepared.data_path.join("socket")).unwrap();

    assert!(matches!(
        cache.promote_entry(&prepared),
        Err(CacheError::UnexpectedEntry(_))
    ));
    assert!(!resolved.path.join(PROMOTION_JOURNAL_FILE).exists());
    assert!(!cache.entry_path(&key).join(COMPLETE_FILE).exists());

    drop(listener);
    drop(prepared);
    clean(&resolved.path);
    clean(&repo);
}
```

- [ ] **Step 2: Run it to verify the current promotion accepts the object**

Run:

```console
cargo test --locked cache::tests::promotion_rejects_an_unsupported_payload_object_before_journaling -- --exact
```

Expected: FAIL because `promote_entry` currently reaches promotion instead of
returning `UnexpectedEntry` before journal creation.

- [ ] **Step 3: Validate payloads before journal creation and legacy completion**

At the end of `validate_prepared_entry`, after manifest identity checks, add:

```rust
let mut nodes = 0;
validate_payload_tree(
    &prepared.data_path,
    &mut nodes,
    MAX_INVENTORY_NODES,
)?;
```

In `mark_entry_complete`, perform the same validation on `entry.join("data")`
before inspecting or creating `.complete-v1`. Do not add payload traversal to
`current_generation_is_complete` or `current_matches_previous`; recovery must
reason about exact directory/marker/manifest identity and rename whole owned
generation directories without following descendants.

- [ ] **Step 4: Add link-bearing promotion and recovery tests**

Extend `multi_entry_promotion_is_journaled_and_cleans_only_after_success` with
a relative payload link in one entry and assert it remains a link after
promotion. Add this separate Unix recovery assertion to
`interrupted_prepared_journal_is_recovered_without_adopting_data`:

```rust
let outside = repo.join("recovery-sentinel");
fs::write(&outside, b"recovery").unwrap();
symlink(&outside, prepared.data_path.join("external")).unwrap();
// create the prepared journal, drop the prepared owner, and recover
assert_eq!(fs::read(&outside).unwrap(), b"recovery");
```

The committed test must retain the existing assertions that the journal is
removed and unpromoted data is not adopted.

- [ ] **Step 5: Run promotion and recovery tests**

Run:

```console
cargo test --locked cache::tests::promotion_rejects_an_unsupported_payload_object_before_journaling -- --exact
cargo test --locked cache::tests::multi_entry_promotion_is_journaled_and_cleans_only_after_success -- --exact
cargo test --locked cache::tests::interrupted_prepared_journal_is_recovered_without_adopting_data -- --exact
cargo test --locked cache::tests::complete_payload_symlinks_are_preserved_across_generation_reuse -- --exact
```

Expected: all four pass; links remain links, unsupported objects never create a
journal, and recovery leaves the external sentinel unchanged.

- [ ] **Step 6: Commit Task 5**

```console
git add src/cache.rs
git commit -m "fix: validate opaque payloads before cache promotion"
```

### Task 6: Public documentation and deterministic contract

**Files:**

- Modify: `docs/CACHE_AND_WORKSPACE.md:80-120`
- Modify: `docs/CACHE_AND_WORKSPACE.md:155-214`
- Modify: `docs/THREAT_MODEL.md:60-115`
- Modify: `docs/TESTING_AND_FAULT_INJECTION.md:80-150`
- Modify: `tests/repository_hygiene_contract.rs:14-130`
- Modify: `CHANGELOG.md:8-30`

**Interfaces:**

- Consumes: exact test names and platform behavior from Tasks 1-5.
- Produces: public no-follow/cache-reuse documentation and one deterministic
  contract test that prevents future overclaim or boundary collapse.

- [ ] **Step 1: Write the failing documentation contract**

Add constants and this test to `tests/repository_hygiene_contract.rs`:

```rust
const CACHE_AND_WORKSPACE: &str = include_str!("../docs/CACHE_AND_WORKSPACE.md");
const TESTING_AND_FAULT_INJECTION: &str =
    include_str!("../docs/TESTING_AND_FAULT_INJECTION.md");

#[test]
fn cache_payload_symlinks_are_documented_as_opaque_unattested_state() {
    for phrase in [
        "control plane",
        "payload plane",
        "never follows a payload link target on the host",
        "relative, absolute, broken, recursive, and outside-root",
        "Windows link-bearing payload reuse remains unsupported",
        "one node and one non-directory object",
    ] {
        assert!(CACHE_AND_WORKSPACE.contains(phrase), "missing {phrase}");
    }
    for reference in [
        "src/cache.rs::complete_payload_symlinks_are_preserved_across_generation_reuse",
        "src/cache.rs::failed_payload_preflight_removes_the_new_staging_generation",
        "src/cache.rs::staging_cleanup_unlinks_payload_links_without_touching_targets",
    ] {
        assert!(
            TESTING_AND_FAULT_INJECTION.contains(reference),
            "missing {reference}"
        );
    }
}
```

- [ ] **Step 2: Run the contract to verify it fails**

Run:

```console
cargo test --locked --test repository_hygiene_contract cache_payload_symlinks_are_documented_as_opaque_unattested_state -- --exact
```

Expected: FAIL on the first missing documentation phrase.

- [ ] **Step 3: Update the cache and threat-model contracts**

In `docs/CACHE_AND_WORKSPACE.md`, add one section titled
`## Control plane and opaque payload links` containing all six phrases asserted
above. State that inventory counts a link's stored target length as bytes,
never target content, and retains the 100,000-node bound. State that
control-plane and payload-root links still fail closed and that cache payloads
remain mutable, unattested performance state.

In `docs/THREAT_MODEL.md`, update T15 and T21 to distinguish opaque payload
links from link-free control paths. The residual-risk cell must state that a
containerized project process can resolve payload links in its mount namespace
and that CCP does not claim hostile-code sandboxing or cache-content trust.

- [ ] **Step 4: Update testing documentation and changelog**

Add a `## Opaque cache-payload symbolic links` section to
`docs/TESTING_AND_FAULT_INJECTION.md`. Include the three exact `src/cache.rs`
references asserted by the contract plus
`src/cache_payload.rs::payload_measurement_counts_links_without_following_targets`
and
`src/cache_payload.rs::fallback_copy_preserves_each_link_target_and_external_sentinel`.
Describe macOS clone behavior, Unix fallback copy, Windows fail-closed status,
and the fact that deterministic tests are not a native CCP receipt.

Under `CHANGELOG.md` → `[Unreleased]` → `Added`, add one bullet stating that
Unix cache generations now preserve ordinary opaque payload links during
inventory and reuse, strict control paths remain link-free, failed preparation
owns cleanup before reuse, and native candidate qualification remains pending.

- [ ] **Step 5: Run documentation and referenced-test contracts**

Run:

```console
cargo test --locked --test repository_hygiene_contract cache_payload_symlinks_are_documented_as_opaque_unattested_state -- --exact
cargo test --locked cache::tests::complete_payload_symlinks_are_preserved_across_generation_reuse -- --exact
cargo test --locked cache::tests::failed_payload_preflight_removes_the_new_staging_generation -- --exact
cargo test --locked cache::tests::staging_cleanup_unlinks_payload_links_without_touching_targets -- --exact
git diff --check
```

Expected: every test passes and `git diff --check` emits no output.

- [ ] **Step 6: Commit Task 6**

```console
git add CHANGELOG.md docs/CACHE_AND_WORKSPACE.md docs/THREAT_MODEL.md docs/TESTING_AND_FAULT_INJECTION.md tests/repository_hygiene_contract.rs
git commit -m "docs: define opaque cache payload link boundary"
```

### Task 7: Full deterministic gates and review checkpoint

**Files:**

- Review: `src/cache_payload.rs`
- Review: `src/cache.rs`
- Review: `src/lib.rs`
- Review: `tests/repository_hygiene_contract.rs`
- Review: `docs/CACHE_AND_WORKSPACE.md`
- Review: `docs/THREAT_MODEL.md`
- Review: `docs/TESTING_AND_FAULT_INJECTION.md`
- Review: `CHANGELOG.md`

**Interfaces:**

- Consumes: the complete implementation from Tasks 1-6.
- Produces: an exact-HEAD deterministic qualification checkpoint ready for
  independent review and a separately authorized native candidate run.

- [ ] **Step 1: Recheck scope and repository state**

Run:

```console
git status --short --branch
git log --oneline --decorate -8
git diff --check 7adc340ca39c18a5ea31e7a1df054f97629876f9..HEAD
git diff --name-only 7adc340ca39c18a5ea31e7a1df054f97629876f9..HEAD
```

Expected: only the files listed in this plan changed; no generated cache,
receipt, target artifact, local path, or unrelated file is tracked.

- [ ] **Step 2: Run formatting and warnings-denied compilation**

Run:

```console
cargo fmt --all -- --check
cargo check --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
```

Expected: all commands exit `0` with no warning.

- [ ] **Step 3: Run the complete deterministic suite**

Run:

```console
cargo test --locked --all-targets --all-features
```

Expected: exit `0`; ignored native tests remain explicitly ignored and are not
reported as PASS evidence.

- [ ] **Step 4: Run release-boundary and metadata checks**

Run:

```console
cargo test --locked --quiet --test release_hardening_contract
cargo run --locked --quiet --example generate_release_metadata -- --check
```

Expected: both commands exit `0` and checked-in release metadata is unchanged.

- [ ] **Step 5: Perform the spec-coverage review**

Read the approved spec from top to bottom and record a local checklist mapping:

```text
control-plane rejection -> cache boundary tests
opaque no-follow traversal -> cache_payload trace and sentinel tests
inventory accounting -> inventory delta test
Unix copy and macOS clone -> fallback and two-generation tests
Windows fail-closed -> non-Unix recreation test and public docs
early cleanup ownership -> leak, lock, clone-owner, and sentinel tests
promotion/recovery -> unsupported-object and journal recovery tests
schema compatibility -> unchanged fixtures and full deterministic suite
```

Any missing mapping blocks completion and is repaired through a new red-green
cycle before continuing.

- [ ] **Step 6: Request an independent code and security review**

Invoke `superpowers:requesting-code-review` against the exact diff from
`7adc340ca39c18a5ea31e7a1df054f97629876f9` to `HEAD`. The reviewer must check:

```text
1. no host operation accepts a payload link target as a filesystem path;
2. payload mode is reachable only below an exact validated data root;
3. node and byte accounting is checked and deterministic;
4. fallback copy preserves links and never materializes their targets;
5. preparation cleanup owns the exact staging path before every failure;
6. entry lock lifetime covers cleanup;
7. promotion/recovery do not weaken marker, manifest, journal, or pin checks;
8. Windows claims remain fail-closed and evidence-bounded;
9. no schema, dependency, privacy, or authority expansion is hidden.
```

Resolve every confirmed finding with a focused test and separate commit. Do not
accept a style-only rewrite that broadens the reviewed filesystem surface.

- [ ] **Step 7: Record the final local checkpoint**

Run:

```console
git status --short --branch
git rev-parse HEAD
cargo build --locked --bin commit-ci-preflight
shasum -a 256 target/debug/commit-ci-preflight
git log -1 --format='%H%n%s'
```

The explicit build is mandatory even when `target/debug/commit-ci-preflight`
already exists, so the recorded hash cannot name a stale binary from an older
HEAD. Record the exact HEAD, binary path, binary SHA-256, platform, Rust
version, gate commands, and results in the private operator handoff. Do not
commit absolute paths or host identity.

- [ ] **Step 8: Stop at the native qualification boundary**

Do not install the candidate or run `plan`, `doctor`, `dry-run`, `run`,
`benchmark`, or `guard exec`. A future authorization must bind the exact
candidate commit and binary hash, isolated installation prefix, rollback
binary, repository/worktree, reviewed configuration digest, generation,
maximum run count, expected receipt, post-run checks, and stop boundary.

The native acceptance sequence will require two separately reviewed cache
generations: the first creates and promotes link-bearing package-manager and
environment payloads; the second reuses the same completed entries. That
sequence is not authorized by this implementation plan.
