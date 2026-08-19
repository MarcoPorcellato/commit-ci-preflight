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

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

use crate::cache::{CacheError, CacheKey, ManagedCache, ResolvedCacheRoot};
use crate::config::ExecutionPlanEnvelopeV1;

const CONTAINER_WORKSPACE: &str = "/workspace";
const RUN_LOCK_FILE: &str = ".run-lock-v1";
static RUN_LOCK_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MountAccess {
    ReadOnly,
    ReadWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MountPurpose {
    Repository,
    Cache,
    Artifact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MountBinding {
    pub source: PathBuf,
    pub target: String,
    pub access: MountAccess,
    pub purpose: MountPurpose,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspacePlanV1 {
    pub schema_version: &'static str,
    pub repository: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_snapshot_digest: Option<String>,
    pub run_root: PathBuf,
    pub mounts: Vec<MountBinding>,
}

impl WorkspacePlanV1 {
    pub fn build(
        envelope: &ExecutionPlanEnvelopeV1,
        repository: &Path,
        cache: &ResolvedCacheRoot,
    ) -> Result<Self, WorkspaceError> {
        Self::build_with_cache_sources(envelope, repository, cache, &BTreeMap::new(), None)
    }

    fn build_with_cache_sources(
        envelope: &ExecutionPlanEnvelopeV1,
        repository: &Path,
        cache: &ResolvedCacheRoot,
        cache_sources: &BTreeMap<String, PathBuf>,
        source_snapshot_digest: Option<&str>,
    ) -> Result<Self, WorkspaceError> {
        let repository = fs::canonicalize(repository).map_err(WorkspaceError::Io)?;
        if !repository.is_dir() {
            return Err(WorkspaceError::RepositoryNotDirectory);
        }
        validate_host_path(&repository)?;
        let run_root = cache
            .workspace_path(&envelope.plan_digest)
            .map_err(WorkspaceError::Cache)?;
        validate_under_root(&run_root, &cache.path)?;

        let mut mounts = vec![MountBinding {
            source: repository.clone(),
            target: CONTAINER_WORKSPACE.to_owned(),
            access: MountAccess::ReadOnly,
            purpose: MountPurpose::Repository,
            logical_id: None,
        }];
        let mut targets = BTreeSet::from([CONTAINER_WORKSPACE.to_owned()]);

        for declared in &envelope.plan.caches {
            let key =
                CacheKey::for_plan_cache(envelope, declared).map_err(WorkspaceError::Cache)?;
            let source = cache_sources
                .get(&declared.id)
                .cloned()
                .unwrap_or_else(|| cache.entry_data_path(&key));
            validate_under_root(&source, &cache.path)?;
            let target = container_target(&declared.mount_path)?;
            if !targets.insert(target.clone()) {
                return Err(WorkspaceError::DuplicateTarget(target));
            }
            mounts.push(MountBinding {
                source,
                target,
                access: MountAccess::ReadWrite,
                purpose: MountPurpose::Cache,
                logical_id: Some(declared.id.clone()),
            });
        }

        for check in &envelope.plan.checks {
            for artifact in &check.artifacts {
                let source = run_root.join("artifacts").join(artifact);
                validate_under_root(&source, &run_root)?;
                let target = container_target(artifact)?;
                if !targets.insert(target.clone()) {
                    return Err(WorkspaceError::DuplicateTarget(target));
                }
                mounts.push(MountBinding {
                    source,
                    target,
                    access: MountAccess::ReadWrite,
                    purpose: MountPurpose::Artifact,
                    logical_id: Some(format!("{}:{artifact}", check.id)),
                });
            }
        }

        Ok(Self {
            schema_version: "1.0",
            repository,
            source_snapshot_digest: source_snapshot_digest.map(str::to_owned),
            run_root,
            mounts,
        })
    }
}

pub struct PreparedWorkspace {
    pub plan: WorkspacePlanV1,
    cache_entries: Vec<crate::cache::PreparedCacheEntry>,
    lock: WorkspaceLock,
}

impl PreparedWorkspace {
    pub fn prepare(
        envelope: &ExecutionPlanEnvelopeV1,
        repository: &Path,
        cache: &ManagedCache,
    ) -> Result<Self, WorkspaceError> {
        Self::prepare_with_generation(envelope, repository, cache, 0)
    }

    pub fn prepare_with_generation(
        envelope: &ExecutionPlanEnvelopeV1,
        repository: &Path,
        cache: &ManagedCache,
        generation: u64,
    ) -> Result<Self, WorkspaceError> {
        validate_repository_mount_targets(envelope, repository)?;
        let run_root = cache
            .workspace_path(&envelope.plan_digest)
            .map_err(WorkspaceError::Cache)?;
        create_managed_directory_chain(&run_root, &cache.root().path)?;
        let lock = WorkspaceLock::acquire(&run_root)?;

        let mut cache_sources = BTreeMap::new();
        let mut cache_entries = Vec::new();
        for declared in &envelope.plan.caches {
            let key =
                CacheKey::for_plan_cache(envelope, declared).map_err(WorkspaceError::Cache)?;
            let prepared = cache
                .prepare_entry(&key, &envelope.plan_digest, generation)
                .map_err(WorkspaceError::Cache)?;
            validate_under_root(&prepared.data_path, &cache.root().path)?;
            cache_sources.insert(declared.id.clone(), prepared.data_path.clone());
            cache_entries.push(prepared);
        }

        for check in &envelope.plan.checks {
            for artifact in &check.artifacts {
                prepare_artifact_file(&run_root, artifact)?;
            }
        }

        let plan = WorkspacePlanV1::build_with_cache_sources(
            envelope,
            repository,
            cache.root(),
            &cache_sources,
            None,
        )?;
        Ok(Self {
            plan,
            cache_entries,
            lock,
        })
    }

    pub fn prepare_snapshot(
        envelope: &ExecutionPlanEnvelopeV1,
        snapshot_root: &Path,
        source_snapshot_digest: &str,
        cache: &ManagedCache,
    ) -> Result<Self, WorkspaceError> {
        Self::prepare_snapshot_with_generation(
            envelope,
            snapshot_root,
            source_snapshot_digest,
            cache,
            0,
        )
    }

    pub fn prepare_snapshot_with_generation(
        envelope: &ExecutionPlanEnvelopeV1,
        snapshot_root: &Path,
        source_snapshot_digest: &str,
        cache: &ManagedCache,
        generation: u64,
    ) -> Result<Self, WorkspaceError> {
        let mut prepared =
            Self::prepare_with_generation(envelope, snapshot_root, cache, generation)?;
        prepared.plan.source_snapshot_digest = Some(source_snapshot_digest.to_owned());
        Ok(prepared)
    }

    pub fn mark_caches_complete(&self, cache: &ManagedCache) -> Result<(), WorkspaceError> {
        cache
            .promote_entries(&self.cache_entries)
            .map_err(WorkspaceError::Cache)
    }

    pub fn lock_path(&self) -> &Path {
        &self.lock.path
    }
}

struct WorkspaceLock {
    path: PathBuf,
    owner: Vec<u8>,
}

impl WorkspaceLock {
    fn acquire(run_root: &Path) -> Result<Self, WorkspaceError> {
        let path = run_root.join(RUN_LOCK_FILE);
        let sequence = RUN_LOCK_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let owner = format!("pid={} sequence={sequence}\n", std::process::id()).into_bytes();
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    WorkspaceError::Busy(path.clone())
                } else {
                    WorkspaceError::Io(error)
                }
            })?;
        let result = (|| {
            file.write_all(&owner).map_err(WorkspaceError::Io)?;
            file.sync_all().map_err(WorkspaceError::Io)
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&path);
            return Err(error);
        }
        Ok(Self { path, owner })
    }
}

