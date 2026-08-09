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
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_commit-ci-preflight")
}

fn fixture() -> &'static Path {
    Path::new("tests/fixtures/config-v1-read-only.toml")
}

struct PersistentFixture {
    root: PathBuf,
    repository: PathBuf,
    cache: PathBuf,
}

impl PersistentFixture {
    fn new(label: &str) -> Self {
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::var_os("CCP_TEST_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .expect("current directory")
                    .parent()
                    .expect("repository parent")
                    .to_path_buf()
            })
            .join(format!(
                ".ccp-cli-test-{}-{sequence}-{label}",
                std::process::id()
            ));
        assert!(!root.exists(), "test root must be unique");
        let repository = root.join("repository");
        let cache = root.join("persistent-cache");
        fs::create_dir_all(&repository).expect("create repository fixture");
        Self {
            root,
            repository,
            cache,
        }
    }

    fn location_args(&self) -> [&std::ffi::OsStr; 4] {
        [
            std::ffi::OsStr::new("--repository"),
            self.repository.as_os_str(),
            std::ffi::OsStr::new("--cache-dir"),
            self.cache.as_os_str(),
        ]
    }
}

impl Drop for PersistentFixture {
    fn drop(&mut self) {
        if self.root.exists() {
            fs::remove_dir_all(&self.root).expect("remove exact owned fixture");
        }
    }
}

#[test]
fn path_and_dry_run_are_read_only_and_explicit() {
    let fixture_root = PersistentFixture::new("read-only");

    let path = Command::new(binary())
        .args(["cache", "path"])
        .args(fixture_root.location_args())
        .arg("--json")
        .output()
        .expect("cache path");
    assert!(path.status.success());
    let path_json: Value = serde_json::from_slice(&path.stdout).expect("path JSON");
    assert_eq!(path_json["source"], "explicit");
    assert!(!fixture_root.cache.exists());

    let dry_run = Command::new(binary())
        .args(["dry-run", "--config"])
        .arg(fixture())
        .args(fixture_root.location_args())
        .arg("--json")
        .output()
        .expect("dry-run");
    assert!(dry_run.status.success());
    let dry_run_json: Value = serde_json::from_slice(&dry_run.stdout).expect("dry-run JSON");
    assert_eq!(dry_run_json["executed"], false);
    assert_eq!(dry_run_json["workspace_mount_policy"], "explicit_bindings");
    assert!(!fixture_root.cache.exists());
}

#[test]
fn init_inventory_and_cleanup_dry_run_are_truthful() {
    let fixture_root = PersistentFixture::new("lifecycle");

    for _ in 0..2 {
        let init = Command::new(binary())
            .args(["cache", "init"])
            .args(fixture_root.location_args())
            .arg("--json")
            .output()
            .expect("cache init");
        assert!(init.status.success());
    }
    assert!(fixture_root.cache.join(".ccp-cache-root-v1.json").is_file());

    let inventory = Command::new(binary())
        .args(["cache", "inventory"])
        .args(fixture_root.location_args())
        .arg("--json")
        .output()
        .expect("cache inventory");
    assert!(inventory.status.success());
    let inventory_json: Value = serde_json::from_slice(&inventory.stdout).expect("inventory JSON");
    assert_eq!(inventory_json["entries"].as_array().map(Vec::len), Some(0));

    let rejected = Command::new(binary())
        .args(["cache", "cleanup"])
        .args(fixture_root.location_args())
        .output()
        .expect("cleanup without acknowledgement");
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("requires --dry-run"));

    let cleanup = Command::new(binary())
        .args(["cache", "cleanup", "--dry-run"])
        .args(fixture_root.location_args())
        .arg("--json")
        .output()
        .expect("cleanup dry-run");
    assert!(cleanup.status.success());
    let cleanup_json: Value = serde_json::from_slice(&cleanup.stdout).expect("cleanup JSON");
    assert_eq!(cleanup_json["deletion_performed"], false);
    assert!(fixture_root.cache.exists());
}

#[test]
fn environment_cache_root_is_used_only_when_explicit_is_absent() {
    let fixture_root = PersistentFixture::new("environment");
    let output = Command::new(binary())
        .args(["cache", "path", "--repository"])
        .arg(&fixture_root.repository)
        .arg("--json")
        .env("CCP_CACHE_DIR", &fixture_root.cache)
        .output()
        .expect("environment cache path");

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON output");
    assert_eq!(value["source"], "environment");
    assert!(!fixture_root.cache.exists());
}
