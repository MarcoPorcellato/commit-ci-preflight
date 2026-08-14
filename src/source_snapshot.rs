// Copyright 2026 Marco Porcellato
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::ExecutionPlanEnvelopeV1;
use crate::process::{
    CancellationToken, CleanupStatus, GenerationGuard, ProcessRequest, ProcessTermination,
    RunIdentity, SupervisorPort,
};
use crate::receipt::canonical_digest;

pub const SOURCE_SNAPSHOT_SCHEMA_VERSION: &str = "1.0";
const GIT_TIMEOUT: Duration = Duration::from_secs(30);
const TREE_CAPTURE_LIMIT: usize = 16 * 1024 * 1024;
const BLOB_CAPTURE_LIMIT: usize = 128 * 1024 * 1024;
const MAX_ENTRIES: usize = 50_000;
const LFS_HEADER: &[u8] = b"version https://git-lfs.github.com/spec/v1\n";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceManifestV1 {
    pub schema_version: String,
    pub commit_sha: String,
    pub strategy: String,
    pub entries: Vec<SourceManifestEntryV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceManifestEntryV1 {
    pub path: String,
    pub mode: String,
    pub kind: SourceEntryKindV1,
    pub blob_oid: Option<String>,
    pub submodule_oid: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceEntryKindV1 {
    Regular,
    Executable,
    Symlink,
    Submodule,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceSnapshotEvidenceV1 {
    pub schema_version: String,
    pub strategy: String,
    pub manifest_digest: String,
    pub entry_count: u64,
}

#[derive(Debug)]
pub struct SourceSnapshot {
    root: PathBuf,
    resource_root: PathBuf,
    repository: PathBuf,
    manifest: SourceManifestV1,
    evidence: SourceSnapshotEvidenceV1,
    cleaned: bool,
    overlay_files: BTreeSet<String>,
}

impl SourceSnapshot {
    pub fn materialize(
        repository: &Path,
        commit_sha: &str,
        resource_root: &Path,
        supervisor: &dyn SupervisorPort,
        cancellation: &CancellationToken,
        generation: &GenerationGuard,
        identity: &RunIdentity,
    ) -> Result<Self, SourceSnapshotError> {
        validate_commit(commit_sha)?;
        if fs::symlink_metadata(resource_root).is_ok() {
            return Err(SourceSnapshotError::DestinationExists);
        }
        fs::create_dir(resource_root).map_err(SourceSnapshotError::Io)?;
        let root = resource_root.join("source");
        if let Err(error) = fs::create_dir(&root) {
            let _ = fs::remove_dir(resource_root);
            return Err(SourceSnapshotError::Io(error));
        }

        let result = (|| {
            let tree = execute_git(
                repository,
                &["ls-tree", "--full-tree", "-r", "-z", commit_sha],
                TREE_CAPTURE_LIMIT,
                supervisor,
                cancellation,
                generation,
                identity,
            )?;
            let entries = parse_tree(&tree)?;
            for entry in &entries {
                match entry.kind {
                    SourceEntryKindV1::Submodule => {
                        return Err(SourceSnapshotError::UnsupportedSubmodule(
                            entry.path.clone(),
                        ));
                    }
                    SourceEntryKindV1::Symlink => {
                        return Err(SourceSnapshotError::UnsupportedSymlink(entry.path.clone()));
                    }
                    SourceEntryKindV1::Regular | SourceEntryKindV1::Executable => {}
                }
                let oid = entry
                    .blob_oid
                    .as_deref()
                    .ok_or(SourceSnapshotError::InvalidTree)?;
                let bytes = execute_git(
                    repository,
                    &["cat-file", "blob", oid],
                    BLOB_CAPTURE_LIMIT,
                    supervisor,
                    cancellation,
                    generation,
                    identity,
                )?;
                if bytes.starts_with(LFS_HEADER) {
                    return Err(SourceSnapshotError::UnsupportedLfs(entry.path.clone()));
                }
                write_entry(&root, entry, &bytes)?;
            }
            let manifest = SourceManifestV1 {
                schema_version: SOURCE_SNAPSHOT_SCHEMA_VERSION.to_owned(),
                commit_sha: commit_sha.to_owned(),
                strategy: "git-object-materialization-v1".to_owned(),
                entries,
            };
            let manifest_digest = canonical_digest(&manifest)
                .map_err(|_| SourceSnapshotError::ManifestSerialization)?;
            let evidence = SourceSnapshotEvidenceV1 {
                schema_version: SOURCE_SNAPSHOT_SCHEMA_VERSION.to_owned(),
                strategy: manifest.strategy.clone(),
                manifest_digest,
                entry_count: u64::try_from(manifest.entries.len())
                    .map_err(|_| SourceSnapshotError::TooManyEntries)?,
            };
            let mut bytes = serde_json::to_vec(&manifest)
                .map_err(|_| SourceSnapshotError::ManifestSerialization)?;
            bytes.push(b'\n');
            write_new_file(&resource_root.join("manifest-v1.json"), &bytes)?;
            Ok((manifest, evidence))
        })();

        let (manifest, evidence) = match result {
            Ok(value) => value,
            Err(error) => {
                let _ = fs::remove_dir_all(resource_root);
                return Err(error);
            }
        };
        Ok(Self {
            root,
            resource_root: resource_root.to_path_buf(),
            repository: repository.to_path_buf(),
            manifest,
            evidence,
            cleaned: false,
            overlay_files: BTreeSet::new(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn commit_sha(&self) -> &str {
        &self.manifest.commit_sha
    }

    pub fn evidence(&self) -> &SourceSnapshotEvidenceV1 {
        &self.evidence
    }

    /// Create only the empty destinations required for nested writable mounts.
    /// They are an execution overlay, not source evidence, and are excluded from
    /// byte-identity checks while every tracked source entry remains verified.
    pub fn prepare_mount_overlay(
        &mut self,
        envelope: &ExecutionPlanEnvelopeV1,
    ) -> Result<(), SourceSnapshotError> {
        for cache in &envelope.plan.caches {
            let target = self.root.join(&cache.mount_path);
            if !target.exists() {
                fs::create_dir_all(&target).map_err(SourceSnapshotError::Io)?;
            }
        }
        for check in &envelope.plan.checks {
            for artifact in &check.artifacts {
                let target = self.root.join(artifact);
                if !target.exists() {
                    let parent = target.parent().ok_or(SourceSnapshotError::InvalidPath)?;
                    fs::create_dir_all(parent).map_err(SourceSnapshotError::Io)?;
                    write_new_file(&target, &[])?;
                    self.overlay_files.insert(artifact.clone());
                }
            }
        }
        Ok(())
    }

    pub fn revalidate(
        &self,
        supervisor: &dyn SupervisorPort,
        cancellation: &CancellationToken,
        generation: &GenerationGuard,
        identity: &RunIdentity,
    ) -> Result<(), SourceSnapshotError> {
        let mut expected_paths: BTreeSet<_> = self
            .manifest
            .entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        expected_paths.extend(self.overlay_files.iter().cloned());
        if collect_regular_files(&self.root)? != expected_paths {
            return Err(SourceSnapshotError::SnapshotChanged);
        }
        for entry in &self.manifest.entries {
            let absolute = self.root.join(&entry.path);
            validate_entry_mode(&absolute, entry)?;
            let absolute_text = absolute.to_str().ok_or(SourceSnapshotError::InvalidPath)?;
            let output = execute_git(
                &self.repository,
                &["hash-object", "--no-filters", "--", absolute_text],
                256,
                supervisor,
                cancellation,
                generation,
                identity,
            )?;
            let actual = std::str::from_utf8(&output)
                .map_err(|_| SourceSnapshotError::GitFailure)?
                .trim();
            if entry.blob_oid.as_deref() != Some(actual) {
                return Err(SourceSnapshotError::SnapshotChanged);
            }
        }
        let digest = canonical_digest(&self.manifest)
            .map_err(|_| SourceSnapshotError::ManifestSerialization)?;
        if digest != self.evidence.manifest_digest {
            return Err(SourceSnapshotError::SnapshotChanged);
        }
        Ok(())
    }

    pub fn cleanup(&mut self) -> Result<(), SourceSnapshotError> {
        fs::remove_dir_all(&self.resource_root).map_err(SourceSnapshotError::Io)?;
        self.cleaned = true;
        Ok(())
    }
}

impl Drop for SourceSnapshot {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = fs::remove_dir_all(&self.resource_root);
        }
    }
}

pub fn resolve_clean_head(
    repository: &Path,
    receipt_output: &str,
    supervisor: &dyn SupervisorPort,
    cancellation: &CancellationToken,
    generation: &GenerationGuard,
    identity: &RunIdentity,
) -> Result<String, SourceSnapshotError> {
    let output = execute_git(
        repository,
        &["rev-parse", "--verify", "HEAD"],
        256,
        supervisor,
        cancellation,
        generation,
        identity,
    )?;
    let commit = std::str::from_utf8(&output)
        .map_err(|_| SourceSnapshotError::InvalidCommit)?
        .trim();
    validate_commit(commit)?;
    let exclusion = format!(":(exclude){receipt_output}");
    let status = execute_git(
        repository,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--",
            ".",
            &exclusion,
        ],
        TREE_CAPTURE_LIMIT,
        supervisor,
        cancellation,
        generation,
        identity,
    )?;
    if !status.is_empty() {
        return Err(SourceSnapshotError::DirtyRepository);
    }
    Ok(commit.to_owned())
}

fn parse_tree(bytes: &[u8]) -> Result<Vec<SourceManifestEntryV1>, SourceSnapshotError> {
    let mut entries = Vec::new();
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        if entries.len() >= MAX_ENTRIES {
            return Err(SourceSnapshotError::TooManyEntries);
        }
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or(SourceSnapshotError::InvalidTree)?;
        let header =
            std::str::from_utf8(&record[..tab]).map_err(|_| SourceSnapshotError::InvalidTree)?;
        let path = std::str::from_utf8(&record[tab + 1..])
            .map_err(|_| SourceSnapshotError::InvalidPath)?;
        validate_path(path)?;
        let mut fields = header.split(' ');
        let mode = fields.next().ok_or(SourceSnapshotError::InvalidTree)?;
        let object_type = fields.next().ok_or(SourceSnapshotError::InvalidTree)?;
        let oid = fields.next().ok_or(SourceSnapshotError::InvalidTree)?;
        if fields.next().is_some() || !valid_oid(oid) {
            return Err(SourceSnapshotError::InvalidTree);
        }
        let (kind, blob_oid, submodule_oid) = match (mode, object_type) {
            ("100644", "blob") => (SourceEntryKindV1::Regular, Some(oid.to_owned()), None),
            ("100755", "blob") => (SourceEntryKindV1::Executable, Some(oid.to_owned()), None),
            ("120000", "blob") => (SourceEntryKindV1::Symlink, Some(oid.to_owned()), None),
            ("160000", "commit") => (SourceEntryKindV1::Submodule, None, Some(oid.to_owned())),
            _ => return Err(SourceSnapshotError::UnsupportedMode(mode.to_owned())),
        };
        entries.push(SourceManifestEntryV1 {
            path: path.to_owned(),
            mode: mode.to_owned(),
            kind,
            blob_oid,
            submodule_oid,
        });
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

fn write_entry(
    root: &Path,
    entry: &SourceManifestEntryV1,
    bytes: &[u8],
) -> Result<(), SourceSnapshotError> {
    let path = root.join(&entry.path);
    let parent = path.parent().ok_or(SourceSnapshotError::InvalidPath)?;
    fs::create_dir_all(parent).map_err(SourceSnapshotError::Io)?;
    write_new_file(&path, bytes)?;
    set_entry_mode(&path, entry)
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), SourceSnapshotError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(SourceSnapshotError::Io)?;
    file.write_all(bytes).map_err(SourceSnapshotError::Io)?;
    file.sync_all().map_err(SourceSnapshotError::Io)
}

#[cfg(unix)]
fn set_entry_mode(path: &Path, entry: &SourceManifestEntryV1) -> Result<(), SourceSnapshotError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if entry.kind == SourceEntryKindV1::Executable {
        0o755
    } else {
        0o644
    };
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(SourceSnapshotError::Io)
}

#[cfg(not(unix))]
fn set_entry_mode(_path: &Path, entry: &SourceManifestEntryV1) -> Result<(), SourceSnapshotError> {
    if entry.kind == SourceEntryKindV1::Executable {
        Err(SourceSnapshotError::UnsupportedExecutableMode)
    } else {
        Ok(())
    }
}

fn validate_entry_mode(
    path: &Path,
    entry: &SourceManifestEntryV1,
) -> Result<(), SourceSnapshotError> {
    let metadata = fs::symlink_metadata(path).map_err(SourceSnapshotError::Io)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(SourceSnapshotError::SnapshotChanged);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let executable = metadata.permissions().mode() & 0o111 != 0;
        if executable != (entry.kind == SourceEntryKindV1::Executable) {
            return Err(SourceSnapshotError::SnapshotChanged);
        }
    }
    Ok(())
}

fn collect_regular_files(root: &Path) -> Result<BTreeSet<String>, SourceSnapshotError> {
    let mut pending = vec![root.to_path_buf()];
    let mut output = BTreeSet::new();
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current).map_err(SourceSnapshotError::Io)? {
            let entry = entry.map_err(SourceSnapshotError::Io)?;
            let metadata = fs::symlink_metadata(entry.path()).map_err(SourceSnapshotError::Io)?;
            if metadata.file_type().is_symlink() {
                return Err(SourceSnapshotError::SnapshotChanged);
            }
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                output.insert(
                    entry
                        .path()
                        .strip_prefix(root)
                        .map_err(|_| SourceSnapshotError::InvalidPath)?
                        .to_str()
                        .ok_or(SourceSnapshotError::InvalidPath)?
                        .to_owned(),
                );
            } else {
                return Err(SourceSnapshotError::SnapshotChanged);
            }
        }
    }
    Ok(output)
}