impl Drop for WorkspaceLock {
    fn drop(&mut self) {
        if fs::read(&self.path).is_ok_and(|bytes| bytes == self.owner) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn create_managed_directory_chain(path: &Path, root: &Path) -> Result<(), WorkspaceError> {
    validate_under_root(path, root)?;
    let relative = path
        .strip_prefix(root)
        .map_err(|_| WorkspaceError::PathEscape)?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        ensure_plain_directory(&current)?;
    }
    Ok(())
}

fn ensure_plain_directory(path: &Path) -> Result<(), WorkspaceError> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(path).map_err(WorkspaceError::Io)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                Err(WorkspaceError::UnsafeManagedObject)
            } else {
                Ok(())
            }
        }
        Err(error) => Err(WorkspaceError::Io(error)),
    }
}

fn validate_repository_mount_targets(
    envelope: &ExecutionPlanEnvelopeV1,
    repository: &Path,
) -> Result<(), WorkspaceError> {
    let repository = fs::canonicalize(repository).map_err(WorkspaceError::Io)?;
    for cache in &envelope.plan.caches {
        validate_repository_mount_target(&repository, &cache.mount_path, true)?;
    }
    for check in &envelope.plan.checks {
        for artifact in &check.artifacts {
            validate_repository_mount_target(&repository, artifact, false)?;
        }
    }
    Ok(())
}

