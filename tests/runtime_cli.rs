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

use commit_ci_preflight::cache::{CacheKey, CacheRootSource, ResolvedCacheRoot};
use commit_ci_preflight::matrix::{MatrixConfigV2, MatrixPlanProfile, build_matrix_plan};
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

fn tree_bytes(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn visit(root: &Path, path: &Path, output: &mut Vec<(PathBuf, Vec<u8>)>) {
        let mut entries = fs::read_dir(path)
            .expect("read fixture tree")
            .collect::<Result<Vec<_>, _>>()
            .expect("fixture tree entries");
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                visit(root, &path, output);
            } else {
                output.push((
                    path.strip_prefix(root)
                        .expect("relative fixture path")
                        .to_path_buf(),
                    fs::read(&path).expect("fixture file bytes"),
                ));
            }
        }
    }

    let mut output = Vec::new();
    visit(root, root, &mut output);
    output
}

const RUNTIME_SENTINEL: &str = "ccp-legacy-profile-runtime-sentinel";

struct IsolatedLegacyRunFixture {
    root: PathBuf,
    source: PathBuf,
    cache_dir: PathBuf,
    home: PathBuf,
    xdg_cache_home: PathBuf,
    local_app_data: PathBuf,
    bin: PathBuf,
    marker: PathBuf,
    source_before: Vec<(PathBuf, Vec<u8>)>,
}

impl IsolatedLegacyRunFixture {
    fn new(prefix: &str, config: &str) -> Self {
        let root = executable_fixture_root(prefix);
        let _ = fs::remove_dir_all(&root);
        let source = root.join("source");
        let bin = root.join("bin");
        fs::create_dir_all(&source).expect("source fixture root");
        fs::create_dir_all(&bin).expect("fake runtime directory");
        fs::write(source.join("config.toml"), config).expect("write config fixture");

        let marker = root.join("runtime-marker");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let docker = bin.join("docker");
            fs::write(
                &docker,
                format!(
                    "#!/bin/sh\nprintf runtime > '{}'\nprintf '%s\\n' '{}' >&2\nexit 97\n",
                    marker.display(),
                    RUNTIME_SENTINEL,
                ),
            )
            .expect("fake docker");
            let mut permissions = fs::metadata(&docker)
                .expect("fake docker metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&docker, permissions).expect("fake docker executable");
        }
        #[cfg(windows)]
        fs::write(
            bin.join("docker.cmd"),
            format!(
                "@echo runtime> \"{}\"\r\n@echo {RUNTIME_SENTINEL} 1>&2\r\n@exit /b 97\r\n",
                marker.display()
            ),
        )
        .expect("fake docker command");

        let source_before = tree_bytes(&source);
        Self {
            cache_dir: root.join("cache"),
            home: root.join("home"),
            xdg_cache_home: root.join("xdg-cache"),
            local_app_data: root.join("local-app-data"),
            root,
            source,
            bin,
            marker,
            source_before,
        }
    }

    fn run(&self) -> std::process::Output {
        Command::new(binary())
            .args(["run", "--config"])
            .arg(self.source.join("config.toml"))
            .args([
                "--matrix-plan-profile",
                "matrix-v2-legacy-v1",
                "--repository",
            ])
            .arg(&self.source)
            .args(["--cache-dir"])
            .arg(&self.cache_dir)
            .env("HOME", &self.home)
            .env("XDG_CACHE_HOME", &self.xdg_cache_home)
            .env("LOCALAPPDATA", &self.local_app_data)
            .env("PATH", &self.bin)
            .output()
            .expect("legacy misuse run")
    }

    fn assert_rejected_before_shared_state(&self, output: std::process::Output, field: &str) {
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(field),
            "stderr did not name {field}: {stderr}"
        );
        assert!(
            !stderr.contains(RUNTIME_SENTINEL),
            "runtime sentinel reached stderr: {stderr}"
        );
        assert!(!self.cache_dir.exists(), "run initialized the cache root");
        assert!(
            !self.cache_dir.join("run-journal-v1").exists(),
            "run initialized the journal"
        );
        for admission_root in [
            self.home
                .join("Library")
                .join("Caches")
                .join("commit-ci-preflight-admission"),
            self.xdg_cache_home.join("commit-ci-preflight-admission"),
            self.local_app_data.join("commit-ci-preflight-admission"),
        ] {
            assert!(
                !admission_root.exists(),
                "run initialized admission state at {}",
                admission_root.display()
            );
        }
        assert!(!self.home.exists(), "run created HOME state");
        assert!(!self.xdg_cache_home.exists(), "run created XDG cache state");
        assert!(
            !self.local_app_data.exists(),
            "run created local app-data state"
        );
        assert!(
            !self.source.join(".ccp/receipt.json").exists(),
            "run wrote a receipt"
        );
        assert!(!self.marker.exists(), "run constructed the Docker runtime");
        assert_eq!(
            tree_bytes(&self.source),
            self.source_before,
            "run mutated the source tree"
        );
    }

    fn cleanup(self) {
        fs::remove_dir_all(self.root).expect("remove owned fixture root");
    }
}

