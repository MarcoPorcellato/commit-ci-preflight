# Admission Layout Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an explicit, hash-bound CCP command that diagnoses and preserves one provably empty historical `agent-tickets/` directory without weakening normal admission.

**Architecture:** `AdmissionCoordinator` keeps its strict normal layout validator. A separate recovery-only validator acquires the existing queue and slot locks, constructs a privacy-bounded canonical plan for the exact empty historical layout, and applies it only when the caller supplies the matching SHA-256. Apply atomically moves the empty directory beneath the existing quarantine directory; it never parses or ignores agent state.

**Tech Stack:** Rust 2024; existing `clap`, `fs2`, `serde`, `serde_json`, and `sha2`; deterministic unit and contract tests. No new dependency, Docker, network, model, or global coordinator access.

**Spec:** `docs/superpowers/specs/2026-08-24-admission-layout-recovery-design.md`

## Global Constraints

- Normal `validate_layout`, `admission status`, `run`, `benchmark`, and `guard exec` must continue to reject `agent-tickets/` until explicit recovery succeeds.
- Recovery supports only the exact child name `agent-tickets`; every other unknown root child remains blocking.
- Recovery never parses, adopts, deletes, overwrites, or silently ignores agent-ticket records, staging files, canonical tickets, leases, or active locks.
- `status` is read-only; `apply` requires one exact lowercase 64-character plan SHA-256 and revalidates under lock.
- Lock order is queue first, slot second; release order is slot first, queue second.
- Timeouts default to 5 seconds and accept only integers from 1 through 60 seconds.
- JSON contains no absolute path, process data, repository, user, command, record content, or environment value.
- The release binary gains no admission-root flag, environment override, hidden test mode, or alternate-root behavior.
- Receipt, journal, source-binding, resource-policy, and normal admission-status schemas remain unchanged.
- Tests use owned temporary roots only; no test or implementation step accesses the live platform coordinator.
- No CCP `run`, Docker operation, evidence publication, push, or R5 action belongs to this plan.

---

## File map

| File | Responsibility |
|---|---|
| `src/admission.rs` | Recovery schemas, reason codes, plan digest, recovery-only structural validation, lock-scoped status/apply, and coordinator unit tests. |
| `src/durable_fs.rs` | One fail-closed primitive that moves a plain empty directory between owned plain-directory parents and synchronizes both parents. |
| `src/main.rs` | Nested CLI parsing, 1–60 second validation, test-only injected-coordinator dispatch seam, bounded rendering, and reported exit 70 for both non-success apply outcomes. |
| `docs/COORDINATION_RUNBOOK.md` | Normative two-step operator flow and explicit authorization boundaries. |
| `docs/TROUBLESHOOTING.md` | Safe diagnosis for the exact historical empty-directory case; manual filesystem recovery remains unsupported. |
| `tests/agent_integration_contract.rs` | Public documentation contract for hash-bound recovery, no manual cleanup, and no implicit heavy authorization. |

### Task 1: Add the read-only recovery plan and CLI status surface

**Files:**
- Modify: `src/admission.rs:25-110,197-420,926-990,1483-1605,1640-end`
- Modify: `src/main.rs:80-110,404-435,437-520,1534-1565,2315-end`

**Interfaces:**
- Consumes: existing `AdmissionCoordinator`, `AdmissionDeadline`, `CancellationToken`, `QUEUE_LOCK`, `SLOT_LOCK`, `OWNER_BYTES`, `validate_regular`, `lock_exclusive_until`, and `unlock`.
- Produces:

```rust
pub const ADMISSION_LAYOUT_RECOVERY_SCHEMA_VERSION: &str =
    "admission-layout-recovery/1.0";
pub const DEFAULT_LAYOUT_RECOVERY_TIMEOUT: Duration = Duration::from_secs(5);
pub const MAX_LAYOUT_RECOVERY_TIMEOUT_SECONDS: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionLayoutRecoveryClassificationV1 {
    NotNeeded,
    RecoverableEmptyHistoricalAgentTickets,
    OperatorRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionLayoutRecoveryReasonV1 {
    CanonicalLayout,
    EmptyHistoricalAgentTickets,
    LockTimeout,
    ForeignOwner,
    UnsupportedLayout,
    TargetNotEmpty,
    CoordinatorNotIdle,
    QuarantineCollision,
    PlanMismatch,
    FilesystemUncertain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdmissionLayoutRecoveryStatusV1 {
    pub schema_version: String,
    pub classification: AdmissionLayoutRecoveryClassificationV1,
    pub target_kind: Option<String>,
    pub reason: AdmissionLayoutRecoveryReasonV1,
    pub plan_sha256: Option<String>,
}

impl AdmissionCoordinator {
    pub fn layout_recovery_status_with_timeout(
        &self,
        timeout: Duration,
        cancellation: &CancellationToken,
    ) -> AdmissionLayoutRecoveryStatusV1;
}
```

- Produces CLI shapes:

```rust
#[derive(Debug, Subcommand)]
enum AdmissionCommand {
    Status {
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = DEFAULT_STATUS_TIMEOUT.as_secs())]
        timeout_seconds: u64,
    },
    LayoutRecovery {
        #[command(subcommand)]
        action: AdmissionLayoutRecoveryCommand,
    },
}

#[derive(Debug, Subcommand)]
enum AdmissionLayoutRecoveryCommand {
    Status {
        #[arg(long)]
        json: bool,
        #[arg(
            long,
            default_value_t = DEFAULT_LAYOUT_RECOVERY_TIMEOUT.as_secs(),
            value_parser = parse_layout_recovery_timeout
        )]
        timeout_seconds: u64,
    },
}

fn parse_layout_recovery_timeout(value: &str) -> Result<u64, String> {
    let seconds = value
        .parse::<u64>()
        .map_err(|_| "layout recovery timeout must be an integer from 1 through 60".to_owned())?;
    if !(1..=MAX_LAYOUT_RECOVERY_TIMEOUT_SECONDS).contains(&seconds) {
        return Err("layout recovery timeout must be an integer from 1 through 60".to_owned());
    }
    Ok(seconds)
}
```

- [ ] **Step 1: Write the CLI RED tests**

Add `src/main.rs` tests that parse the public command and reject out-of-range timeouts without running a dispatcher:

```rust
#[test]
fn admission_layout_recovery_status_parses_only_bounded_timeouts() {
    let parsed = Cli::try_parse_from([
        "commit-ci-preflight",
        "admission",
        "layout-recovery",
        "status",
        "--json",
        "--timeout-seconds",
        "5",
    ]);
    assert!(parsed.is_ok());

    for invalid in ["0", "61", "not-a-number"] {
        assert!(Cli::try_parse_from([
            "commit-ci-preflight",
            "admission",
            "layout-recovery",
            "status",
            "--timeout-seconds",
            invalid,
        ])
        .is_err());
    }
}
```

- [ ] **Step 2: Run the parser test and confirm RED**

Run: `rtk cargo test --locked --bin commit-ci-preflight tests::admission_layout_recovery_status_parses_only_bounded_timeouts -- --exact`

Expected: FAIL because `layout-recovery` is not an `AdmissionCommand` variant.

- [ ] **Step 3: Write the coordinator RED tests**

In `src/admission.rs`, change `AdmissionCoordinator::test_at` to `pub(crate)` under `#[cfg(test)]`, then add a fixture that initializes a canonical root before adding the incompatible child:

```rust
fn coordinator_with_empty_historical_agent_tickets(
    label: &str,
) -> AdmissionCoordinator {
    let coordinator = coordinator(label);
    coordinator.initialize().expect("canonical coordinator");
    fs::create_dir(coordinator.root().join("agent-tickets"))
        .expect("historical empty directory");
    coordinator
}

#[test]
fn normal_status_stays_closed_but_recovery_status_plans_empty_historical_directory() {
    let coordinator = coordinator_with_empty_historical_agent_tickets("layout-status");
    assert!(matches!(
        coordinator.status(),
        Err(AdmissionError::UnsafeLayout(_))
    ));

    let before = tree_fingerprint(coordinator.root());
    let report = coordinator.layout_recovery_status_with_timeout(
        Duration::from_secs(1),
        &CancellationToken::default(),
    );
    assert_eq!(
        report.classification,
        AdmissionLayoutRecoveryClassificationV1::RecoverableEmptyHistoricalAgentTickets
    );
    let digest = report.plan_sha256.expect("recovery plan");
    assert_eq!(digest.len(), 64);
    assert!(digest.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    assert_eq!(before, tree_fingerprint(coordinator.root()));
}
```

Add this test helper; it records every entry and excludes nothing:

```rust
fn tree_fingerprint(root: &Path) -> Vec<(PathBuf, &'static str, Vec<u8>)> {
    fn walk(
        root: &Path,
        path: &Path,
        out: &mut Vec<(PathBuf, &'static str, Vec<u8>)>,
    ) {
        let mut entries = fs::read_dir(path)
            .expect("read fingerprint directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("fingerprint entries");
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            let relative = path.strip_prefix(root).expect("relative path").to_path_buf();
            let metadata = fs::symlink_metadata(&path).expect("fingerprint metadata");
            if metadata.file_type().is_symlink() {
                let target = fs::read_link(&path)
                    .expect("symlink target")
                    .to_string_lossy()
                    .as_bytes()
                    .to_vec();
                out.push((relative, "symlink", target));
            } else if metadata.is_dir() {
                out.push((relative, "directory", Vec::new()));
                walk(root, &path, out);
            } else {
                out.push((relative, "file", fs::read(&path).expect("file bytes")));
            }
        }
    }
    let mut entries = Vec::new();
    walk(root, root, &mut entries);
    entries
}
```

- [ ] **Step 4: Run the coordinator test and confirm RED**

Run: `rtk cargo test --locked --lib admission::tests::normal_status_stays_closed_but_recovery_status_plans_empty_historical_directory -- --exact`

Expected: FAIL to compile because the recovery status types and method do not exist.

- [ ] **Step 5: Implement the minimal read-only recovery model**

Add private strict plan types and digesting without a new dependency:

```rust
#[derive(Serialize)]
struct RecoveryRootEntryV1 {
    name: String,
    kind: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CoordinatorOwnerMarkerV1 {
    owner: String,
    purpose: String,
    schema_version: String,
}

#[derive(Serialize)]
struct AdmissionLayoutRecoveryPlanV1 {
    schema_version: &'static str,
    recovery_kind: &'static str,
    owner: String,
    purpose: String,
    owner_schema_version: String,
    root_entries: Vec<RecoveryRootEntryV1>,
    queue_lock_name: &'static str,
    queue_lock_kind: &'static str,
    queue_lock_exclusively_held: bool,
    slot_lock_name: &'static str,
    slot_lock_kind: &'static str,
    slot_lock_was_free: bool,
    ticket_count: usize,
    lease_count: usize,
    target_entry_count: usize,
}

fn layout_recovery_plan_digest(plan: &AdmissionLayoutRecoveryPlanV1) -> Result<String, AdmissionError> {
    let bytes = serde_json::to_vec(plan).map_err(AdmissionError::Json)?;
    let digest = Sha256::digest(bytes);
    let mut value = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut value, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(value)
}
```

Implement a dedicated structural validator that recognizes only the exact
plain-directory target, validates every other canonical entry with existing
helpers, and never calls normal `validate_layout` after deciding to inspect
the historical case. Require existing plain `queue.lock` and `slot.lock` files,
then acquire queue followed by slot; do not create or truncate either. Read and
strictly deserialize the actual owner marker, require its raw bytes to equal
`OWNER_BYTES`, and put its validated tuple into the plan rather than copying
unobserved constants. Require empty `tickets`, `leases`,
and target inventories. Set `target_kind` only to the bounded literal
`historical_agent_tickets`. After hashing the plan, derive
`agent-tickets.recovered-v1-{plan_sha256}` and require that exact quarantine
destination to be absent before returning a plan. Release slot then queue on
every return.

Map failures into closed reason codes. `AdmissionError::Timeout` while locking
maps to `OperatorRequired/LockTimeout`; unknown siblings and invalid types map
to `OperatorRequired/UnsupportedLayout`; target contents map to
`OperatorRequired/TargetNotEmpty`; any canonical ticket or lease maps to
`OperatorRequired/CoordinatorNotIdle`.

- [ ] **Step 6: Implement parser and test-only dispatcher injection**

Production dispatch must construct the platform coordinator internally:

```rust
fn run_admission_command(action: AdmissionCommand) -> Result<(), CliError> {
    let coordinator = AdmissionCoordinator::platform().map_err(CliError::Admission)?;
    run_admission_command_with(action, coordinator)
}

fn run_admission_command_with(
    action: AdmissionCommand,
    coordinator: AdmissionCoordinator,
) -> Result<(), CliError> {
    match action {
        AdmissionCommand::Status { json, timeout_seconds } => {
            render_admission_status(&coordinator, json, timeout_seconds)
        }
        AdmissionCommand::LayoutRecovery {
            action: AdmissionLayoutRecoveryCommand::Status { json, timeout_seconds },
        } => render_layout_recovery_status(&coordinator, json, timeout_seconds),
    }
}

fn render_admission_status(
    coordinator: &AdmissionCoordinator,
    json: bool,
    timeout_seconds: u64,
) -> Result<(), CliError> {
    let cancellation = CancellationToken::default();
    let status = coordinator
        .status_with_timeout(Duration::from_secs(timeout_seconds), &cancellation)
        .map_err(CliError::Admission)?;
    if json {
        println!("{}", serde_json::to_string(&status).map_err(CliError::internal)?);
    } else {
        println!("Admission schema: {ADMISSION_STATUS_SCHEMA_VERSION}");
        println!("Active: {}", status.active);
        println!("Queued: {}", status.queue_count);
        println!("Slot lock: {}", status.slot.state);
        println!("Slot owner/run: {:?}", status.slot.owner_run_id);
        println!("Slot lease: {}", status.slot.lease_state);
        println!("Queue lock: {}", status.queue_lock.state);
        for ticket in &status.ticket_ids {
            println!("  - {ticket}");
        }
        println!("Note: {}", status.process_visibility_note);
    }
    Ok(())
}

fn render_layout_recovery_status(
    coordinator: &AdmissionCoordinator,
    json: bool,
    timeout_seconds: u64,
) -> Result<(), CliError> {
    let report = coordinator.layout_recovery_status_with_timeout(
        Duration::from_secs(timeout_seconds),
        &CancellationToken::default(),
    );
    if json {
        println!("{}", serde_json::to_string(&report).map_err(CliError::internal)?);
    } else {
        println!("Layout recovery schema: {}", report.schema_version);
        println!("Classification: {:?}", report.classification);
        println!("Reason: {:?}", report.reason);
        println!("Plan SHA-256: {:?}", report.plan_sha256);
        println!("Read-only: no state was changed.");
    }
    Ok(())
}
```

The injected coordinator function stays private to `src/main.rs`; only its unit
tests call it. Do not add a process environment lookup or public root option.

- [ ] **Step 7: Verify Task 1 GREEN**

Run: `rtk cargo test --locked --lib layout_recovery`

Expected: PASS with at least one selected read-only recovery status test; zero
selected tests is a failure.

Run: `rtk cargo test --locked --bin commit-ci-preflight admission_layout_recovery`

Expected: PASS with at least one selected parser/dispatch test; zero selected
tests is a failure.

Run: `rtk cargo test --locked --lib status_`

Expected: existing normal admission status tests remain PASS with a nonzero
selected-test count.

- [ ] **Step 8: Commit Task 1**

```bash
rtk git add src/admission.rs src/main.rs
rtk git commit -m "feat: plan empty admission layout recovery"
```

### Task 2: Apply one exact plan through a durable empty-directory move

