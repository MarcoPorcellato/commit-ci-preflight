use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_commit-ci-preflight")
}

fn command(home: &PathBuf) -> Command {
    let mut c = Command::new(bin());
    c.env("HOME", home);
    c
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
