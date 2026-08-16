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

const CONFIG: &str = "tests/fixtures/config-v1-read-only.toml";
const MATRIX_CONFIG: &str = "tests/fixtures/config-v2-matrix.toml";
const PINNED_SCHEMA: &str = include_str!("../schema/config-v1.schema.json");

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
fn missing_configuration_exits_with_usage_code_two() {
    let output = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args(["plan", "--config", "tests/fixtures/does-not-exist.toml"])
        .output()
        .expect("run plan command");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read configuration"));
}
