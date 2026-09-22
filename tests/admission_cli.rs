use std::fs;
use std::fs::File;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use commit_ci_preflight::admission::MAX_QUEUE_TICKETS;
use fs2::FileExt;
use serde_json::Value;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_commit-ci-preflight")
}

fn command(home: &PathBuf) -> Command {
    let mut c = Command::new(bin());
    c.env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("LOCALAPPDATA", home.join("local-app-data"));
    c
}

fn admission_root(home: &PathBuf) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library")
            .join("Caches")
            .join("commit-ci-preflight-admission")
    } else if cfg!(windows) {
        home.join("local-app-data")
            .join("commit-ci-preflight-admission")
    } else {
        home.join("cache").join("commit-ci-preflight-admission")
    }
}

fn owned_admission_root(home: &PathBuf) -> PathBuf {
    let root = admission_root(home);
    fs::create_dir_all(root.join("tickets")).unwrap();
    fs::create_dir(root.join("leases")).unwrap();
    fs::create_dir(root.join("quarantine")).unwrap();
    fs::write(
        root.join(".ccp-admission-root-v1.json"),
        b"{\"owner\":\"commit-ci-preflight\",\"purpose\":\"host-admission-coordinator\",\"schema_version\":\"1.0\"}\n",
    )
    .unwrap();
    fs::write(root.join("queue.lock"), []).unwrap();
    fs::write(root.join("slot.lock"), []).unwrap();
    fs::write(root.join("next-ticket-v1"), b"1\n").unwrap();
    root
}

fn ticket(root: &PathBuf, id: &str) {
    fs::write(
        root.join("tickets").join(format!("ticket-{id}.json")),
        format!(
            "{{\"owner\":\"commit-ci-preflight\",\"purpose\":\"host-admission-ticket\",\"schema_version\":\"1.0\",\"ticket_id\":\"{id}\"}}"
        ),
    )
    .unwrap();
}

fn expired_lease(root: &PathBuf, id: &str) {
    fs::write(
        root.join("leases").join(format!("lease-{id}.json")),
        format!(
            "{{\"owner\":\"commit-ci-preflight\",\"purpose\":\"host-admission-lease\",\"schema_version\":\"1.0\",\"owner_run_id\":\"{id}\",\"acquired_at_unix_seconds\":1,\"heartbeat_at_unix_seconds\":1,\"state\":\"active\"}}"
        ),
    )
    .unwrap();
}

