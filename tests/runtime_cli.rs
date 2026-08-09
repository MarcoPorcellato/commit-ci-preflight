// Copyright 2026 Marco Porcellato
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_commit-ci-preflight")
}

fn fixture() -> &'static Path {
    Path::new("tests/fixtures/config-v1-read-only.toml")
}

#[test]
fn dry_run_json_is_deterministic_and_never_executes_the_declared_argv() {
    let marker = std::env::temp_dir().join(format!("ccp-dry-run-marker-{}", std::process::id()));
    let _ = fs::remove_file(&marker);
    let config = fs::read_to_string(fixture()).expect("fixture");
    assert!(!config.contains(marker.to_string_lossy().as_ref()));

    let first = Command::new(binary())
        .args(["dry-run", "--config"])
        .arg(fixture())
        .arg("--json")
        .output()
        .expect("first dry-run");
    let second = Command::new(binary())
        .args(["dry-run", "--config"])
        .arg(fixture())
        .arg("--json")
        .output()
        .expect("second dry-run");

    assert!(first.status.success());
    assert_eq!(first.stdout, second.stdout);
    assert!(!marker.exists());
    let value: Value = serde_json::from_slice(&first.stdout).expect("JSON output");
    assert_eq!(value["executed"], false);
    assert_eq!(value["workspace_mount_policy"], "deferred_to_pr04");
}

#[test]
fn host_doctor_fails_with_runtime_exit_code_before_spawning() {
    let source = fs::read_to_string(fixture()).expect("fixture");
    let host = source.replace("kind = \"docker_compatible\"", "kind = \"host\"");
    let config_path = std::env::temp_dir().join(format!("ccp-host-{}.toml", std::process::id()));
    fs::write(&config_path, host).expect("host fixture");

    let output = Command::new(binary())
        .args(["doctor", "--config"])
        .arg(&config_path)
        .output()
        .expect("doctor");

    assert_eq!(output.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported runtime"));
    fs::remove_file(config_path).expect("remove owned host fixture");
}

#[test]
fn dry_run_human_output_states_that_argv_is_not_a_shell_and_was_not_run() {
    let output = Command::new(binary())
        .args(["dry-run", "--config"])
        .arg(fixture())
        .output()
        .expect("dry-run");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 output");
    assert!(stdout.contains("Argv (not shell)"));
    assert!(stdout.contains("Dry-run: no command was executed."));
}
