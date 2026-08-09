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

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::cache::{CacheError, CacheKey, ResolvedCacheRoot};
use crate::config::ExecutionPlanEnvelopeV1;

const CONTAINER_WORKSPACE: &str = "/workspace";

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
    pub run_root: PathBuf,
    pub mounts: Vec<MountBinding>,
}

impl WorkspacePlanV1 {
    pub fn build(
        envelope: &ExecutionPlanEnvelopeV1,
        repository: &Path,
        cache: &ResolvedCacheRoot,
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
            let source = cache.entry_data_path(&key);
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
            run_root,
            mounts,
        })
    }
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
    PathEscape,
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
            Self::PathEscape => formatter.write_str("writable mount escaped its managed root"),
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
        std::env::current_dir()
            .expect("current directory")
            .parent()
            .expect("repository parent")
            .join(format!(".ccp-workspace-test-{}-{name}", std::process::id()))
    }

    fn fixture(name: &str) -> (PathBuf, ResolvedCacheRoot, ExecutionPlanEnvelopeV1) {
        let base = test_root(name);
        if base.exists() {
            fs::remove_dir_all(&base).expect("clean test root");
        }
        let repository = base.join("repository");
        let cache_root = base.join("persistent-cache");
        fs::create_dir_all(&repository).expect("repository");
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
}
