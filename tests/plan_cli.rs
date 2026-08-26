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

use std::process::Command;

use commit_ci_preflight::config::config_schema_json;
use commit_ci_preflight::receipt::canonical_digest;

const CONFIG: &str = "tests/fixtures/config-v1-read-only.toml";
const MATRIX_CONFIG: &str = "tests/fixtures/config-v2-matrix.toml";
const LEGACY_MATRIX_CONFIG: &str = "tests/fixtures/config-v2-legacy-compatible.toml";
const PINNED_SCHEMA: &str = include_str!("../schema/config-v1.schema.json");
const CURRENT_V2_PLAN_STDOUT: &[u8] =
    include_bytes!("fixtures/plan-v2-current-default.stdout.json");

#[test]
fn matrix_plan_profile_flag_is_exposed_only_by_configuration_commands() {
    let binary = env!("CARGO_BIN_EXE_commit-ci-preflight");
    for command in ["plan", "doctor", "dry-run", "run"] {
        let output = Command::new(binary)
            .args([command, "--help"])
            .output()
            .expect("help");
        assert!(String::from_utf8_lossy(&output.stdout).contains("--matrix-plan-profile"));
    }
    let output = Command::new(binary)
        .args(["verify", "--help"])
        .output()
        .expect("help");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("--matrix-plan-profile"));
}

#[test]
fn generated_configuration_schema_matches_pinned_bytes() {
    assert_eq!(config_schema_json().expect("config schema"), PINNED_SCHEMA);
}

#[test]
fn plan_json_is_read_only_and_machine_readable() {
    let output = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args(["plan", "--config", CONFIG, "--json"])
        .output()
        .expect("run plan command");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("plan JSON");
    assert_eq!(json["plan"]["checks"][0]["id"], "must-not-execute");
    assert_eq!(
        json["plan"]["checks"][0]["argv"][0],
        "this-command-does-not-exist-and-must-not-run"
    );
    assert!(
        json["plan_digest"]
            .as_str()
            .expect("plan digest")
            .starts_with("sha256:")
    );
}

#[test]
fn human_plan_explicitly_reports_read_only_behavior() {
    let output = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args(["plan", "--config", CONFIG])
        .output()
        .expect("run plan command");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(stdout.contains("Read-only: no command was executed."));
    assert!(stdout.contains("must-not-execute"));
}

#[test]
fn v2_plan_exposes_reviewable_per_runtime_digests_without_execution() {
    let output = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args(["plan", "--config", MATRIX_CONFIG, "--json"])
        .output()
        .expect("run matrix plan command");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("matrix plan JSON");
    assert_eq!(json["plan"]["schema_version"], "2.0");
    assert_eq!(
        json["plan"]["runtimes"].as_array().expect("runtimes").len(),
        2
    );
    for runtime in json["plan"]["runtimes"].as_array().expect("runtimes") {
        assert!(
            runtime["configuration_digest"]
                .as_str()
                .expect("configuration digest")
                .starts_with("sha256:")
        );
    }
}

#[test]
fn matrix_current_profile_and_omission_preserve_pinned_default_plan_bytes() {
    for profile in [None, Some("current-v2")] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"));
        command.args(["plan", "--config", MATRIX_CONFIG, "--json"]);
        if let Some(profile) = profile {
            command.args(["--matrix-plan-profile", profile]);
        }
        let output = command.output().expect("run current matrix plan command");

        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, CURRENT_V2_PLAN_STDOUT);
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("plan JSON");
        assert!(json.get("matrix_plan_profile").is_none());
        assert!(json.get("legacy_digest_basis").is_none());
    }
}

#[test]
fn matrix_legacy_profile_discloses_a_reconstructible_digest_basis() {
    let output = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args([
            "plan",
            "--config",
            LEGACY_MATRIX_CONFIG,
            "--matrix-plan-profile",
            "matrix-v2-legacy-v1",
            "--json",
        ])
        .output()
        .expect("run legacy matrix plan command");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("legacy plan JSON");
    assert_eq!(json["matrix_plan_profile"], "matrix-v2-legacy-v1");
    assert!(json["legacy_digest_basis"].is_object());
    assert_eq!(
        canonical_digest(&json["legacy_digest_basis"]).expect("canonical legacy digest basis"),
        json["plan_digest"].as_str().expect("legacy plan digest")
    );
    assert_eq!(json["plan"]["schema_version"], "2.0");
}

#[test]
fn legacy_matrix_profile_reproduces_the_pinned_historical_plan_digest() {
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/matrix-v2-legacy-plan-044697.json"
    ))
    .expect("historical plan fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args([
            "plan",
            "--config",
            LEGACY_MATRIX_CONFIG,
            "--matrix-plan-profile",
            "matrix-v2-legacy-v1",
            "--json",
        ])
        .output()
        .expect("run legacy matrix plan command");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual: serde_json::Value = serde_json::from_slice(&output.stdout).expect("plan JSON");
    assert_eq!(actual["plan_digest"], expected["plan_digest"]);
}

#[test]
fn matrix_plan_profile_rejects_unknown_values_with_usage_exit_code() {
    let output = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args([
            "plan",
            "--config",
            MATRIX_CONFIG,
            "--matrix-plan-profile",
            "unknown-profile",
        ])
        .output()
        .expect("run unknown matrix profile command");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown-profile"));
}

#[test]
fn legacy_matrix_profile_rejects_single_runtime_configuration_with_usage_exit_code() {
    let output = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args([
            "plan",
            "--config",
            CONFIG,
            "--matrix-plan-profile",
            "matrix-v2-legacy-v1",
        ])
        .output()
        .expect("run single-runtime legacy matrix plan command");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("matrix plan profile requires schema version 2.0")
    );
}

#[test]
fn missing_configuration_exits_with_usage_code_two() {
    let output = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args(["plan", "--config", "tests/fixtures/does-not-exist.toml"])
        .output()
        .expect("run plan command");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read configuration"));
}