fn execute_git(
    repository: &Path,
    argv: &[&str],
    max_capture_bytes: usize,
    supervisor: &dyn SupervisorPort,
    cancellation: &CancellationToken,
    generation: &GenerationGuard,
    identity: &RunIdentity,
) -> Result<Vec<u8>, SourceSnapshotError> {
    let request = ProcessRequest {
        identity: identity.clone(),
        program: OsString::from("git"),
        argv: argv.iter().map(OsString::from).collect(),
        current_dir: repository.to_path_buf(),
        environment: git_environment(),
        timeout: GIT_TIMEOUT,
        max_capture_bytes,
    };
    let result = supervisor
        .execute(&request, cancellation, generation)
        .map_err(|_| SourceSnapshotError::GitFailure)?;
    if result.termination != ProcessTermination::Completed
        || result.cleanup != CleanupStatus::Verified
        || result.exit.map(|status| status.success) != Some(true)
        || result.stdout.truncated
        || result.stderr.truncated
    {
        return Err(SourceSnapshotError::GitFailure);
    }
    Ok(result.stdout.bytes)
}

fn git_environment() -> BTreeMap<OsString, OsString> {
    [
        "PATH",
        "HOME",
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "SYSTEMROOT",
        "XDG_CONFIG_HOME",
    ]
    .into_iter()
    .filter_map(|name| std::env::var_os(name).map(|value| (OsString::from(name), value)))
    .collect()
}

