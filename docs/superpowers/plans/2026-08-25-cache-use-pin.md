# Cache-use Pin Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep declared CCP cache bind sources owned through execution and fail closed immediately before Docker creation when any structured mount source is unsafe or no longer matches its prepared generation.

**Architecture:** Reuse the existing per-entry advisory lock as the sole cache-use authority. Make cloned prepared generations share one final-drop owner, carry nonserialized mount expectations into runtime execution, and add an opt-in `guard exec` pin for exact completed entries without parsing arbitrary child argv.

**Tech Stack:** Rust 2024, standard library filesystem and synchronization primitives, `fs2` advisory locks already in the crate, Clap, Serde, existing fake supervisor and fixture infrastructure.

**Spec:** `docs/superpowers/specs/2026-08-25-cache-use-pin-design.md`

## Global Constraints

- Baseline is `3fccc197e5055a2759ee7afe51b91133938ec904`; implementation branch already contains only spec commit `90b4ec36278bcd1e41cefa9ad7775e8da61a4709`.
- Every shell command begins with `rtk`.
- Use strict red-green TDD for every production change; record the expected failing assertion before implementation.
- The existing per-entry advisory lock is the only cache-use authority; add no TTL, heartbeat, daemon, persistent lease, or dependency.
- Existing valid configuration digests, receipts, policies, verification behavior, dry-run JSON, and fixtures remain serialized-compatible.
- New mount expectations are internal and `#[serde(skip)]`; absolute paths never enter structured JSON, journals, receipts, resource history, admission state, telemetry, or remote evidence.
- `guard exec` remains cooperative, shell-free, receipt-free, and does not parse or attest arbitrary child argv.
- Automatic cache deletion/quarantine remains unavailable. Never mutate the persistent host cache during tests.
- No Docker, OrbStack, network, model workload, CCP `run`, receipt publication, push, PR, or merge belongs to this plan.

---

### Task 1: Share prepared-generation cleanup and lock lifetime

**Files:**
- Modify: `src/cache.rs:357-414,738-764,1986-2013`

**Interfaces:**
- Produces: `PreparedCacheEntry::generation_expectation(&self) -> CacheGenerationExpectation` for Task 2.
- Produces: one shared final-drop owner that retains the existing entry lock until validated staging cleanup finishes.
- Preserves: `PreparedCacheEntry: Clone`, public `path`, `data_path`, `was_complete`, promotion behavior, and current manifest format.

- [ ] **Step 1: Write the clone-lifetime regression test**

Add beside `active_entry_lock_blocks_a_second_preparation_until_release`:

```rust
#[test]
fn prepared_entry_clones_share_cleanup_and_lock_until_final_drop() {
    let (repo, resolved) = resolved_fixture("entry-clone-lifetime");
    let cache = ManagedCache::initialize(resolved.clone()).expect("initialize");
    let envelope = envelope();
    let key = CacheKey::for_plan_cache(&envelope, &envelope.plan.caches[0]).expect("key");
    let first = cache
        .prepare_entry(&key, &envelope.plan_digest, 7)
        .expect("prepare");
    let staging = first.staging_path.clone();
    let clone = first.clone();

    drop(first);
    assert!(staging.is_dir(), "one clone must not remove live staging");
    assert!(matches!(
        cache.prepare_entry(&key, &envelope.plan_digest, 8),
        Err(CacheError::LockBusy(_))
    ));

    drop(clone);
    assert!(!staging.exists(), "final owner removes matching staging");
    let next = cache
        .prepare_entry(&key, &envelope.plan_digest, 8)
        .expect("lock released after final owner");
    drop(next);
    clean(&resolved.path);
    clean(&repo);
}
```

- [ ] **Step 2: Run the focused test and verify RED**

Run:

```console
rtk cargo test --lib prepared_entry_clones_share_cleanup_and_lock_until_final_drop
```

Expected: FAIL at `staging.is_dir()` because dropping the first clone currently removes the shared staging directory.

- [ ] **Step 3: Introduce shared final-drop state**

Add a crate-visible immutable expectation and a private final-drop owner:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CacheGenerationExpectation {
    pub key_digest: String,
    pub plan_digest: String,
    pub generation: u64,
    pub state: &'static str,
}