fn validate_repository_mount_target(
    repository: &Path,
    relative: &str,
    expect_directory: bool,
) -> Result<(), WorkspaceError> {
    let logical = PathBuf::from(relative);
    let target = repository.join(&logical);
    let metadata = match fs::symlink_metadata(&target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(WorkspaceError::MissingRepositoryMountTarget(logical));
        }
        Err(error) => return Err(WorkspaceError::Io(error)),
    };
    let correct_type = if expect_directory {
        metadata.is_dir()
    } else {
        metadata.is_file()
    };
    if metadata.file_type().is_symlink() || !correct_type {
        return Err(WorkspaceError::UnsafeRepositoryMountTarget(logical));
    }
    let canonical = fs::canonicalize(&target).map_err(WorkspaceError::Io)?;
    if !canonical.starts_with(repository) {
        return Err(WorkspaceError::PathEscape);
    }
    Ok(())
}

fn prepare_artifact_file(run_root: &Path, artifact: &str) -> Result<(), WorkspaceError> {
    let path = run_root.join("artifacts").join(artifact);
    validate_under_root(&path, run_root)?;
    let parent = path.parent().ok_or(WorkspaceError::PathEscape)?;
    create_managed_directory_chain(parent, run_root)?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(WorkspaceError::UnsafeManagedObject);
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(WorkspaceError::Io(error)),
    }
    File::create(path).map_err(WorkspaceError::Io)?;
    Ok(())
}

fn container_target(relative: &str) -> Result<String, WorkspaceError> {
    if relative.is_empty() || relative.starts_with('/') || relative.contains('\\') {
        return Err(WorkspaceError::InvalidLogicalPath);
    }
    Ok(format!("{CONTAINER_WORKSPACE}/{relative}"))
}

fn validate_under_root(path: &Path, root: &Path) -> Result<(), WorkspaceError> {
    if path == root || path.starts_with(root) {
        Ok(())
    } else {
        Err(WorkspaceError::PathEscape)
    }
}

pub fn validate_host_path(path: &Path) -> Result<(), WorkspaceError> {
    let text = path.to_str().ok_or(WorkspaceError::UnsupportedHostPath)?;
    if text.contains(',') || text.chars().any(char::is_control) {
        return Err(WorkspaceError::UnsupportedHostPath);
    }
    Ok(())
}

#[derive(Debug)]
pub enum WorkspaceError {
    RepositoryNotDirectory,
    InvalidLogicalPath,
    UnsupportedHostPath,
    DuplicateTarget(String),
    MissingRepositoryMountTarget(PathBuf),
    UnsafeRepositoryMountTarget(PathBuf),
    PathEscape,
    Busy(PathBuf),
    UnsafeManagedObject,
    Cache(CacheError),
    Io(std::io::Error),
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RepositoryNotDirectory => formatter.write_str("repository is not a directory"),
            Self::InvalidLogicalPath => formatter.write_str("workspace logical path is invalid"),
            Self::UnsupportedHostPath => {
                formatter.write_str("host mount path cannot be represented safely")
            }
            Self::DuplicateTarget(target) => write!(formatter, "duplicate mount target: {target}"),
            Self::MissingRepositoryMountTarget(target) => write!(
                formatter,
                "repository mount target does not exist; create it before run: {}",
                target.display()
            ),
            Self::UnsafeRepositoryMountTarget(target) => write!(
                formatter,
                "repository mount target has the wrong type or is unsafe: {}",
                target.display()
            ),
            Self::PathEscape => formatter.write_str("writable mount escaped its managed root"),
            Self::Busy(path) => write!(
                formatter,
                "another run owns this workspace generation; verify no runner is active before removing {}",
                path.display()
            ),
            Self::UnsafeManagedObject => {
                formatter.write_str("unsafe object found inside managed workspace")
            }
            Self::Cache(error) => write!(formatter, "{error}"),
            Self::Io(_) => formatter.write_str("workspace filesystem operation failed"),
        }
    }
}