**Files:**
- Modify: `src/durable_fs.rs:35-180,260-end`
- Modify: `src/admission.rs` recovery types and methods added in Task 1
- Modify: `src/main.rs` by extending Task 1's status-only recovery command

**Interfaces:**
- Consumes: `AdmissionLayoutRecoveryStatusV1.plan_sha256`, Task 1's lock-scoped plan constructor, `DurableFileSystem`, and the existing `keep_first_error`/unlock pattern.
- Produces:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionLayoutRecoveryOutcomeV1 {
    Recovered,
    NotApplied,
    RecoveryUncertain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdmissionLayoutRecoveryApplyV1 {
    pub schema_version: String,
    pub outcome: AdmissionLayoutRecoveryOutcomeV1,
    pub reason: AdmissionLayoutRecoveryReasonV1,
    pub quarantine_entry: Option<String>,
}

impl AdmissionCoordinator {
    pub fn apply_layout_recovery_with_timeout(
        &self,
        expected_plan: &str,
        timeout: Duration,
        cancellation: &CancellationToken,
    ) -> AdmissionLayoutRecoveryApplyV1;
}

impl DurableFileSystem {
    pub(crate) fn relocate_empty_directory(
        &self,
        source: &Path,
        destination: &Path,
    ) -> Result<(), DurableFsError>;
}

#[derive(Debug, Subcommand)]
enum AdmissionLayoutRecoveryCommand {
    Status {
        #[arg(long)]
        json: bool,
        #[arg(
            long,
            default_value_t = DEFAULT_LAYOUT_RECOVERY_TIMEOUT.as_secs(),
            value_parser = parse_layout_recovery_timeout
        )]
        timeout_seconds: u64,
    },
    Apply {
        #[arg(long, value_parser = parse_plan_sha256)]
        expected_plan: String,
        #[arg(long)]
        json: bool,
        #[arg(
            long,
            default_value_t = DEFAULT_LAYOUT_RECOVERY_TIMEOUT.as_secs(),
            value_parser = parse_layout_recovery_timeout
        )]
        timeout_seconds: u64,
    },
}