fn fixture() -> PathBuf {
    let root = fs::canonicalize(env!("CARGO_MANIFEST_DIR"))
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
        .join(format!(
            ".ccp-admission-cli-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn preview_json_is_read_only_bounded_and_status_schema_stays_frozen() {
    let home = fixture();
    let before = fs::read_dir(&home).unwrap().count();
    let preview = command(&home)
        .args(["admission", "reconcile", "--json"])
        .output()
        .unwrap();
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let value: Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(value["mode"], "Preview");
    assert_eq!(value["schema_version"], "1.0");
    assert_eq!(value["reason_category"], "completed");
    assert_eq!(value["outcomes"], serde_json::json!([]));
    assert!(!String::from_utf8_lossy(&preview.stdout).contains(home.to_str().unwrap()));
    assert_eq!(before, fs::read_dir(&home).unwrap().count());

    let status = command(&home)
        .args(["admission", "status", "--json"])
        .output()
        .unwrap();
    assert!(status.status.success());
    let object = serde_json::from_slice::<Value>(&status.stdout).unwrap();
    assert_eq!(object.as_object().unwrap().len(), 7);
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn operational_block_and_unknown_target_are_bounded_non_usage_reports() {
    let home = fixture();
    let root = owned_admission_root(&home);
    let selected = "00000000000000000001";
    ticket(&root, selected);
    let slot = File::options()
        .read(true)
        .write(true)
        .open(root.join("slot.lock"))
        .unwrap();
    slot.lock_exclusive().unwrap();

    let blocked = command(&home)
        .args([
            "admission",
            "reconcile",
            "--apply",
            "--ticket-id",
            selected,
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(blocked.status.code(), Some(4));
    let blocked_json: Value = serde_json::from_slice(&blocked.stdout).unwrap();
    assert_eq!(blocked_json["schema_version"], "1.0");
    assert_eq!(blocked_json["mode"], "Apply");
    assert_eq!(blocked_json["reason_category"], "slot_busy");
    assert_eq!(
        blocked_json["outcomes"],
        serde_json::json!([{"ticket_id": selected, "classification": "blocked"}])
    );
    FileExt::unlock(&slot).unwrap();

    let unknown = "00000000000000000002";
    let output = command(&home)
        .args([
            "admission",
            "reconcile",
            "--apply",
            "--ticket-id",
            unknown,
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    let unknown_json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(unknown_json["mode"], "Apply");
    assert_eq!(unknown_json["reason_category"], "unknown_target");
    assert_eq!(
        unknown_json["outcomes"],
        serde_json::json!([{"ticket_id": unknown, "classification": "blocked"}])
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!combined.contains(home.to_str().unwrap()));
    assert!(!combined.contains("No such file"));

    let human = command(&home)
        .args(["admission", "reconcile", "--apply", "--ticket-id", unknown])
        .output()
        .unwrap();
    assert_eq!(human.status.code(), Some(4));
    let human_stdout = String::from_utf8(human.stdout).unwrap();
    assert!(human_stdout.contains("Admission reconciliation schema: 1.0"));
    assert!(human_stdout.contains("Mode: Apply"));
    assert!(human_stdout.contains("Reason category: unknown_target"));
    assert!(human_stdout.contains(&format!("  - {unknown}: blocked")));
    assert!(!human_stdout.contains(home.to_str().unwrap()));
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn over_max_queue_tickets_is_bounded_usage_error_before_platform_access() {
    let home = fixture();
    let ticket_ids: Vec<String> = (0..=MAX_QUEUE_TICKETS)
        .map(|index| format!("{index:020}"))
        .collect();
    let mut args = vec!["admission", "reconcile", "--apply"];
    for ticket_id in &ticket_ids {
        args.extend(["--ticket-id", ticket_id]);
    }
    args.push("--json");

    let output = command(&home).args(args).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("--ticket-id values exceed the maximum allowed count"));
    assert!(!stderr.contains(home.to_str().unwrap()));
    assert!(!admission_root(&home).exists());
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn partial_apply_reports_reason_and_ordered_bounded_outcomes() {
    let home = fixture();
    let root = owned_admission_root(&home);
    let selected = "00000000000000000003";
    ticket(&root, selected);
    expired_lease(&root, selected);
    fs::write(
        root.join("quarantine")
            .join(format!("ticket-{selected}.json.0")),
        b"preserved collision evidence",
    )
    .unwrap();

    let output = command(&home)
        .args([
            "admission",
            "reconcile",
            "--apply",
            "--ticket-id",
            selected,
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], "1.0");
    assert_eq!(value["mode"], "Apply");
    assert_eq!(value["reason_category"], "quarantine_collision");
    assert_eq!(
        value["outcomes"],
        serde_json::json!([{
            "ticket_id": selected,
            "classification": "partial_quarantine_collision"
        }])
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains(home.to_str().unwrap()));
    fs::remove_dir_all(home).unwrap();
}

#[cfg(unix)]
#[test]
fn reconcile_installs_cancellation_before_waiting_on_coordinator_lock() {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let home = fixture();
    let root = owned_admission_root(&home);
    let queue = File::options()
        .read(true)
        .write(true)
        .open(root.join("queue.lock"))
        .unwrap();
    queue.lock_exclusive().unwrap();
    let mut child = command(&home)
        .args([
            "admission",
            "reconcile",
            "--json",
            "--timeout-seconds",
            "10",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_secs(1));
    assert!(child.try_wait().unwrap().is_none());
    kill(Pid::from_raw(child.id() as i32), Signal::SIGINT).unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(5));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["mode"], "Preview");
    assert_eq!(value["reason_category"], "cancelled");
    assert_eq!(value["outcomes"], serde_json::json!([]));
    FileExt::unlock(&queue).unwrap();
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn reconcile_apply_shape_and_target_validation_exit_two() {
    let home = fixture();
    for args in [
        vec![
            "admission",
            "reconcile",
            "--ticket-id",
            "00000000000000000001",
        ],
        vec!["admission", "reconcile", "--apply"],
        vec!["admission", "reconcile", "--apply", "--ticket-id", "bad"],
        vec![
            "admission",
            "reconcile",
            "--apply",
            "--ticket-id",
            "00000000000000000001",
            "--ticket-id",
            "00000000000000000001",
        ],
    ] {
        let output = command(&home).args(args).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fs::remove_dir_all(home).unwrap();
}