fn validate_path(value: &str) -> Result<(), SourceSnapshotError> {
    if value.is_empty()
        || value.bytes().any(|byte| byte.is_ascii_control())
        || Path::new(value).is_absolute()
        || Path::new(value)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        Err(SourceSnapshotError::InvalidPath)
    } else {
        Ok(())
    }
}

fn validate_commit(value: &str) -> Result<(), SourceSnapshotError> {
    if valid_oid(value) {
        Ok(())
    } else {
        Err(SourceSnapshotError::InvalidCommit)
    }
}

fn valid_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Debug)]
pub enum SourceSnapshotError {
    Io(io::Error),
    InvalidCommit,
    InvalidPath,
    InvalidTree,
    TooManyEntries,
    DestinationExists,
    GitFailure,
    UnsupportedMode(String),
    UnsupportedSubmodule(String),
    UnsupportedSymlink(String),
    UnsupportedLfs(String),
    UnsupportedExecutableMode,
    ManifestSerialization,
    SnapshotChanged,
    DirtyRepository,
}

impl fmt::Display for SourceSnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "source snapshot I/O failed: {error}"),
            Self::InvalidCommit => formatter.write_str("invalid source commit identity"),
            Self::InvalidPath => formatter.write_str("source tree contains an unsafe path"),
            Self::InvalidTree => formatter.write_str("Git source tree response is invalid"),
            Self::TooManyEntries => formatter.write_str("source tree exceeds the entry limit"),
            Self::DestinationExists => formatter.write_str("source snapshot destination exists"),
            Self::GitFailure => formatter.write_str("bounded Git snapshot command failed"),
            Self::UnsupportedMode(mode) => write!(formatter, "unsupported Git mode: {mode}"),
            Self::UnsupportedSubmodule(path) => {
                write!(formatter, "submodule is unsupported: {path}")
            }
            Self::UnsupportedSymlink(path) => write!(formatter, "symlink is unsupported: {path}"),
            Self::UnsupportedLfs(path) => {
                write!(formatter, "Git LFS pointer is unsupported: {path}")
            }
            Self::UnsupportedExecutableMode => {
                formatter.write_str("executable Git mode is unsupported on this host")
            }
            Self::ManifestSerialization => {
                formatter.write_str("source manifest serialization failed")
            }
            Self::SnapshotChanged => {
                formatter.write_str("source snapshot changed after materialization")
            }
            Self::DirtyRepository => formatter.write_str("source repository is dirty"),
        }
    }
}