#[derive(Debug)]
struct PreparedCacheGenerationOwner {
    staging_path: PathBuf,
    key_digest: String,
    plan_digest: String,
    generation: u64,
    entry_lock: Arc<File>,
}
```

Move the existing manifest-checked cleanup from `Drop for PreparedCacheEntry`
to `Drop for PreparedCacheGenerationOwner`. Keep `entry_lock` in that struct so
Rust drops it only after the cleanup method returns. Add
`owner: Arc<PreparedCacheGenerationOwner>` to `PreparedCacheEntry`, retain the
existing immutable fields used by promotion, remove `_entry_lock` from the
outer struct, and construct exactly one owner in `prepare_entry`.

Implement:

```rust
pub(crate) fn generation_expectation(&self) -> CacheGenerationExpectation {
    CacheGenerationExpectation {
        key_digest: self.key_digest.clone(),
        plan_digest: self.plan_digest.clone(),
        generation: self.generation,
        state: "staging",
    }
}
```

- [ ] **Step 4: Verify GREEN and cache regressions**

Run:

```console
rtk cargo test --lib prepared_entry_clones_share_cleanup_and_lock_until_final_drop
rtk cargo test --lib cache::tests
```

Expected: the new test passes; all cache unit tests pass.

- [ ] **Step 5: Commit Task 1**

```console
rtk git add src/cache.rs
rtk git commit -m "fix: share prepared cache generation lifetime"
```

### Task 2: Revalidate structured mounts at the Docker spawn boundary

**Files:**
- Modify: `src/cache.rs:738-780,912-980`
- Modify: `src/workspace.rs:35-149,152-248,745-770,787-850`
- Modify: `src/runtime.rs:185-277,690-775,847-871,1633-1726`

**Interfaces:**
- Consumes: `PreparedCacheEntry::generation_expectation()` from Task 1.
- Produces: `cache::revalidate_generation_source(&Path, &CacheGenerationExpectation) -> Result<(), CacheError>`.
- Produces: side-effect-free, read-only
  `workspace::revalidate_mount_sources(&[MountBinding]) -> Result<(), WorkspaceError>`.
- Produces: private `DryRunCheck.mounts: Vec<MountBinding>` with `#[serde(skip)]`.
- Preserves: serialized `DryRunPlan`, `DryRunCheck`, normalized plan digest, Docker argv, and `RuntimePort::execute_check` signature.

- [ ] **Step 1: Write read-only workspace validation tests**

Add `prepared_fixture(name)` beside the existing `fixture(name)`. It initializes
`ManagedCache` and calls `PreparedWorkspace::prepare_with_generation(..., 7)`,
returning the owned base, cache, and prepared workspace. Assert the validator
accepts untouched mounts. Before calling the validator, explicitly assert that
the prepared cache mount has
`expectation.as_ref().and_then(|value| value.cache_generation.as_ref()).is_some()`.
Keep this test inside `workspace.rs`'s crate-local test module so it exercises
the intended crate-visible fields and validator rather than failing on fixture
privacy. The helper must return the exact `PreparedWorkspace.plan.mounts` that
`prepare_with_generation` mutates, including the snapshot-preparation path.
For separate tests, mutate only one source after preparation and assert the
named typed error:

```rust
#[test]
fn live_mount_revalidation_rejects_missing_cache_generation() {
    let (base, _cache, prepared) = prepared_fixture("revalidate-missing-cache");
    let cache_mount = prepared
        .plan
        .mounts
        .iter()
        .find(|mount| mount.purpose == MountPurpose::Cache)
        .expect("cache mount")
        .source
        .clone();
    fs::remove_dir_all(cache_mount).expect("remove owned staged cache source");

    assert!(matches!(
        revalidate_mount_sources(&prepared.plan.mounts),
        Err(WorkspaceError::MountSourceChanged { .. })
    ));
    fs::remove_dir_all(base).expect("remove owned fixture");
}
```

Cover these cases independently:

- valid repository, cache, regular-file artifact, and directory artifact;
- missing source;
- wrong file/directory type;
- leaf symlink;
- symlink in a parent component beneath the anchor;
- cache manifest key, plan digest, generation, or state mismatch.