fn parse_plan_sha256(value: &str) -> Result<String, String> {
    if value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(value.to_owned())
    } else {
        Err("expected plan must be exactly 64 lowercase hexadecimal characters".to_owned())
    }
}
```

- [ ] **Step 1: Write durable-filesystem RED tests**

```rust
#[test]
fn relocate_empty_directory_preserves_source_as_append_only_destination() {
    let root = temporary_directory("relocate-empty");
    let source = root.join("agent-tickets");
    let quarantine = root.join("quarantine");
    let destination = quarantine.join("agent-tickets.recovered-v1-plan");
    fs::create_dir(&source).expect("source");
    fs::create_dir(&quarantine).expect("quarantine");

    DurableFileSystem::default()
        .relocate_empty_directory(&source, &destination)
        .expect("relocate");

    assert!(!source.exists());
    assert!(destination.is_dir());
    assert!(DurableFileSystem::default()
        .relocate_empty_directory(&destination, &destination)
        .is_err());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn relocate_empty_directory_rejects_contents_and_existing_destination() {
    let root = temporary_directory("relocate-reject");
    let source = root.join("agent-tickets");
    let quarantine = root.join("quarantine");
    let destination = quarantine.join("agent-tickets.recovered-v1-plan");
    fs::create_dir(&source).expect("source");
    fs::create_dir(&quarantine).expect("quarantine");
    fs::write(source.join("entry"), b"blocked\n").expect("source entry");

    assert!(DurableFileSystem::default()
        .relocate_empty_directory(&source, &destination)
        .is_err());
    assert_eq!(fs::read(source.join("entry")).expect("entry remains"), b"blocked\n");
    assert!(!destination.exists());

    fs::remove_file(source.join("entry")).expect("remove fixture entry");
    fs::create_dir(&destination).expect("destination collision");
    assert!(DurableFileSystem::default()
        .relocate_empty_directory(&source, &destination)
        .is_err());
    assert!(source.is_dir());
    assert!(destination.is_dir());
    fs::remove_dir_all(root).expect("cleanup");
}
```

- [ ] **Step 2: Run durable tests and confirm RED**

Run: `rtk cargo test --locked --lib durable_fs::tests::relocate_empty_directory`

Expected: FAIL to compile because `relocate_empty_directory` does not exist.

- [ ] **Step 3: Implement the minimal durable move**

```rust
pub(crate) fn relocate_empty_directory(
    &self,
    source: &Path,
    destination: &Path,
) -> Result<(), DurableFsError> {
    let source_parent = checked_parent(source)?;
    let destination_parent = checked_parent(destination)?;
    validate_plain_directory(source_parent)?;
    validate_plain_directory(destination_parent)?;
    validate_plain_directory(source)?;
    if fs::read_dir(source)?.next().transpose()?.is_some() {
        return Err(DurableFsError::UnsafePath("source directory must be empty"));
    }
    if fs::symlink_metadata(destination).is_ok() {
        return Err(DurableFsError::UnsafePath(
            "quarantine destination already exists",
        ));
    }
    self.checkpoint()?;
    fs::rename(source, destination)?;
    self.checkpoint()?;
    sync_directory(destination_parent)?;
    self.checkpoint()?;
    sync_directory(source_parent)?;
    Ok(())
}
```

Handle destination metadata errors other than `NotFound` as I/O uncertainty;
do not treat them as absence. `fs::rename` is the only move operation: a
cross-device/`EXDEV` error maps to filesystem uncertainty and must never fall
back to copy, delete, or recursive movement. Keep both parent syncs even when
one parent is an ancestor of the other.

- [ ] **Step 4: Write coordinator apply RED tests**

```rust
#[test]
fn apply_requires_exact_plan_and_preserves_empty_directory_in_quarantine() {
    let coordinator = coordinator_with_empty_historical_agent_tickets("layout-apply");
    let status = coordinator.layout_recovery_status_with_timeout(
        Duration::from_secs(1),
        &CancellationToken::default(),
    );
    let plan = status.plan_sha256.expect("plan");

    let wrong_before = tree_fingerprint(coordinator.root());
    let wrong = coordinator.apply_layout_recovery_with_timeout(
        &"0".repeat(64),
        Duration::from_secs(1),
        &CancellationToken::default(),
    );
    assert_eq!(wrong.outcome, AdmissionLayoutRecoveryOutcomeV1::NotApplied);
    assert_eq!(wrong_before, tree_fingerprint(coordinator.root()));

    let applied = coordinator.apply_layout_recovery_with_timeout(
        &plan,
        Duration::from_secs(1),
        &CancellationToken::default(),
    );
    assert_eq!(applied.outcome, AdmissionLayoutRecoveryOutcomeV1::Recovered);
    let entry = applied.quarantine_entry.expect("quarantine basename");
    assert_eq!(entry, format!("agent-tickets.recovered-v1-{plan}"));
    assert!(coordinator.root().join("quarantine").join(entry).is_dir());
    assert!(!coordinator.root().join("agent-tickets").exists());
    assert!(!coordinator.status().expect("canonical status").active);

    let after = tree_fingerprint(coordinator.root());
    let status_after = coordinator.layout_recovery_status_with_timeout(
        Duration::from_secs(1),
        &CancellationToken::default(),
    );
    assert_eq!(
        status_after.classification,
        AdmissionLayoutRecoveryClassificationV1::NotNeeded
    );
    assert!(status_after.plan_sha256.is_none());
    let repeated = coordinator.apply_layout_recovery_with_timeout(
        &plan,
        Duration::from_secs(1),
        &CancellationToken::default(),
    );
    assert_eq!(repeated.outcome, AdmissionLayoutRecoveryOutcomeV1::NotApplied);
    assert_eq!(after, tree_fingerprint(coordinator.root()));
}
```

- [ ] **Step 5: Run apply test and confirm RED**

Run: `rtk cargo test --locked --lib admission::tests::apply_requires_exact_plan_and_preserves_empty_directory_in_quarantine -- --exact`

Expected: FAIL to compile because apply types and method do not exist.

- [ ] **Step 6: Implement lock-scoped apply**

Validate `expected_plan` before filesystem access:

```rust
fn valid_plan_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
```

Then acquire queue and slot in the Task 1 order, rebuild the plan, compare the
digest, derive `agent-tickets.recovered-v1-{digest}`, verify the destination is
absent, and call `relocate_empty_directory`. Before returning `Recovered`, run
the unchanged strict `validate_layout(true)` while both locks are still held.
Explicitly release slot then queue; if release fails, return
`RecoveryUncertain/FilesystemUncertain` because the preserved quarantine entry
may already exist.
An invalid or unequal digest returns `NotApplied/PlanMismatch`; an already
canonical layout returns `NotApplied/CanonicalLayout`.

Every early return must leave the source tree byte-identical unless the durable
move itself completed. A post-rename synchronization or validation failure
returns `RecoveryUncertain/FilesystemUncertain` and never attempts rollback or
deletion. Any pre-rename I/O error, including `EXDEV`, returns
`NotApplied/FilesystemUncertain`.

Classify a relocation error from observed paths while the locks are still held:

```rust
fn outcome_after_relocation_error(
    source: &Path,
    destination: &Path,
) -> AdmissionLayoutRecoveryOutcomeV1 {
    match (fs::symlink_metadata(source), fs::symlink_metadata(destination)) {
        (Ok(source_meta), Err(destination_error))
            if source_meta.is_dir()
                && !source_meta.file_type().is_symlink()
                && destination_error.kind() == io::ErrorKind::NotFound =>
        {
            AdmissionLayoutRecoveryOutcomeV1::NotApplied
        }
        _ => AdmissionLayoutRecoveryOutcomeV1::RecoveryUncertain,
    }
}
```

This makes an `EXDEV` rename failure `NotApplied` only when the unchanged
source and absent destination are observable. Any contradictory or unreadable
post-error state is `RecoveryUncertain`.

- [ ] **Step 7: Complete CLI apply rendering and exit behavior**

Add `ReportedExit(i32)` to the existing `CliError` enum. It suppresses an extra
raw error line after the bounded report has been printed. Change main's
terminal error handling to:

```rust
if let Err(error) = result {
    if !matches!(error, CliError::ReportedExit(_)) {
        eprintln!("error: {error}");
    }
    std::process::exit(error.exit_code());
}
```

`run_admission_command_with` prints the apply report. It returns `Ok(())` only
for `Recovered`; `NotApplied` and `RecoveryUncertain` return
`Err(CliError::ReportedExit(70))`.
Malformed or absent `--expected-plan` remains a Clap usage error with code `2`.

Implement the apply renderer exactly as follows:

```rust
fn render_layout_recovery_apply(
    coordinator: &AdmissionCoordinator,
    expected_plan: &str,
    json: bool,
    timeout_seconds: u64,
) -> Result<(), CliError> {
    let report = coordinator.apply_layout_recovery_with_timeout(
        expected_plan,
        Duration::from_secs(timeout_seconds),
        &CancellationToken::default(),
    );
    if json {
        println!("{}", serde_json::to_string(&report).map_err(CliError::internal)?);
    } else {
        println!("Layout recovery schema: {}", report.schema_version);
        println!("Outcome: {:?}", report.outcome);
        println!("Reason: {:?}", report.reason);
        println!("Quarantine entry: {:?}", report.quarantine_entry);
    }
    match report.outcome {
        AdmissionLayoutRecoveryOutcomeV1::Recovered => Ok(()),
        AdmissionLayoutRecoveryOutcomeV1::NotApplied
        | AdmissionLayoutRecoveryOutcomeV1::RecoveryUncertain => {
            Err(CliError::ReportedExit(70))
        }
    }
}
```

Add this arm to `run_admission_command_with`:

```rust
AdmissionCommand::LayoutRecovery {
    action:
        AdmissionLayoutRecoveryCommand::Apply {
            expected_plan,
            json,
            timeout_seconds,
        },
} => render_layout_recovery_apply(
    &coordinator,
    &expected_plan,
    json,
    timeout_seconds,
),
```

Extend `CliError::exit_code` with `Self::ReportedExit(code) => *code` and its
`Display` implementation with
`Self::ReportedExit(_) => formatter.write_str("command outcome already reported")`.

- [ ] **Step 8: Verify Task 2 GREEN**

Run: `rtk cargo test --locked --lib durable_fs::tests::relocate_empty_directory`

Expected: PASS with at least two selected durable relocation tests; zero
selected tests is a failure.

Run: `rtk cargo test --locked --lib layout_recovery`

Expected: PASS with at least two selected coordinator recovery tests covering
plan mismatch, success, canonical post-status, and idempotence; zero selected
tests is a failure.

Run: `rtk cargo test --locked --bin commit-ci-preflight admission_layout_recovery`

Expected: PASS with at least two selected parser/dispatch tests for status and
apply; zero selected tests is a failure.

- [ ] **Step 9: Commit Task 2**

```bash
rtk git add src/durable_fs.rs src/admission.rs src/main.rs
rtk git commit -m "feat: apply hash-bound admission layout recovery"
```

### Task 3: Prove adversarial state remains fail-closed and non-mutating

**Files:**
- Modify: `src/admission.rs` tests and test-only recovery seams
- Modify: `src/durable_fs.rs` fault-injection tests
- Modify: `src/main.rs` tests

**Interfaces:**
- Consumes: Task 1 status report and Task 2 apply report/durable move.
- Produces: deterministic regression coverage for every blocked state in the specification; no new public production interface.

- [ ] **Step 1: Add table-driven structural RED tests**

Add fixtures that start from the owned historical layout and introduce exactly
one mutation per case:

```rust
#[test]
fn layout_recovery_rejects_unsupported_or_nonempty_state_without_mutation() {
    for case in [
        RecoveryFixtureCase::TargetEntry(".agent-ticket-staging-partial"),
        RecoveryFixtureCase::TargetIsFile,
        RecoveryFixtureCase::TargetIsSymlink,
        RecoveryFixtureCase::ForeignOwner,
        RecoveryFixtureCase::MalformedOwner,
        RecoveryFixtureCase::MissingQueueLock,
        RecoveryFixtureCase::QueueLockIsSymlink,
        RecoveryFixtureCase::MissingSlotLock,
        RecoveryFixtureCase::SlotLockIsSymlink,
        RecoveryFixtureCase::CanonicalTicket,
        RecoveryFixtureCase::CanonicalLease,
        RecoveryFixtureCase::UnknownSibling,
    ] {
        let coordinator = recovery_fixture(case);
        let before = tree_fingerprint(coordinator.root());
        let report = coordinator.layout_recovery_status_with_timeout(
            Duration::from_millis(200),
            &CancellationToken::default(),
        );
        assert_eq!(
            report.classification,
            AdmissionLayoutRecoveryClassificationV1::OperatorRequired,
            "case: {case:?}"
        );
        assert!(report.plan_sha256.is_none(), "case: {case:?}");
        assert_eq!(before, tree_fingerprint(coordinator.root()), "case: {case:?}");
    }
}
```

Do not use a mode-`000` test, whose behavior changes with platform and
privilege. Add a deterministic test-only effect at the exact target-inventory
read boundary:

```rust
#[derive(Debug, Clone, Copy, Default)]
struct LayoutRecoveryEffects {
    #[cfg(test)]
    deny_target_inventory: bool,
}

impl LayoutRecoveryEffects {
    fn before_target_inventory(&self, path: &Path) -> Result<(), AdmissionError> {
        #[cfg(test)]
        if self.deny_target_inventory {
            return Err(AdmissionError::Io {
                path: path.to_path_buf(),
                source: io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "injected layout recovery permission denial",
                ),
            });
        }
        Ok(())
    }
}
```

Production calls the planner with `LayoutRecoveryEffects::default()`. A
`#[cfg(test)]` private status seam passes `deny_target_inventory: true`; assert
`OperatorRequired/FilesystemUncertain`, no plan, no path in JSON, and an
identical tree fingerprint.