fn assert_legacy_run_rejected_before_shared_state(prefix: &str, config: &str, field: &str) {
    let fixture = IsolatedLegacyRunFixture::new(prefix, config);
    fixture.assert_rejected_before_shared_state(fixture.run(), field);
    fixture.cleanup();
}

#[test]
fn legacy_profile_uses_distinct_plan_cache_identity() {
    let legacy = build_matrix_plan(
        MatrixConfigV2::parse(include_str!("fixtures/config-v2-legacy-compatible.toml"))
            .expect("parse legacy fixture"),
        MatrixPlanProfile::LegacyV1,
    )
    .expect("legacy plan");
    let current = build_matrix_plan(
        MatrixConfigV2::parse(include_str!("fixtures/config-v2-legacy-compatible.toml"))
            .expect("parse current fixture"),
        MatrixPlanProfile::CurrentV2,
    )
    .expect("current plan");
    let legacy_runtime = legacy
        .runtime_envelopes()
        .expect("legacy envelopes")
        .into_iter()
        .find(|(id, _)| id == "python311")
        .expect("legacy python311")
        .1;
    let current_runtime = current
        .runtime_envelopes()
        .expect("current envelopes")
        .into_iter()
        .find(|(id, _)| id == "python311")
        .expect("current python311")
        .1;

    let cache = ResolvedCacheRoot {
        path: PathBuf::from("/owned-test-cache-root"),
        source: CacheRootSource::Explicit,
    };

    let legacy_key = CacheKey::for_plan_cache(&legacy_runtime, &legacy_runtime.plan.caches[0])
        .expect("legacy cache key");
    let current_key = CacheKey::for_plan_cache(&current_runtime, &current_runtime.plan.caches[0])
        .expect("current cache key");
    assert_ne!(legacy_runtime.plan_digest, current_runtime.plan_digest);
    assert_ne!(
        cache
            .workspace_path(&legacy_runtime.plan_digest)
            .expect("legacy workspace path"),
        cache
            .workspace_path(&current_runtime.plan_digest)
            .expect("current workspace path")
    );
    assert_ne!(legacy_key.directory_name(), current_key.directory_name());
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
fn legacy_profile_rejection_precedes_shared_state() {
    let single_runtime = fs::read_to_string(fixture()).expect("v1 fixture");
    for schema_version in ["1.0", "1.1", "1.2", "1.3"] {
        let config = single_runtime.replace(
            "schema_version = \"1.0\"",
            &format!("schema_version = \"{schema_version}\""),
        );
        assert_legacy_run_rejected_before_shared_state(
            &format!("ccp-legacy-profile-v1-{schema_version}"),
            &config,
            "matrix plan profile requires schema version 2.0",
        );
    }
}

#[test]
fn legacy_profile_rejects_current_only_matrix_syntax_before_shared_state() {
    let matrix = include_str!("fixtures/config-v2-legacy-compatible.toml");
    let cases = [
        (
            "runtime-pull-policy",
            matrix.replacen(
                "network = false",
                "network = false\npull_policy = \"never\"",
                1,
            ),
            "pull_policy",
        ),
        (
            "runtime-swap-mode",
            matrix.replacen(
                "network = false",
                "network = false\nswap_mode = \"disabled\"",
                1,
            ),
            "swap_mode",
        ),
        (
            "environment-fixed",
            matrix.replacen(
                "allow = [\"SOURCE_DATE_EPOCH\"]",
                "allow = [\"SOURCE_DATE_EPOCH\"]\nfixed = { FIXED = \"value\" }",
                1,
            ),
            "fixed",
        ),
        (
            "environment-runtime-internal",
            matrix.replacen(
                "allow = [\"SOURCE_DATE_EPOCH\"]",
                "allow = [\"SOURCE_DATE_EPOCH\"]\nruntime_internal = []",
                1,
            ),
            "runtime_internal",
        ),
        (
            "environment-remote-secret-only",
            matrix.replacen(
                "allow = [\"SOURCE_DATE_EPOCH\"]",
                "allow = [\"SOURCE_DATE_EPOCH\"]\nremote_secret_only = []",
                1,
            ),
            "remote_secret_only",
        ),
        (
            "check-artifact-contracts",
            matrix.replacen(
                "timeout_seconds = 30",
                "timeout_seconds = 30\nartifact_contracts = []",
                1,
            ),
            "artifact_contracts",
        ),
    ];

    for (name, config, field) in cases {
        assert_legacy_run_rejected_before_shared_state(
            &format!("ccp-legacy-profile-{name}"),
            &config,
            field,
        );
    }
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