Every fixture removes only its own test root.

- [ ] **Step 2: Run the workspace tests and verify RED**

Run:

```console
rtk cargo test --lib live_mount_revalidation
```

Expected: compile failure because `revalidate_mount_sources`, the expectation
fields, and `WorkspaceError::MountSourceChanged` do not yet exist.

- [ ] **Step 3: Add nonserialized mount expectations**

Add internal types:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MountSourceKind {
    Directory,
    RegularFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MountSourceExpectation {
    pub kind: MountSourceKind,
    pub canonical_anchor: PathBuf,
    pub exact_repository: Option<PathBuf>,
    pub cache_generation: Option<CacheGenerationExpectation>,
}
```

Add to `MountBinding`:

```rust
#[serde(skip)]
pub(crate) expectation: Option<MountSourceExpectation>,
```

All `WorkspacePlanV1` constructors set repository and artifact expectations
from the canonical repository/run root and `artifact_kind_for`. Pure dry-run
cache bindings carry kind/anchor but no generation. After each live
`prepare_entry`, `PreparedWorkspace::prepare_with_generation` attaches the
exact generation expectation to its matching cache mount. Snapshot preparation
uses the same path.

Add errors that do not serialize paths:

```rust
MountExpectationMissing,
MountSourceChanged { purpose: MountPurpose },
```

Their `Display` messages name only the bounded mount purpose, never the absolute
source.

- [ ] **Step 4: Implement the side-effect-free read-only validator**

`revalidate_mount_sources` must, for every mount:

1. require an expectation;
2. walk each component from the canonical anchor with `symlink_metadata` and
   reject any symlink;
3. require the expected leaf type;
4. canonicalize and require exact repository equality or cache/artifact
   containment;
5. rerun `validate_host_path`;
6. for cache mounts, require a generation expectation and call
   `cache::revalidate_generation_source`, which compares the private manifest's
   schema, key digest, plan digest, generation, and state.

Keep manifest constants and parsing in `cache.rs`; do not duplicate its on-disk
schema in `workspace.rs`. Do not create, repair, rename, or delete anything in
either validator.

- [ ] **Step 5: Verify read-only validation GREEN**

Run:

```console
rtk cargo test --lib live_mount_revalidation
rtk cargo test --lib workspace::tests
```

Expected: all new validation cases and existing workspace tests pass.

- [ ] **Step 6: Write the zero-supervisor-call runtime test**

Extend the existing recording/fake supervisor. Render a check from a live
workspace, remove its owned cache source, then call `execute_check`:

```rust
let result = runtime.execute_check(&check, &rendered, &context);
assert!(matches!(
    result,
    Err(RuntimeError::Workspace(WorkspaceError::MountSourceChanged { .. }))
));
assert!(supervisor.calls.lock().expect("calls").is_empty());
```

Add a serialization regression asserting the JSON value for the same dry-run
fixture is unchanged after private expectations are populated.

- [ ] **Step 7: Run the runtime tests and verify RED**

Run:

```console
rtk cargo test --lib execute_check_revalidates_mounts_before_supervisor
rtk cargo test --lib dry_run_private_mount_expectations_are_not_serialized
```

Expected: the first test fails because `execute_check` does not call the
validator; the serialization test may not compile until the private mounts are
added to `DryRunCheck`.

- [ ] **Step 8: Integrate the authoritative runtime gate**

Add to `DryRunCheck`:

```rust
#[serde(skip)]
mounts: Vec<crate::workspace::MountBinding>,
```

Populate it from `workspace.mounts.clone()` in every `DryRunCheck` constructor,
including test helpers; do not allow a constructor to silently default to an
empty mount vector. Before editing, inventory `DryRunCheck {` with `rtk rg`:
the baseline has one production literal in `docker_dry_run_check` and no
`Deserialize` or `Default` path. Record any newly discovered literal and make
the required field compile-time mandatory; all tests must obtain checks through
that constructor or set the real mount vector explicitly. At the
first line of `DockerCompatibleRuntime::execute_check`, before
`DockerLifecyclePlan::build`, call:

```rust
crate::workspace::revalidate_mount_sources(&rendered.mounts)
    .map_err(RuntimeError::Workspace)?;
```

No other Docker lifecycle entry point may call the supervisor with a rendered
check.

- [ ] **Step 9: Verify Task 2 GREEN and compatibility**

Run:

```console
rtk cargo test --lib execute_check_revalidates_mounts_before_supervisor
rtk cargo test --lib dry_run_private_mount_expectations_are_not_serialized
rtk cargo test --lib workspace::tests
rtk cargo test --lib runtime::tests
rtk cargo test --test runtime_cli
rtk cargo test --test cache_cli
```

Expected: all commands pass and dry-run JSON assertions remain unchanged.

- [ ] **Step 10: Commit Task 2**

```console
rtk git add src/cache.rs src/workspace.rs src/runtime.rs
rtk git commit -m "fix: revalidate mounts before Docker creation"
```

### Task 3: Add opt-in managed-cache pins to `guard exec`

**Files:**
- Modify: `src/cache.rs:322-356,663-764,1190-1235,1316-1350`
- Modify: `src/main.rs:250-330,574-660,2260-2310,2436-2800`
- Modify: `tests/guard_exec_cli.rs:1-100` only for CLI surface regressions that do not require native admission.

**Interfaces:**
- Produces: `ManagedCache::pin_completed_sources(&[PathBuf]) -> Result<Vec<CacheUsePin>, CacheError>`.
- Produces: `CacheUsePin::revalidate(&self) -> Result<(), CacheError>`.
- Produces CLI flags `--managed-cache-root <absolute-root>` and repeatable `--managed-cache-source <exact-data-path>`.
- Consumes: existing `ResolvedCacheRoot::resolve`, `ManagedCache::open`, entry lock, ownership marker, completion marker, and generation manifest.
- Preserves: legacy unpinned `guard exec` parsing and execution.

- [ ] **Step 1: Write cache pin parser and lock tests**

Add cache tests that construct only CCP-owned fixture roots:

```rust
#[test]
fn completed_source_pin_holds_entry_lock_until_drop() {
    let fixture = completed_entry_fixture("completed-source-pin");
    let pins = fixture
        .cache
        .pin_completed_sources(std::slice::from_ref(&fixture.data_path))
        .expect("pin completed source");
    assert_eq!(pins.len(), 1);
    assert!(matches!(
        fixture.cache.pin_completed_sources(std::slice::from_ref(&fixture.data_path)),
        Err(CacheError::LockBusy(_))
    ));
    drop(pins);
    assert_eq!(
        fixture
            .cache
            .pin_completed_sources(std::slice::from_ref(&fixture.data_path))
            .expect("re-pin after release")
            .len(),
        1
    );
}
```

Separate tests cover duplicate source deduplication, deterministic ordering,
wrong root, extra components, invalid key directory, staging source,
incomplete entry, missing source, symlink component, wrong type, missing or
mismatched completion manifest, and an active prepared generation returning
`LockBusy`.

- [ ] **Step 2: Run cache pin tests and verify RED**

Run:

```console
rtk cargo test --lib completed_source_pin
```

Expected: compile failure because `CacheUsePin` and
`pin_completed_sources` do not exist.

- [ ] **Step 3: Implement one strict source parser and RAII pin**

Add:

```rust
#[derive(Debug)]
pub struct CacheUsePin {
    entry_path: PathBuf,
    data_path: PathBuf,
    key_digest: String,
    plan_digest: String,
    generation: u64,
    _entry_lock: Arc<File>,
}
```

`pin_completed_sources` must:

1. validate the existing owner marker and fixed root layout;
2. reject non-absolute/noncanonical paths and symlink components;
3. require exactly `entries/sha256-<64 lowercase hex>/data` beneath this
   cache's exact canonical root;
4. canonicalize, deduplicate, and sort by entry path;
5. acquire each existing entry lock with current non-blocking semantics;
6. after acquisition, require plain entry/data directories, exact complete
   marker, and a complete generation manifest whose key digest matches the
   directory and whose plan digest is valid;
7. release every already-acquired pin automatically if any later source fails.

`CacheUsePin::revalidate` repeats step 6 and exact path/root containment without
creating or repairing state. Human-readable `Display` may name a path, but the
CLI must not serialize it.

- [ ] **Step 4: Verify cache pin GREEN**

Run:

```console
rtk cargo test --lib completed_source_pin
rtk cargo test --lib cache::tests
```

Expected: all pin, existing lock, promotion, inventory, and recovery tests pass.

- [ ] **Step 5: Write guard CLI parsing tests**

Extend `guard_exec_requires_double_dash_and_program` and add focused cases:

```rust
let cli = Cli::try_parse_from([
    "commit-ci-preflight", "guard", "exec",
    "--managed-cache-root", "/owned/cache",
    "--managed-cache-source", "/owned/cache/entries/sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/data",
    "--", "fixture",
]).expect("managed cache pin parses");
```

Assert `Cli::try_parse_from` stores exactly one raw root and one source. Then
invoke a pure `validate_guard_cache_args` helper and assert it rejects root-only,
source-only, and repeated root declarations with usage exit 2. Legacy args must
parse with empty root and source vectors and the helper must accept them as the
unpinned case.

- [ ] **Step 6: Run parser tests and verify RED**

Run:

```console
rtk cargo test --bin commit-ci-preflight guard_exec_parses_managed_cache_pins
```

Expected: FAIL because `GuardExecArgs` has no managed-cache fields and
`validate_guard_cache_args` does not exist.

- [ ] **Step 7: Add flags and typed guard errors**

Add fields:

```rust
#[arg(long)]
managed_cache_root: Vec<PathBuf>,
#[arg(long)]
managed_cache_source: Vec<PathBuf>,
```

Validate cardinality immediately after parsing: either both vectors are empty,
or `managed_cache_root.len() == 1` and the source vector is nonempty. Zero or
multiple roots with any source, root-only, and source-only all return
`GuardExecError::InvalidManagedCache` with usage exit 2 before admission or
cache access. This raw-vector representation makes repeated-root rejection
explicit instead of relying on ambiguous `Option<PathBuf>` Clap behavior. Add a
separate fail-closed internal/cache error path for busy or changed owned
entries. Neither structured output nor persistent state may include the
absolute source.

Implement that rule in a side-effect-free
`validate_guard_cache_args(&GuardExecArgs)` helper. Parser tests must first call
`Cli::try_parse_from`, extract `GuardExecArgs`, and then call this helper; they
must not claim Clap itself rejects combinations that are intentionally parsed
as raw vectors. `print_guard_exec` calls the same helper before admission,
filesystem canonicalization, or cache access.

- [ ] **Step 8: Write the post-acquisition race and lifecycle tests**

Extract two private helpers so the race boundary is directly testable:

```rust
fn execute_with_guard_cache_pins<T>(
    pins: &[CacheUsePin],
    child: impl FnOnce() -> Result<T, GuardExecError>,
) -> Result<T, GuardExecError>;

fn with_guard_cache_pins<T>(
    cache: Option<&ManagedCache>,
    sources: &[PathBuf],
    child: impl FnOnce() -> Result<T, GuardExecError>,
) -> Result<T, GuardExecError>;
```

The first helper revalidates already-acquired pins immediately before calling
the closure. The second acquires pins, calls the first, captures the closure
result, and drops pins only after that result exists. Test with an atomic call
counter that:

- missing/symlink-replaced data after acquisition yields zero child calls;
- success and child error both observe the lock as busy inside the closure;
- the lock is available after the helper returns;
- duplicate sources still create one pin.

For the race test, acquire pins directly, remove or symlink-replace the owned
data source, then call `execute_with_guard_cache_pins`; this creates the exact
post-acquisition/pre-child sequence without a production sleep or global hook.
Treat this explicitly as a non-cooperative TOCTOU-limit test: removing or
replacing a path while its advisory lock is open is platform-sensitive, so the
portable assertion is only validator failure plus zero child calls. Test the
cooperative lock-busy lifetime separately on an untouched fixture.

- [ ] **Step 9: Run lifecycle tests and verify RED**

Run:

```console
rtk cargo test --bin commit-ci-preflight guard_cache_pins
```

Expected: compile failure until the helper and guard integration exist.

- [ ] **Step 10: Integrate pins into `print_guard_exec`**

Before admission, resolve the explicit root with the existing
`CacheRootOptions`/`ResolvedCacheRoot::resolve` predicates and open it read-only;
do not initialize it. After `GuardExecSession::acquire` and before child launch,
call `with_guard_cache_pins`. Its closure performs
`supervisor.execute_with_output` and then `session.finish(...)`, so pins remain
in scope through both child containment cleanup and guard-session release. The
helper drops them only after that closure returns.

If pin acquisition or revalidation fails after admission, call
`session.finish(Err(...), &cancellation)` and return without starting the child.
Legacy calls with no managed-cache flags bypass only the pin helper; admission,
resource watchdog, child containment, and exit classification remain unchanged.

- [ ] **Step 11: Verify Task 3 GREEN**

Run:

```console
rtk cargo test --lib completed_source_pin
rtk cargo test --bin commit-ci-preflight guard_exec
rtk cargo test --test guard_exec_cli --no-run
rtk cargo test --lib cache::tests
```

Expected: all focused tests pass; the ignored native guard test is compiled but
not executed.

- [ ] **Step 12: Commit Task 3**

```console
rtk git add src/cache.rs src/main.rs tests/guard_exec_cli.rs
rtk git commit -m "feat: pin declared guard cache sources"
```

### Task 4: Document the contract and run the non-heavy verification gate

**Files:**
- Create: `tests/cache_pin_contract.rs`
- Modify: `docs/CACHE_AND_WORKSPACE.md`
- Modify: `docs/RUNTIME.md`
- Modify: `docs/LOCAL_RUN.md`
- Modify: `docs/COORDINATION_RUNBOOK.md`
- Modify: `docs/TESTING_AND_FAULT_INJECTION.md`
- Modify: `docs/THREAT_MODEL.md`
- Modify: `/private/tmp/ccp-cache-use-pin-checkpoint.md` outside Git only as the durable operator checkpoint.

**Interfaces:**
- Consumes: final cache pin flags and typed behavior from Tasks 1-3.
- Produces: operator guidance that distinguishes standard-run locks, opt-in guard pins, spawn-boundary revalidation, and unsupported manual deletion.
- Produces: a locally reviewed issue #4 comment draft with exact commit/test
  evidence, but does not post it, push, open a PR, merge, publish a receipt, or
  run CCP.

- [ ] **Step 1: Add a documentation contract test before editing prose**

Create `tests/cache_pin_contract.rs`. Load each named Markdown file from
`env!("CARGO_MANIFEST_DIR")`, join their text, and require these exact concepts:

```rust
use std::fs;
use std::path::Path;

#[test]
fn cache_pin_documentation_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let docs = [
        "docs/CACHE_AND_WORKSPACE.md",
        "docs/RUNTIME.md",
        "docs/LOCAL_RUN.md",
        "docs/COORDINATION_RUNBOOK.md",
        "docs/TESTING_AND_FAULT_INJECTION.md",
        "docs/THREAT_MODEL.md",
    ]
    .map(|path| (path, fs::read_to_string(root.join(path)).expect("read contract doc")));
    let combined_docs = docs
        .iter()
        .map(|(_, text)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    for required in [
        "--managed-cache-root",
        "--managed-cache-source",
        "spawn-boundary revalidation",
        "undeclared paths are not pinned",
        "manual deletion remains unsupported",
    ] {
        assert!(combined_docs.contains(required), "missing {required}");
    }

    for forbidden in [
        "cache pin uses a TTL lease",
        "automatic cache deletion is enabled",
        "guard exec emits a receipt",
        "external same-path replacement is prevented",
    ] {
        assert!(!combined_docs.contains(forbidden), "forbidden claim: {forbidden}");
    }

    for (path, required) in [
        ("docs/CACHE_AND_WORKSPACE.md", "--managed-cache-source"),
        ("docs/RUNTIME.md", "spawn-boundary revalidation"),
        ("docs/LOCAL_RUN.md", "--managed-cache-root"),
        ("docs/COORDINATION_RUNBOOK.md", "undeclared paths are not pinned"),
        ("docs/TESTING_AND_FAULT_INJECTION.md", "non-cooperative"),
        ("docs/THREAT_MODEL.md", "manual deletion remains unsupported"),
    ] {
        let text = docs
            .iter()
            .find(|(candidate, _)| *candidate == path)
            .map(|(_, text)| text)
            .expect("named contract document");
        assert!(text.contains(required), "{path} missing {required}");
    }
}
```

- [ ] **Step 2: Run the documentation test and verify RED**

Run:

```console
rtk cargo test --test cache_pin_contract
```

Expected: FAIL because the new flag and lifecycle language is absent.

- [ ] **Step 3: Update the six contract documents**

Document these exact facts:

- standard `run` retains the prepared entry lock and validates the exact staging
  generation immediately before Docker creation;
- `guard exec` pins only exact completed entry data paths explicitly declared
  under one already-owned persistent cache root;
- pins use the existing advisory entry lock, not TTL/heartbeat state;
- declarations are cooperative and do not prove actual child argv use;
- undeclared paths have no pin guarantee;
- pin validation never initializes, repairs, deletes, quarantines, or publishes
  cache contents;
- any future mutator must acquire the same lock and revalidate after acquisition;
- manual deletion and non-cooperative external replacement remain unsupported;
- Docker/OrbStack/native qualification remains separate exact-head evidence.

Do not copy absolute fixture paths, usernames, raw child argv, or cache contents
into documentation evidence.

Read `docs/RECEIPT_SPEC.md` and `docs/CONFIGURATION.md` during this step to
confirm the new internal fields and guard-only flags do not contradict receipt
or configuration claims. Do not edit them unless a concrete contradiction is
found and first recorded in the checkpoint.

- [ ] **Step 4: Verify documentation GREEN**

Run:

```console
rtk cargo test --test cache_pin_contract
rtk rg -n "managed-cache-(root|source)|spawn-boundary|manual deletion" docs
```

Expected: the contract test passes and every operational claim has one clear
home rather than contradictory duplicates.

- [ ] **Step 5: Run formatting, lint, and the full non-heavy suite**

Run:

```console
rtk cargo fmt --all -- --check
rtk cargo clippy --all-targets --all-features -- -D warnings
rtk cargo test --all-targets --all-features
rtk cargo test --doc
rtk git diff --check
```

Expected: zero failures and zero warnings. The ignored native guard test remains
ignored; no Docker or CCP command is invoked.

- [ ] **Step 6: Commit Task 4**

```console
rtk git add docs/CACHE_AND_WORKSPACE.md docs/RUNTIME.md docs/LOCAL_RUN.md docs/COORDINATION_RUNBOOK.md docs/TESTING_AND_FAULT_INJECTION.md docs/THREAT_MODEL.md tests/cache_pin_contract.rs
rtk git commit -m "docs: define cache pin lifecycle contract"
```

- [ ] **Step 7: Prepare the issue #4 comment draft locally**

First save the proposed comment in this plan's SDD workspace. It must contain:

- exact branch HEAD and baseline;
- the historical Docker exit 125 classified as correct fail-closed behavior,
  without claiming the deleting actor;
- implementation summary for clone lifetime, spawn revalidation, and guard pin;
- exact test commands and terminal counts;
- explicit limitations: cooperative lock only, no automatic cleanup, no receipt,
  no heavy/native qualification, no arbitrary external replacement guarantee;
- a statement that no retry authorization was consumed.

Review the body locally and stop with its path and hash recorded. Posting the
comment is a separate GitHub mutation requiring a later explicit authorization;
this plan records `not posted`. Do not open a duplicate issue or modify any PR.

- [ ] **Step 8: Update the durable checkpoint**

Append task commits, RED/GREEN evidence with exact terminal test counts, final
verification output, issue-comment draft path/hash and `not posted`, remaining
qualification boundaries, baseline/final HEADs, and exact clean/dirty status to
`/private/tmp/ccp-cache-use-pin-checkpoint.md` using `apply_patch`.

- [ ] **Step 9: Stop at the branch-review boundary**

Confirm the worktree is clean and report the exact HEAD. Do not push, open a PR,
run CCP, publish evidence, or merge without new explicit authorization.