- [ ] **Step 2: Run structural test and confirm RED**

Run: `rtk cargo test --locked --lib admission::tests::layout_recovery_rejects_unsupported_or_nonempty_state_without_mutation -- --exact`

Expected: at least one case FAILS until reason mapping and inventory checks are complete.

- [ ] **Step 3: Add lock and deadline RED tests**

Hold the canonical slot file with `fs2::FileExt::lock_exclusive`, then request a
50 ms recovery status. Assert completion in under one second,
`OperatorRequired/LockTimeout`, no plan, and an identical tree fingerprint.
Repeat for apply and assert `NotApplied/LockTimeout`.

Also test parser values `0` and `61`; parsing must fail before constructing or
calling a coordinator.

- [ ] **Step 4: Run deadline tests and confirm RED**

Run: `rtk cargo test --locked layout_recovery_lock_timeout`

Expected: FAIL until slot locking and timeout mappings follow the specification.

- [ ] **Step 5: Add plan-race and collision RED tests**

After obtaining a valid plan, separately:

- create one target entry;
- create an unknown root sibling;
- create the deterministic quarantine destination; and
- add one canonical ticket marker.

Call apply with the previously valid plan. Each call must return `NotApplied`,
must preserve the exact pre-apply fingerprint, and must not create another
quarantine entry.

