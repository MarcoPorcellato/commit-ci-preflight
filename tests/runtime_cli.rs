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
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_commit-ci-preflight")
}

fn fixture() -> &'static Path {
    Path::new("tests/fixtures/config-v1-read-only.toml")
}

fn matryca_matrix_fixture() -> &'static Path {
    Path::new("tests/fixtures/config-v2-matryca-three-runtimes.toml")
}

fn executable_fixture_root(prefix: &str) -> PathBuf {
    std::env::var_os("CCP_TEST_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(format!("{prefix}-{}", std::process::id()))
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
    assert_eq!(value["workspace_mount_policy"], "explicit_bindings");
    let mounts = value["workspace"]["mounts"]
        .as_array()
        .expect("explicit mount array");
    assert_eq!(mounts[0]["access"], "read_only");
    assert_eq!(mounts[0]["purpose"], "repository");
    assert!(
        mounts
            .iter()
            .skip(1)
            .all(|mount| mount["access"] == "read_write")
    );
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
fn remote_secret_only_run_fails_before_cache_or_admission_setup() {
    let root = std::env::temp_dir().join(format!("ccp-remote-secret-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("owned fixture root");
    let config_path = root.join("remote-secret.toml");
    fs::write(
        &config_path,
        r#"
schema_version = "1.1"
project = "example/project"

[runtime]
kind = "docker_compatible"
image = "registry.example/ci@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
cpu_count = 1
memory_mib = 64
pids_limit = 1

[environment]
remote_secret_only = ["DEPLOY_TOKEN"]

[[checks]]
id = "never-run"
required = true
argv = ["false"]
working_directory = "."
timeout_seconds = 1
"#,
    )
    .expect("config fixture");
    let cache_dir = root.join("cache");

    let output = Command::new(binary())
        .args(["run", "--config"])
        .arg(&config_path)
        .args(["--repository"])
        .arg(&root)
        .args(["--cache-dir"])
        .arg(&cache_dir)
        .output()
        .expect("run");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("remote-secret-only"));
    assert!(!cache_dir.exists());
    fs::remove_dir_all(root).expect("remove owned fixture root");
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

#[test]
fn matrix_dry_run_renders_all_matryca_runtimes_without_execution() {
    let output = Command::new(binary())
        .args(["dry-run", "--config"])
        .arg(matryca_matrix_fixture())
        .arg("--json")
        .output()
        .expect("matrix dry-run");

    assert!(
        output.status.success(),
        "matrix dry-run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("matrix dry-run JSON");
    assert_eq!(value["schema_version"], "2.0");
    assert!(
        value["plan_digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("sha256:"))
    );
    let runtimes = value["runtimes"].as_array().expect("runtime array");
    assert_eq!(runtimes.len(), 3);
    assert_eq!(
        runtimes
            .iter()
            .map(|runtime| runtime["runtime_id"].as_str().expect("runtime id"))
            .collect::<Vec<_>>(),
        vec!["node22", "python312", "python313"]
    );
    for runtime in runtimes {
        assert!(
            runtime["configuration_digest"]
                .as_str()
                .is_some_and(|digest| digest.starts_with("sha256:"))
        );
        let dry_run = &runtime["dry_run"];
        assert_eq!(dry_run["executed"], false);
        assert_eq!(dry_run["workspace_mount_policy"], "explicit_bindings");
        let mounts = dry_run["workspace"]["mounts"]
            .as_array()
            .expect("explicit mounts");
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0]["access"], "read_only");
        assert_eq!(mounts[0]["purpose"], "repository");
        assert_eq!(mounts[1]["access"], "read_write");
        assert_eq!(mounts[1]["purpose"], "cache");
    }
}

#[test]
fn matrix_dry_run_human_output_labels_every_runtime() {
    let output = Command::new(binary())
        .args(["dry-run", "--config"])
        .arg(matryca_matrix_fixture())
        .output()
        .expect("matrix dry-run human output");

    assert!(
        output.status.success(),
        "matrix dry-run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 output");
    assert!(stdout.contains("Matrix plan: sha256:"));
    assert!(stdout.contains("Runtime ID: node22"));
    assert!(stdout.contains("Runtime ID: python312"));
    assert!(stdout.contains("Runtime ID: python313"));
    assert_eq!(
        stdout.matches("Dry-run: no command was executed.").count(),
        3
    );
}

#[cfg(unix)]
#[test]
fn matrix_doctor_probes_all_matryca_runtimes_with_labeled_output() {
    use std::os::unix::fs::PermissionsExt;

    let root = executable_fixture_root("ccp-matrix-doctor");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("fake docker root");
    let marker = root.join("probe-count");
    let docker = root.join("docker");
    fs::write(
        &docker,
        format!(
            "#!/bin/sh\nprintf 'probe\\n' >> '{}'\nprintf '%s\\n' '{{\"ServerVersion\":\"29.4.0\",\"OperatingSystem\":\"OrbStack\",\"OSType\":\"linux\",\"MemoryLimit\":true,\"SwapLimit\":true}}'\n",
            marker.display()
        ),
    )
    .expect("fake docker");
    let mut permissions = fs::metadata(&docker)
        .expect("fake docker metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&docker, permissions).expect("fake docker executable");

    let output = Command::new(binary())
        .args(["doctor", "--config"])
        .arg(matryca_matrix_fixture())
        .arg("--json")
        .env("PATH", &root)
        .output()
        .expect("matrix doctor");

    assert!(
        output.status.success(),
        "matrix doctor failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("matrix doctor JSON");
    assert_eq!(value["schema_version"], "2.0");
    let runtimes = value["runtimes"].as_array().expect("runtime array");
    assert_eq!(runtimes.len(), 3);
    assert_eq!(
        runtimes
            .iter()
            .map(|runtime| runtime["runtime_id"].as_str().expect("runtime id"))
            .collect::<Vec<_>>(),
        vec!["node22", "python312", "python313"]
    );
    assert!(runtimes.iter().all(|runtime| {
        runtime["configuration_digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("sha256:"))
    }));
    assert!(runtimes.iter().all(|runtime| {
        runtime["probe"]["runtime"] == "docker_compatible"
            && runtime["probe"]["flavor"] == "orb_stack"
    }));
    assert_eq!(
        fs::read_to_string(&marker)
            .expect("probe marker")
            .lines()
            .count(),
        3
    );
    fs::remove_dir_all(root).expect("remove fake docker root");
}
