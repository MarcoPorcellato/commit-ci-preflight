// Copyright 2026 Marco Porcellato
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use commit_ci_preflight::cache::{
    CacheRootOptions, ManagedCache, PlatformFamily, ResolvedCacheRoot,
};
use commit_ci_preflight::run_journal::{
    RecoveryClassificationV1, RunFailureKindV1, RunJournalStateV1, RunJournalStore,
};
use serde_json::Value;

const RUN_ID: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const AT: &str = "2026-08-14T12:00:00Z";
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_commit-ci-preflight")
}

struct Fixture {
    root: PathBuf,
    repository: PathBuf,
    cache: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
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
                ".ccp-recover-test-{}-{sequence}-{label}",
                std::process::id()
            ));
        let repository = root.join("repository");
        let cache = root.join("cache");
        fs::create_dir_all(&repository).expect("repository");
        let resolved = ResolvedCacheRoot::resolve(
            &repository,
            &CacheRootOptions {
                explicit: Some(cache.clone()),
                environment: None,
                home: None,
                xdg_cache_home: None,
                local_app_data: None,
                platform: PlatformFamily::current(),
            },
        )
        .expect("resolve cache");
        ManagedCache::initialize(resolved).expect("initialize cache");
        Self {
            root,
            repository,
            cache,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(binary());
        command
            .arg("recover")
            .arg("status")
            .arg("--repository")
            .arg(&self.repository)
            .arg("--cache-dir")
            .arg(&self.cache);
        command
    }

    fn location(&self, command: &mut Command) {
        command
            .arg("--repository")
            .arg(&self.repository)
            .arg("--cache-dir")
            .arg(&self.cache);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.root.exists() {
            fs::remove_dir_all(&self.root).expect("cleanup exact fixture");
        }
    }
}

#[test]
fn status_is_read_only_and_reports_restartable_state() {
    let fixture = Fixture::new("status");
    let store = RunJournalStore::initialize(&fixture.cache).expect("journal");
    store.create_run(RUN_ID, AT).expect("run");
    let before = tree_fingerprint(&fixture.cache);

    let output = fixture.command().arg("--json").output().expect("status");

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert_eq!(value["runs"][0]["run_id"], RUN_ID);
    assert_eq!(value["runs"][0]["classification"], "restartable");
    assert_eq!(before, tree_fingerprint(&fixture.cache));
}

#[test]
fn apply_quarantines_one_exact_owned_run_and_nothing_else() {
    let fixture = Fixture::new("apply");
    let store = RunJournalStore::initialize(&fixture.cache).expect("journal");
    store.create_run(RUN_ID, AT).expect("run");

    let mut command = Command::new(binary());
    command.args(["recover", "apply", RUN_ID]);
    fixture.location(&mut command);
    let output = command.arg("--json").output().expect("apply");

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert_eq!(value["run_id"], RUN_ID);
    assert_eq!(value["outcome"], "quarantined");
    let status = store.status().expect("status");
    assert_eq!(status.runs.len(), 1);
    assert_eq!(
        status.runs[0].classification,
        RecoveryClassificationV1::Quarantined
    );
}

#[test]
fn malformed_and_terminal_run_ids_fail_closed_with_stable_codes() {
    let fixture = Fixture::new("codes");
    let store = RunJournalStore::initialize(&fixture.cache).expect("journal");
    store.create_run(RUN_ID, AT).expect("run");
    store
        .transition(
            RUN_ID,
            RunJournalStateV1::Failed,
            AT,
            Some(RunFailureKindV1::Unknown),
        )
        .expect("terminal");

    let mut malformed = Command::new(binary());
    malformed.args(["recover", "apply", "../foreign"]);
    fixture.location(&mut malformed);
    assert_eq!(
        malformed.output().expect("malformed").status.code(),
        Some(2)
    );

    let mut terminal = Command::new(binary());
    terminal.args(["recover", "apply", RUN_ID]);
    fixture.location(&mut terminal);
    assert_eq!(terminal.output().expect("terminal").status.code(), Some(5));
}

fn tree_fingerprint(root: &std::path::Path) -> Vec<(PathBuf, u64)> {
    fn walk(root: &std::path::Path, path: &std::path::Path, out: &mut Vec<(PathBuf, u64)>) {
        let mut entries: Vec<_> = fs::read_dir(path)
            .expect("read directory")
            .collect::<Result<_, _>>()
            .expect("entries");
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            let metadata = entry.metadata().expect("metadata");
            out.push((
                path.strip_prefix(root).expect("relative").to_path_buf(),
                metadata.len(),
            ));
            if metadata.is_dir() {
                walk(root, &path, out);
            }
        }
    }
    let mut output = Vec::new();
    walk(root, root, &mut output);
    output
}