- [ ] **Step 6: Add durable-failure RED tests**

Use `DurableFileSystem::failing_at` through a `#[cfg(test)]` private
`apply_layout_recovery_with_filesystem` seam:

```rust
let before = tree_fingerprint(coordinator.root());
let before_rename = coordinator.apply_layout_recovery_with_filesystem(
    &plan,
    Duration::from_secs(1),
    &CancellationToken::default(),
    &DurableFileSystem::failing_at(1),
);
assert_eq!(before_rename.outcome, AdmissionLayoutRecoveryOutcomeV1::NotApplied);
assert_eq!(before, tree_fingerprint(coordinator.root()));

let after_rename = coordinator.apply_layout_recovery_with_filesystem(
    &plan,
    Duration::from_secs(1),
    &CancellationToken::default(),
    &DurableFileSystem::failing_at(2),
);
assert_eq!(
    after_rename.outcome,
    AdmissionLayoutRecoveryOutcomeV1::RecoveryUncertain
);
assert!(!coordinator.root().join("agent-tickets").exists());
assert!(coordinator
    .root()
    .join("quarantine")
    .join(format!("agent-tickets.recovered-v1-{plan}"))
    .is_dir());
```

No test rolls the directory back or deletes the evidence before fixture cleanup.

- [ ] **Step 7: Add privacy and schema assertions**

Serialize every classification and apply outcome. Assert exact object key
counts and that output contains neither the fixture root nor `/`, `\\`,
`ticket-000`, `lease-`, `HOME`, `repository`, or `command`.

Assert `ADMISSION_STATUS_SCHEMA_VERSION` remains `2.0` and existing
`AdmissionStatusV1` still serializes exactly seven top-level fields.

- [ ] **Step 8: Implement the minimal missing validation and reason mapping**

Complete only the branches exposed by Steps 1–7. Do not add recovery for
non-empty, foreign, malformed, active, or unknown state. Ensure plan building
sorts all bounded root-entry facts before serialization so the digest is stable
across directory enumeration order.

- [ ] **Step 9: Verify Task 3 GREEN**

Run: `rtk cargo test --locked --lib layout_recovery`

Expected: all structural, deadline, race, collision, durability, idempotence,
and privacy tests PASS, with at least six selected tests.

Run: `rtk cargo test --locked --bin commit-ci-preflight admission_layout_recovery`

Expected: all parser/dispatch/exit tests PASS, with at least three selected
tests.

Run: `rtk cargo test --locked --lib admission::tests`

Expected: all legacy admission tests PASS with a nonzero selected-test count.

- [ ] **Step 10: Commit Task 3**

```bash
rtk git add src/admission.rs src/durable_fs.rs src/main.rs
rtk git commit -m "test: harden admission layout recovery boundaries"
```

### Task 4: Publish the supported operator contract

**Files:**
- Modify: `docs/COORDINATION_RUNBOOK.md:20-40,92-112,198-212`
- Modify: `docs/TROUBLESHOOTING.md:39-80`
- Modify: `tests/agent_integration_contract.rs`

**Interfaces:**
- Consumes: exact Task 1/2 command names, schema, digest, timeout, and outcomes.
- Produces: public instructions that permit only hash-bound CCP recovery and keep all later runs separately authorized.

- [ ] **Step 1: Write failing documentation contract assertions**

```rust
#[test]
fn admission_layout_recovery_guidance_is_hash_bound_and_never_manual() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let runbook = read(root, "docs/COORDINATION_RUNBOOK.md");
    let troubleshooting = read(root, "docs/TROUBLESHOOTING.md");
    for required in [
        "admission layout-recovery status --json",
        "admission layout-recovery apply",
        "--expected-plan",
        "recovery_uncertain",
        "does not authorize",
        "Manual deletion",
    ] {
        assert!(
            runbook.contains(required) || troubleshooting.contains(required),
            "missing recovery boundary: {required}"
        );
    }
    assert!(!runbook.contains("ignore `agent-tickets`"));
}
```