impl std::error::Error for WorkspaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Cache(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{CacheRootOptions, PlatformFamily, ResolvedCacheRoot};
    use crate::config::ConfigV1;

    fn test_root(name: &str) -> PathBuf {
        std::env::var_os("CCP_TEST_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .expect("current directory")
                    .parent()
                    .expect("repository parent")
                    .to_path_buf()
            })
            .join(format!(".ccp-workspace-test-{}-{name}", std::process::id()))
    }

    fn fixture(name: &str) -> (PathBuf, ResolvedCacheRoot, ExecutionPlanEnvelopeV1) {
        let base = test_root(name);
        if base.exists() {
            fs::remove_dir_all(&base).expect("clean test root");
        }
        let repository = base.join("repository");
        let cache_root = base.join("persistent-cache");
        fs::create_dir_all(repository.join(".cache/cargo")).expect("cache mount target");
        fs::create_dir_all(repository.join("target")).expect("artifact parent");
        File::create(repository.join("target/results.json")).expect("artifact mount target");
        let resolved = ResolvedCacheRoot::resolve(
            &repository,
            &CacheRootOptions {
                explicit: Some(cache_root),
                environment: None,
                home: None,
                xdg_cache_home: None,
                local_app_data: None,
                platform: PlatformFamily::Unix,
            },
        )
        .expect("resolve cache");
        let envelope = ConfigV1::parse(
            r#"
schema_version = "1.0"
project = "owner/repository"

[runtime]
kind = "docker_compatible"
image = "example.invalid/ci@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
cpu_count = 1
memory_mib = 128
pids_limit = 16

[[caches]]
id = "cargo"
mount_path = ".cache/cargo"

[[checks]]
id = "test"
required = true
argv = ["cargo", "test"]
working_directory = "."
timeout_seconds = 60
artifacts = ["target/results.json"]
"#,
        )
        .expect("config")
        .into_plan()
        .expect("plan");
        (repository, resolved, envelope)
    }

    #[test]
    fn repository_is_read_only_and_writable_mounts_stay_managed() {
        let name = "mount-policy";
        let (repository, cache, envelope) = fixture(name);
        let plan = WorkspacePlanV1::build(&envelope, &repository, &cache).expect("plan");

        assert_eq!(plan.mounts[0].purpose, MountPurpose::Repository);
        assert_eq!(plan.mounts[0].access, MountAccess::ReadOnly);
        assert_eq!(plan.mounts[0].target, "/workspace");
        assert_eq!(
            plan.mounts
                .iter()
                .filter(|mount| mount.access == MountAccess::ReadWrite)
                .count(),
            2
        );
        for mount in plan
            .mounts
            .iter()
            .filter(|mount| mount.access == MountAccess::ReadWrite)
        {
            assert!(mount.source.starts_with(&cache.path));
            assert!(mount.target.starts_with("/workspace/"));
        }
        fs::remove_dir_all(test_root(name)).expect("clean fixture");
    }

    #[test]
    fn mount_plan_is_deterministic_for_fixed_roots() {
        let name = "deterministic";
        let (repository, cache, envelope) = fixture(name);
        let first = WorkspacePlanV1::build(&envelope, &repository, &cache).expect("first");
        let second = WorkspacePlanV1::build(&envelope, &repository, &cache).expect("second");
        assert_eq!(first, second);
        fs::remove_dir_all(test_root(name)).expect("clean fixture");
    }

    #[test]
    fn comma_in_host_path_is_rejected_before_docker_rendering() {
        assert!(matches!(
            validate_host_path(Path::new("/safe/path,with-comma")),
            Err(WorkspaceError::UnsupportedHostPath)
        ));
    }

    #[test]
    fn prepared_workspace_rejects_a_missing_repository_mount_target_before_mutation() {
        let name = "missing-mount-target";
        let (repository, resolved, envelope) = fixture(name);
        fs::remove_dir_all(repository.join(".cache/cargo")).expect("remove cache target");
        let cache = ManagedCache::initialize(resolved.clone()).expect("cache");

        assert!(matches!(
            PreparedWorkspace::prepare(&envelope, &repository, &cache),
            Err(WorkspaceError::MissingRepositoryMountTarget(path))
                if path == Path::new(".cache/cargo")
        ));
        assert!(
            !cache
                .workspace_path(&envelope.plan_digest)
                .expect("workspace path")
                .exists()
        );
        fs::remove_dir_all(test_root(name)).expect("clean fixture");
    }

    #[test]
    fn prepared_workspace_rejects_wrong_repository_mount_target_type() {
        let name = "wrong-mount-target-type";
        let (repository, resolved, envelope) = fixture(name);
        fs::remove_dir_all(repository.join(".cache/cargo")).expect("remove cache target");
        File::create(repository.join(".cache/cargo")).expect("replace target with file");
        let cache = ManagedCache::initialize(resolved).expect("cache");

        assert!(matches!(
            PreparedWorkspace::prepare(&envelope, &repository, &cache),
            Err(WorkspaceError::UnsafeRepositoryMountTarget(path))
                if path == Path::new(".cache/cargo")
        ));
        fs::remove_dir_all(test_root(name)).expect("clean fixture");
    }

    #[test]
    fn prepared_workspace_creates_only_managed_writable_paths_and_releases_lock() {
        let name = "prepared";
        let (repository, resolved, envelope) = fixture(name);
        let cache = ManagedCache::initialize(resolved.clone()).expect("cache");
        let lock_path;
        {
            let prepared =
                PreparedWorkspace::prepare(&envelope, &repository, &cache).expect("prepare");
            lock_path = prepared.lock_path().to_path_buf();
            assert!(lock_path.is_file());
            assert!(
                prepared
                    .plan
                    .mounts
                    .iter()
                    .filter(|mount| mount.access == MountAccess::ReadWrite)
                    .all(|mount| mount.source.starts_with(&resolved.path))
            );
            assert!(matches!(
                PreparedWorkspace::prepare(&envelope, &repository, &cache),
                Err(WorkspaceError::Busy(path)) if path == lock_path
            ));
        }
        assert!(!lock_path.exists());
        fs::remove_dir_all(test_root(name)).expect("clean fixture");
    }

    #[test]
    fn workspace_drop_never_removes_a_replaced_lock_owner() {
        let name = "replaced-lock";
        let (repository, resolved, envelope) = fixture(name);
        let cache = ManagedCache::initialize(resolved).expect("cache");
        let prepared = PreparedWorkspace::prepare(&envelope, &repository, &cache).expect("prepare");
        let lock_path = prepared.lock_path().to_path_buf();
        fs::write(&lock_path, b"replacement-owner\n").expect("replace lock owner");
        drop(prepared);
        assert_eq!(
            fs::read(&lock_path).expect("replacement lock remains"),
            b"replacement-owner\n"
        );
        fs::remove_file(lock_path).expect("remove exact replacement lock");
        fs::remove_dir_all(test_root(name)).expect("clean fixture");
    }

    #[test]
    fn workspace_mounts_the_explicit_snapshot_root_not_the_live_checkout() {
        let name = "explicit-snapshot-root";
        let (repository, cache, envelope) = fixture(name);
        let snapshot = test_root(name).join("snapshot");
        fs::create_dir(&snapshot).expect("snapshot root");
        fs::create_dir_all(snapshot.join(".cache/cargo")).expect("cache target");
        fs::create_dir_all(snapshot.join("results")).expect("artifact parent");
        fs::write(snapshot.join("results/first.json"), b"").expect("artifact target");

        let plan = WorkspacePlanV1::build(&envelope, &snapshot, &cache).expect("plan");

        assert_eq!(plan.mounts[0].source, snapshot);
        assert_ne!(plan.mounts[0].source, repository);
        fs::remove_dir_all(test_root(name)).expect("clean fixture");
    }
}