impl std::error::Error for SourceSnapshotError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::process::{CapturedStream, ExitOutcome, ProcessError, ProcessResult};

    static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    const COMMIT: &str = "1111111111111111111111111111111111111111";
    const OID: &str = "2222222222222222222222222222222222222222";

    struct FakeGit;

    impl SupervisorPort for FakeGit {
        fn execute(
            &self,
            request: &ProcessRequest,
            _cancellation: &CancellationToken,
            _generation: &GenerationGuard,
        ) -> Result<ProcessResult, ProcessError> {
            let argv: Vec<_> = request
                .argv
                .iter()
                .map(|part| part.to_string_lossy())
                .collect();
            let stdout = if argv.first().is_some_and(|part| part == "ls-tree") {
                format!("100644 blob {OID}\tREADME.md\0").into_bytes()
            } else if argv.first().is_some_and(|part| part == "cat-file") {
                b"hello\n".to_vec()
            } else if argv.first().is_some_and(|part| part == "hash-object") {
                format!("{OID}\n").into_bytes()
            } else {
                Vec::new()
            };
            Ok(ProcessResult {
                identity: request.identity.clone(),
                termination: ProcessTermination::Completed,
                cleanup: CleanupStatus::Verified,
                exit: Some(ExitOutcome {
                    success: true,
                    code: Some(0),
                }),
                stdout: CapturedStream {
                    bytes: stdout,
                    truncated: false,
                },
                stderr: CapturedStream {
                    bytes: Vec::new(),
                    truncated: false,
                },
                elapsed_millis: 1,
            })
        }
    }

    fn fixture_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ccp-source-snapshot-{label}-{}-{}",
            std::process::id(),
            FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn identity() -> RunIdentity {
        RunIdentity {
            project: "fixture".to_owned(),
            commit: Some(COMMIT.to_owned()),
            config_digest: format!("sha256:{}", "3".repeat(64)),
            generation: "1".to_owned(),
        }
    }

    #[test]
    fn materialization_is_manifest_bound_and_revalidates() {
        let root = fixture_root("pass");
        fs::create_dir(&root).expect("fixture root");
        let resource = root.join("resource");
        let identity = identity();
        let generation = GenerationGuard::new(identity.clone());
        let mut snapshot = SourceSnapshot::materialize(
            &root,
            COMMIT,
            &resource,
            &FakeGit,
            &CancellationToken::default(),
            &generation,
            &identity,
        )
        .expect("snapshot");

        assert_eq!(
            fs::read(snapshot.root().join("README.md")).unwrap(),
            b"hello\n"
        );
        assert_eq!(snapshot.evidence().entry_count, 1);
        snapshot
            .revalidate(
                &FakeGit,
                &CancellationToken::default(),
                &generation,
                &identity,
            )
            .expect("revalidate");
        snapshot.cleanup().expect("cleanup");
        fs::remove_dir(root).expect("remove root");
    }

    #[test]
    fn manifest_rejects_submodules_symlinks_and_unsafe_paths() {
        for record in [
            format!("160000 commit {OID}\tdep\0"),
            format!("120000 blob {OID}\tlink\0"),
            format!("100644 blob {OID}\t../escape\0"),
        ] {
            let parsed = parse_tree(record.as_bytes());
            if record.contains("../") {
                assert!(matches!(parsed, Err(SourceSnapshotError::InvalidPath)));
            } else {
                let entry = &parsed.expect("tree syntax")[0];
                assert!(matches!(
                    entry.kind,
                    SourceEntryKindV1::Submodule | SourceEntryKindV1::Symlink
                ));
            }
        }
    }

    #[test]
    fn canonical_manifest_digest_is_order_independent_after_parsing() {
        let first = format!(
            "100644 blob {OID}\tz.txt\0100644 blob {}\ta.txt\0",
            "4".repeat(40)
        );
        let second = format!(
            "100644 blob {}\ta.txt\0100644 blob {OID}\tz.txt\0",
            "4".repeat(40)
        );
        assert_eq!(
            parse_tree(first.as_bytes()).unwrap(),
            parse_tree(second.as_bytes()).unwrap()
        );
    }
}