- [ ] **Step 2: Run the contract and confirm RED**

Run: `rtk cargo test --locked --test agent_integration_contract admission_layout_recovery_guidance_is_hash_bound_and_never_manual -- --exact`

Expected: FAIL because the commands and exact boundary are undocumented.

- [ ] **Step 3: Add the minimal normative runbook sequence**

Document:

```console
commit-ci-preflight admission layout-recovery status --json --timeout-seconds 5
# preserve the exact plan_sha256 and obtain one explicit apply authorization
commit-ci-preflight admission layout-recovery apply \
  --expected-plan <exact-status-plan-sha256> --json --timeout-seconds 5
commit-ci-preflight admission status --json
```

State that status is read-only, apply supports only the exact empty historical
directory, the directory is preserved beneath quarantine, manual filesystem
equivalents remain unsupported, and successful apply does not authorize a
heavy run, Docker, receipt publication, or R5 retry.

Document `recovery_uncertain` as a hard stop: preserve both paths and the JSON,
do not retry apply, do not move either directory manually, and do not start a
heavy run.

- [ ] **Step 4: Update troubleshooting without creating a cleanup recipe**

Route only the exact `UnsafeLayout(.../agent-tickets)` case to recovery status.
Any non-empty, foreign, malformed, active, lock-timeout, plan-mismatch, or
unknown-child result remains code 70/operator-required. Do not publish absolute
example home paths or shell `mv`/`rm` commands.

- [ ] **Step 5: Verify Task 4 GREEN**

Run: `rtk cargo test --locked --test agent_integration_contract admission_layout_recovery_guidance_is_hash_bound_and_never_manual -- --exact`

Expected: PASS with a nonzero selected-test count.

Run: `rtk git diff --check`

Expected: PASS with no whitespace errors.

- [ ] **Step 6: Commit Task 4**

```bash
rtk git add docs/COORDINATION_RUNBOOK.md docs/TROUBLESHOOTING.md tests/agent_integration_contract.rs
rtk git commit -m "docs: add hash-bound admission layout recovery"
```

### Task 5: Full deterministic verification and review gate

**Files:**
- Review: `src/admission.rs`, `src/durable_fs.rs`, `src/main.rs`
- Review: `docs/COORDINATION_RUNBOOK.md`, `docs/TROUBLESHOOTING.md`
- Review: `tests/agent_integration_contract.rs`

**Interfaces:**
- Consumes: Tasks 1–4 exact commits.
- Produces: a source/test review receipt only; no live coordinator recovery or PR evidence receipt.

- [ ] **Step 1: Run formatting and diff validation**

Run: `rtk cargo fmt --all -- --check`

Expected: PASS.

Run: `rtk git diff --check origin/main...HEAD`

Expected: PASS.

- [ ] **Step 2: Run focused deterministic suites**

Run: `rtk cargo test --locked --lib layout_recovery`

Expected: PASS with at least six selected tests; zero selected tests is a
verification failure.

Run: `rtk cargo test --locked --lib durable_fs::tests`

Expected: PASS with a nonzero selected-test count.

Run: `rtk cargo test --locked --bin commit-ci-preflight admission_layout_recovery`

Expected: PASS with at least three selected tests; zero selected tests is a
verification failure.

Run: `rtk cargo test --locked --test agent_integration_contract`

Expected: PASS with a nonzero selected-test count.

- [ ] **Step 3: Run static compilation with warnings denied**

Run: `rtk env RUSTFLAGS=-Dwarnings cargo check --locked --all-targets`

Expected: PASS with no new dependency or warning.

Run: `rtk git diff origin/main...HEAD -- Cargo.toml Cargo.lock`

Expected: empty.

- [ ] **Step 4: Run the full deterministic suite**

Run: `rtk cargo test --locked`

Expected: PASS with a nonzero selected-test count. Record ignored native/heavy
tests as `NOT_RUN`; do not execute them.

- [ ] **Step 5: Verify privacy and scope**

Run: `rtk rg -n '/Users/|/private/tmp|CCP_ADMISSION_TEST_ROOT|GitNexus|Serena|Bearer |ghp_|token=' src docs tests`

Expected: no new match in the Task 1–4 diff. Existing unrelated matches must be
reported separately rather than edited.

Run: `rtk git diff --stat 6686f39..HEAD`

Expected: only the planned Rust, docs, and contract-test files; no receipt,
cache, target artifact, binary, or global-state record.

- [ ] **Step 6: Request two-stage review**

First review specification compliance against the design and this plan. Then
review code quality, lock ordering, error preservation, path privacy, and
no-mutation tests. Any Critical or Important finding starts one bounded fix
round with focused RED/GREEN evidence before re-review.

- [ ] **Step 7: Freeze the implementation handoff**

Record exact base, head, changed files, focused/full test counts, ignored tests,
and that no live `layout-recovery apply`, CCP `run`, Docker, push, evidence
publication, or R5 action occurred.

Stop for a separate authorization before building a live candidate or touching
the platform admission root. A future live apply must bind exact source commit,
absolute candidate path, candidate SHA-256, recovery plan SHA-256, and one
explicit apply authorization. A later receipt run requires another exact-head
authorization.
