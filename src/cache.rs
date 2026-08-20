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

use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::thread;
use std::time::Duration;

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::{ExecutionPlanEnvelopeV1, NormalizedCache};
use crate::receipt::{ReceiptError, canonical_json};

pub const DEFAULT_DISK_BUDGET_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const OWNER_FILE: &str = ".ccp-cache-root-v1.json";
const DEFAULT_CACHE_ROOT_NAME: &str = "commit-ci-preflight-build-v1";
const OWNER_BYTES: &[u8] =
    b"{\"owner\":\"commit-ci-preflight\",\"purpose\":\"managed-cache-root\",\"schema_version\":\"1.0\"}\n";
const ENTRIES_DIR: &str = "entries";
const WORKSPACES_DIR: &str = "workspaces";
const COMPLETE_FILE: &str = ".complete-v1";
const COMPLETE_BYTES: &[u8] = b"complete-v1\n";
const GENERATION_MANIFEST_FILE: &str = ".generation-v1.json";
const GENERATION_SCHEMA_VERSION: &str = "1.0";
const ENTRY_LOCK_FILE: &str = ".entry-lock-v1";
const PROMOTION_LOCK_FILE: &str = ".promotion-lock-v1";
const MAX_INVENTORY_NODES: usize = 100_000;
const INIT_RETRIES: usize = 40;
const INIT_RETRY_DELAY: Duration = Duration::from_millis(5);
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheRootSource {
    Explicit,
    Environment,
    PlatformDefault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformFamily {
    MacOs,
    Windows,
    Unix,
}

impl PlatformFamily {
    pub fn current() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::MacOs
        }
        #[cfg(windows)]
        {
            Self::Windows
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            Self::Unix
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheRootOptions {
    pub explicit: Option<PathBuf>,
    pub environment: Option<OsString>,
    pub home: Option<PathBuf>,
    pub xdg_cache_home: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    pub platform: PlatformFamily,
}

impl CacheRootOptions {
    pub fn from_process(explicit: Option<PathBuf>) -> Self {
        Self {
            explicit,
            environment: std::env::var_os("CCP_CACHE_DIR"),
            home: std::env::var_os("HOME").map(PathBuf::from),
            xdg_cache_home: std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from),
            local_app_data: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            platform: PlatformFamily::current(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedCacheRoot {
    pub path: PathBuf,
    pub source: CacheRootSource,
}

impl ResolvedCacheRoot {
    pub fn resolve(repository: &Path, options: &CacheRootOptions) -> Result<Self, CacheError> {
        let (candidate, source) = if let Some(path) = &options.explicit {
            (path.clone(), CacheRootSource::Explicit)
        } else if let Some(path) = &options.environment {
            (PathBuf::from(path), CacheRootSource::Environment)
        } else {
            (platform_default(options)?, CacheRootSource::PlatformDefault)
        };
        let repository = canonical_directory(repository, "repository")?;
        let path = validate_cache_candidate(&candidate, &repository)?;
        Ok(Self { path, source })
    }

    pub fn entry_path(&self, key: &CacheKey) -> PathBuf {
        self.path.join(ENTRIES_DIR).join(key.directory_name())
    }

    pub fn entry_data_path(&self, key: &CacheKey) -> PathBuf {
        self.entry_path(key).join("data")
    }

    pub fn workspace_path(&self, plan_digest: &str) -> Result<PathBuf, CacheError> {
        let digest = validated_digest_hex(plan_digest)?;
        Ok(self
            .path
            .join(WORKSPACES_DIR)
            .join(format!("sha256-{digest}")))
    }
}

fn platform_default(options: &CacheRootOptions) -> Result<PathBuf, CacheError> {
    let suffix = Path::new(DEFAULT_CACHE_ROOT_NAME);
    match options.platform {
        PlatformFamily::MacOs => options
            .home
            .as_ref()
            .map(|home| home.join("Library").join("Caches").join(suffix)),
        PlatformFamily::Windows => options
            .local_app_data
            .as_ref()
            .map(|base| base.join(suffix)),
        PlatformFamily::Unix => options
            .xdg_cache_home
            .as_ref()
            .map(|base| base.join(suffix))
            .or_else(|| {
                options
                    .home
                    .as_ref()
                    .map(|home| home.join(".cache").join(suffix))
            }),
    }
    .ok_or(CacheError::NoPersistentDefault)
}

fn validate_cache_candidate(candidate: &Path, repository: &Path) -> Result<PathBuf, CacheError> {
    reject_unresolved(candidate)?;
    if !candidate.is_absolute() {
        return Err(CacheError::UnsafePath(
            "cache root must be an absolute path",
        ));
    }
    reject_lexical_escape(candidate)?;
    reject_symlink_components(candidate)?;
    let resolved = canonicalize_existing_prefix(candidate)?;
    if resolved.parent().is_none() {
        return Err(CacheError::UnsafePath(
            "filesystem root cannot be a cache root",
        ));
    }
    if resolved == repository || resolved.starts_with(repository) {
        return Err(CacheError::UnsafePath(
            "cache root cannot be the repository or one of its descendants",
        ));
    }
    let temporary = canonicalize_existing_prefix(&std::env::temp_dir())?;
    if resolved == temporary || resolved.starts_with(&temporary) {
        return Err(CacheError::UnsafePath(
            "cache root cannot be a temporary directory",
        ));
    }
    Ok(resolved)
}

fn reject_unresolved(path: &Path) -> Result<(), CacheError> {
    let text = path
        .to_str()
        .ok_or(CacheError::UnsafePath("cache root must be valid UTF-8"))?;
    if text.starts_with('~')
        || text.contains('$')
        || (text.contains('%') && text.split('%').count() >= 3)
    {
        return Err(CacheError::UnsafePath(
            "cache root contains an unresolved variable or home shorthand",
        ));
    }
    Ok(())
}

fn reject_lexical_escape(path: &Path) -> Result<(), CacheError> {
    if path
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(CacheError::UnsafePath(
            "cache root cannot contain dot or parent components",
        ));
    }
    Ok(())
}

fn reject_symlink_components(path: &Path) -> Result<(), CacheError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(CacheError::UnsafePath(
                    "cache root cannot traverse a symbolic link",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(CacheError::Io(error)),
        }
    }
    Ok(())
}

fn canonicalize_existing_prefix(path: &Path) -> Result<PathBuf, CacheError> {
    let mut ancestor = path.to_path_buf();
    let mut tail = Vec::new();
    loop {
        match fs::canonicalize(&ancestor) {
            Ok(mut canonical) => {
                for component in tail.iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let name = ancestor.file_name().ok_or(CacheError::UnsafePath(
                    "cache root has no existing absolute ancestor",
                ))?;
                tail.push(name.to_os_string());
                if !ancestor.pop() {
                    return Err(CacheError::UnsafePath(
                        "cache root has no existing absolute ancestor",
                    ));
                }
            }
            Err(error) => return Err(CacheError::Io(error)),
        }
    }
}

fn canonical_directory(path: &Path, label: &'static str) -> Result<PathBuf, CacheError> {
    let canonical = fs::canonicalize(path).map_err(CacheError::Io)?;
    if !canonical.is_dir() {
        return Err(CacheError::UnsafePath(label));
    }
    Ok(canonical)
}

#[derive(Debug, Clone)]
pub struct ManagedCache {
    root: ResolvedCacheRoot,
    disk_budget_bytes: u64,
}

impl ManagedCache {
    pub fn initialize(root: ResolvedCacheRoot) -> Result<Self, CacheError> {
        fs::create_dir_all(&root.path).map_err(CacheError::Io)?;
        reject_symlink_components(&root.path)?;
        let marker = root.path.join(OWNER_FILE);
        if marker.exists() {
            validate_owner_marker(&marker)?;
        } else {
            wait_for_concurrent_initializer(&root.path, &marker)?;
            if !marker.exists() {
                ensure_unowned_root_is_empty(&root.path)?;
                write_owner_marker(&root.path, &marker)?;
            }
            validate_owner_marker_after_initialization(&marker)?;
        }
        ensure_managed_directory(&root.path.join(ENTRIES_DIR))?;
        ensure_managed_directory(&root.path.join(WORKSPACES_DIR))?;
        Ok(Self {
            root,
            disk_budget_bytes: DEFAULT_DISK_BUDGET_BYTES,
        })
    }

    pub fn open(root: ResolvedCacheRoot) -> Result<Self, CacheError> {
        reject_symlink_components(&root.path)?;
        validate_owner_marker(&root.path.join(OWNER_FILE))?;
        validate_plain_directory(&root.path.join(ENTRIES_DIR))?;
        validate_plain_directory(&root.path.join(WORKSPACES_DIR))?;
        Ok(Self {
            root,
            disk_budget_bytes: DEFAULT_DISK_BUDGET_BYTES,
        })
    }

    pub fn with_disk_budget(mut self, bytes: u64) -> Result<Self, CacheError> {
        if bytes == 0 {
            return Err(CacheError::InvalidBudget);
        }
        self.disk_budget_bytes = bytes;
        Ok(self)
    }

    pub fn root(&self) -> &ResolvedCacheRoot {
        &self.root
    }

    pub fn entry_path(&self, key: &CacheKey) -> PathBuf {
        self.root.entry_path(key)
    }

    pub fn entry_data_path(&self, key: &CacheKey) -> PathBuf {
        self.root.entry_data_path(key)
    }

    pub fn workspace_path(&self, plan_digest: &str) -> Result<PathBuf, CacheError> {
        self.root.workspace_path(plan_digest)
    }

    pub fn prepare_entry(
        &self,
        key: &CacheKey,
        plan_digest: &str,
        generation: u64,
    ) -> Result<PreparedCacheEntry, CacheError> {
        validate_owner_marker(&self.root.path.join(OWNER_FILE))?;
        let entries_root = self.root.path.join(ENTRIES_DIR);
        validate_plain_directory(&entries_root)?;
        let path = self.entry_path(key);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(CacheError::SymlinkInManagedRoot(path));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(CacheError::UnexpectedEntry(path));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&path).map_err(CacheError::Io)?;
            }
            Err(error) => return Err(CacheError::Io(error)),
        }
        validated_digest_hex(plan_digest)?;
        let entry_lock = acquire_entry_lock(&path)?;
        let complete = entry_status(&path, &key.directory_name())? == CacheEntryStatus::Complete;
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let staging_path = path.join(format!(".staging-{}-{sequence}", std::process::id()));
        ensure_managed_directory(&staging_path)?;
        let data_path = staging_path.join("data");
        ensure_managed_directory(&data_path)?;
        if complete {
            copy_tree(&path.join("data"), &data_path)?;
        }
        let manifest = CacheGenerationManifestV1 {
            schema_version: GENERATION_SCHEMA_VERSION.to_owned(),
            key_digest: key.digest.clone(),
            plan_digest: plan_digest.to_owned(),
            generation,
            state: "staging".to_owned(),
        };
        write_generation_manifest(&staging_path, &manifest)?;
        Ok(PreparedCacheEntry {
            path,
            data_path,
            staging_path,
            key_digest: key.digest.clone(),
            plan_digest: plan_digest.to_owned(),
            generation,
            _entry_lock: entry_lock,
            was_complete: complete,
        })
    }

    pub fn promote_entry(&self, prepared: &PreparedCacheEntry) -> Result<(), CacheError> {
        validate_owner_marker(&self.root.path.join(OWNER_FILE))?;
        let _promotion_lock = acquire_promotion_lock(&self.root.path)?;
        validate_plain_directory(&prepared.path)?;
        validate_plain_directory(&prepared.staging_path)?;
        validate_plain_directory(&prepared.data_path)?;
        let manifest_path = prepared.staging_path.join(GENERATION_MANIFEST_FILE);
        let manifest = read_generation_manifest(&manifest_path)?;
        if manifest.key_digest != prepared.key_digest
            || manifest.plan_digest != prepared.plan_digest
            || manifest.generation != prepared.generation
            || manifest.state != "staging"
        {
            return Err(CacheError::GenerationMismatch);
        }
        let mut complete_manifest = manifest;
        complete_manifest.state = "complete".to_owned();
        write_generation_manifest_replacing(&manifest_path, &complete_manifest)?;

        let current = prepared.path.join("data");
        let backup = prepared.path.join(format!(
            ".backup-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let marker = prepared.path.join(COMPLETE_FILE);
        let previous_marker = read_optional_file(&marker)?;
        let previous_manifest_path = prepared.path.join(GENERATION_MANIFEST_FILE);
        let previous_manifest = read_optional_file(&previous_manifest_path)?;

        let had_current = match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(CacheError::SymlinkInManagedRoot(current));
            }
            Ok(metadata) if metadata.is_dir() => true,
            Ok(_) => return Err(CacheError::UnexpectedEntry(current)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(CacheError::Io(error)),
        };
        remove_if_present(&marker)?;
        remove_if_present(&previous_manifest_path)?;
        if had_current {
            fs::rename(&current, &backup).map_err(CacheError::Io)?;
        }

        let result = (|| {
            fs::rename(&prepared.data_path, &current).map_err(CacheError::Io)?;
            fs::rename(&manifest_path, &previous_manifest_path).map_err(CacheError::Io)?;
            write_complete_marker(&marker)?;
            Ok::<(), CacheError>(())
        })();

        match result {
            Ok(()) => {
                if had_current {
                    fs::remove_dir_all(&backup).map_err(CacheError::Io)?;
                }
                remove_if_present(&prepared.staging_path)?;
                Ok(())
            }
            Err(error) => {
                let rollback = (|| {
                    remove_if_present(&current)?;
                    if had_current {
                        fs::rename(&backup, &current).map_err(CacheError::Io)?;
                    }
                    if let Some(bytes) = previous_manifest {
                        fs::write(&previous_manifest_path, bytes).map_err(CacheError::Io)?;
                    }
                    if let Some(bytes) = previous_marker {
                        fs::write(&marker, bytes).map_err(CacheError::Io)?;
                    }
                    Ok::<(), CacheError>(())
                })();
                if rollback.is_err() {
                    return Err(CacheError::PromotionUncertain);
                }
                Err(error)
            }
        }
    }

    pub fn mark_entry_complete(&self, key: &CacheKey) -> Result<(), CacheError> {
        validate_owner_marker(&self.root.path.join(OWNER_FILE))?;
        let entry = self.entry_path(key);
        validate_plain_directory(&entry)?;
        validate_plain_directory(&entry.join("data"))?;
        let marker = entry.join(COMPLETE_FILE);
        match fs::symlink_metadata(&marker) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(CacheError::SymlinkInManagedRoot(marker));
            }
            Ok(metadata) if !metadata.is_file() => {
                return Err(CacheError::UnexpectedEntry(marker));
            }
            Ok(_) => {
                if fs::read(&marker).map_err(CacheError::Io)? == COMPLETE_BYTES {
                    return Ok(());
                }
                return Err(CacheError::UnexpectedEntry(marker));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(CacheError::Io(error)),
        }
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = entry.join(format!(".complete-tmp-{}-{sequence}", std::process::id()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(CacheError::Io)?;
            file.write_all(COMPLETE_BYTES).map_err(CacheError::Io)?;
            file.sync_all().map_err(CacheError::Io)?;
            fs::rename(&temporary, &marker).map_err(CacheError::Io)
        })();
        if temporary.exists() {
            let _ = fs::remove_file(&temporary);
        }
        result?;
        Ok(())
    }

    pub fn inventory(&self) -> Result<CacheInventory, CacheError> {
        validate_owner_marker(&self.root.path.join(OWNER_FILE))?;
        let entries_root = self.root.path.join(ENTRIES_DIR);
        validate_plain_directory(&entries_root)?;
        let mut entries = Vec::new();
        for entry in sorted_directory_entries(&entries_root)? {
            let metadata = fs::symlink_metadata(entry.path()).map_err(CacheError::Io)?;
            if metadata.file_type().is_symlink() {
                return Err(CacheError::SymlinkInManagedRoot(entry.path()));
            }
            if !metadata.is_dir() {
                return Err(CacheError::UnexpectedEntry(entry.path()));
            }
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| CacheError::UnexpectedEntry(entry.path()))?;
            if !is_cache_directory_name(&name) && !name.starts_with(".entry-tmp-") {
                return Err(CacheError::UnexpectedEntry(entry.path()));
            }
            let mut nodes = 0;
            let (bytes, files) = bounded_tree_size(&entry.path(), &mut nodes)?;
            let status = entry_status(&entry.path(), &name)?;
            entries.push(CacheEntryInventory {
                directory: name,
                status,
                bytes,
                files,
            });
        }
        let total_bytes = entries.iter().try_fold(0_u64, |total, entry| {
            total
                .checked_add(entry.bytes)
                .ok_or(CacheError::SizeOverflow)
        })?;
        Ok(CacheInventory {
            root: self.root.path.clone(),
            source: self.root.source,
            entries,
            total_bytes,
            disk_budget_bytes: self.disk_budget_bytes,
            budget_exceeded: total_bytes > self.disk_budget_bytes,
        })
    }

    pub fn cleanup_dry_run(&self) -> Result<CleanupPlan, CacheError> {
        let inventory = self.inventory()?;
        let candidates: Vec<_> = inventory
            .entries
            .iter()
            .filter(|entry| entry.status == CacheEntryStatus::Incomplete)
            .map(|entry| CleanupCandidate {
                relative_path: PathBuf::from(ENTRIES_DIR).join(&entry.directory),
                reason: CleanupReason::IncompleteEntry,
                bytes: entry.bytes,
            })
            .collect();
        let reclaimable_bytes = candidates.iter().try_fold(0_u64, |total, candidate| {
            total
                .checked_add(candidate.bytes)
                .ok_or(CacheError::SizeOverflow)
        })?;
        Ok(CleanupPlan {
            root: inventory.root,
            total_bytes: inventory.total_bytes,
            disk_budget_bytes: inventory.disk_budget_bytes,
            budget_exceeded: inventory.budget_exceeded,
            candidates,
            reclaimable_bytes,
            deletion_performed: false,
        })
    }
}

#[derive(Debug, Clone)]
pub struct PreparedCacheEntry {
    pub path: PathBuf,
    pub data_path: PathBuf,
    staging_path: PathBuf,
    key_digest: String,
    plan_digest: String,
    generation: u64,
    _entry_lock: Arc<File>,
    pub was_complete: bool,
}

impl Drop for PreparedCacheEntry {
    fn drop(&mut self) {
        let Ok(manifest) =
            read_generation_manifest(&self.staging_path.join(GENERATION_MANIFEST_FILE))
        else {
            return;
        };
        if manifest.schema_version == GENERATION_SCHEMA_VERSION
            && manifest.key_digest == self.key_digest
            && manifest.plan_digest == self.plan_digest
            && manifest.generation == self.generation
            && manifest.state == "staging"
        {
            let _ = fs::remove_dir_all(&self.staging_path);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheGenerationManifestV1 {
    pub schema_version: String,
    pub key_digest: String,
    pub plan_digest: String,
    pub generation: u64,
    pub state: String,
}

fn write_generation_manifest(
    staging_path: &Path,
    manifest: &CacheGenerationManifestV1,
) -> Result<(), CacheError> {
    let bytes = canonical_json(manifest).map_err(CacheError::Canonical)?;
    let path = staging_path.join(GENERATION_MANIFEST_FILE);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(CacheError::Io)?;
    file.write_all(&bytes).map_err(CacheError::Io)?;
    file.write_all(b"\n").map_err(CacheError::Io)?;
    file.sync_all().map_err(CacheError::Io)
}

fn write_generation_manifest_replacing(
    path: &Path,
    manifest: &CacheGenerationManifestV1,
) -> Result<(), CacheError> {
    remove_if_present(path)?;
    write_generation_manifest(
        path.parent().ok_or(CacheError::PromotionUncertain)?,
        manifest,
    )
    .and_then(|_| {
        let generated = path
            .parent()
            .ok_or(CacheError::PromotionUncertain)?
            .join(GENERATION_MANIFEST_FILE);
        if generated != path {
            return Err(CacheError::PromotionUncertain);
        }
        Ok(())
    })
}

fn read_generation_manifest(path: &Path) -> Result<CacheGenerationManifestV1, CacheError> {
    let bytes = fs::read(path).map_err(CacheError::Io)?;
    serde_json::from_slice(&bytes).map_err(|error| {
        CacheError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            error.to_string(),
        ))
    })
}

fn read_optional_file(path: &Path) -> Result<Option<Vec<u8>>, CacheError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(CacheError::SymlinkInManagedRoot(path.to_path_buf()))
        }
        Ok(metadata) if metadata.is_file() => fs::read(path).map(Some).map_err(CacheError::Io),
        Ok(_) => Err(CacheError::UnexpectedEntry(path.to_path_buf())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(CacheError::Io(error)),
    }
}

fn remove_if_present(path: &Path) -> Result<(), CacheError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(CacheError::SymlinkInManagedRoot(path.to_path_buf()))
        }
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path).map_err(CacheError::Io),
        Ok(metadata) if metadata.is_file() => fs::remove_file(path).map_err(CacheError::Io),
        Ok(_) => Err(CacheError::UnexpectedEntry(path.to_path_buf())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CacheError::Io(error)),
    }
}

fn write_complete_marker(path: &Path) -> Result<(), CacheError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(CacheError::Io)?;
    file.write_all(COMPLETE_BYTES).map_err(CacheError::Io)?;
    file.sync_all().map_err(CacheError::Io)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), CacheError> {
    let metadata = fs::symlink_metadata(source).map_err(CacheError::Io)?;
    if metadata.file_type().is_symlink() {
        return Err(CacheError::SymlinkInManagedRoot(source.to_path_buf()));
    }
    if metadata.is_file() {
        fs::copy(source, destination).map_err(CacheError::Io)?;
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(CacheError::UnexpectedEntry(source.to_path_buf()));
    }
    ensure_managed_directory(destination)?;
    for entry in sorted_directory_entries(source)? {
        let name = entry.file_name();
        copy_tree(&entry.path(), &destination.join(name))?;
    }
    Ok(())
}

fn wait_for_concurrent_initializer(root: &Path, marker: &Path) -> Result<(), CacheError> {
    for _ in 0..INIT_RETRIES {
        if marker.exists() {
            return Ok(());
        }
        let entries = sorted_directory_entries(root)?;
        if entries.is_empty() {
            return Ok(());
        }
        if entries.iter().all(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".owner-tmp-")
        }) {
            thread::sleep(INIT_RETRY_DELAY);
            continue;
        }
        return Ok(());
    }
    Ok(())
}

fn validate_owner_marker_after_initialization(marker: &Path) -> Result<(), CacheError> {
    for attempt in 0..INIT_RETRIES {
        match validate_owner_marker(marker) {
            Err(CacheError::OwnershipMissing(_)) if attempt + 1 < INIT_RETRIES => {
                thread::sleep(INIT_RETRY_DELAY);
            }
            result => return result,
        }
    }
    unreachable!("bounded marker validation always returns")
}

fn ensure_unowned_root_is_empty(root: &Path) -> Result<(), CacheError> {
    let unexpected = sorted_directory_entries(root)?.into_iter().find(|entry| {
        !entry
            .file_name()
            .to_string_lossy()
            .starts_with(".owner-tmp-")
    });
    if unexpected.is_some() {
        return Err(CacheError::OwnershipMissing(root.to_path_buf()));
    }
    Ok(())
}

fn write_owner_marker(root: &Path, marker: &Path) -> Result<(), CacheError> {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = root.join(format!(".owner-tmp-{}-{sequence}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(CacheError::Io)?;
        file.write_all(OWNER_BYTES).map_err(CacheError::Io)?;
        file.sync_all().map_err(CacheError::Io)?;
        match fs::rename(&temporary, marker) {
            Ok(()) => Ok(()),
            Err(_) if marker.exists() => validate_owner_marker(marker),
            Err(error) => Err(CacheError::Io(error)),
        }
    })();
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn validate_owner_marker(marker: &Path) -> Result<(), CacheError> {
    match fs::symlink_metadata(marker) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(CacheError::SymlinkInManagedRoot(marker.to_path_buf()));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(CacheError::InvalidOwnershipMarker(marker.to_path_buf()));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(CacheError::OwnershipMissing(
                marker
                    .parent()
                    .unwrap_or_else(|| Path::new("/"))
                    .to_path_buf(),
            ));
        }
        Err(error) => return Err(CacheError::Io(error)),
    }
    let file = File::open(marker).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            CacheError::OwnershipMissing(
                marker
                    .parent()
                    .unwrap_or_else(|| Path::new("/"))
                    .to_path_buf(),
            )
        } else {
            CacheError::Io(error)
        }
    })?;
    let mut bytes = Vec::new();
    file.take((OWNER_BYTES.len() + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(CacheError::Io)?;
    if bytes != OWNER_BYTES {
        return Err(CacheError::InvalidOwnershipMarker(marker.to_path_buf()));
    }
    Ok(())
}

fn ensure_managed_directory(path: &Path) -> Result<(), CacheError> {
    match fs::create_dir(path) {
        Ok(()) => validate_plain_directory(path),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            validate_plain_directory(path)
        }
        Err(error) => Err(CacheError::Io(error)),
    }
}

fn acquire_entry_lock(entry: &Path) -> Result<Arc<File>, CacheError> {
    acquire_advisory_lock(&entry.join(ENTRY_LOCK_FILE), "cache entry")
}

fn acquire_promotion_lock(root: &Path) -> Result<Arc<File>, CacheError> {
    acquire_advisory_lock(&root.join(PROMOTION_LOCK_FILE), "cache promotion")
}

fn acquire_advisory_lock(path: &Path, label: &'static str) -> Result<Arc<File>, CacheError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(CacheError::SymlinkInManagedRoot(path.to_path_buf()));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(CacheError::UnexpectedEntry(path.to_path_buf()));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(CacheError::Io(error)),
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(path)
        .map_err(CacheError::Io)?;
    if let Err(error) = file.try_lock_exclusive() {
        if error.kind() == io::ErrorKind::WouldBlock {
            return Err(CacheError::LockBusy(path.to_path_buf()));
        }
        return Err(CacheError::Io(error));
    }
    file.set_len(0).map_err(CacheError::Io)?;
    let owner = format!(
        "{{\"schema_version\":\"1.0\",\"owner\":\"commit-ci-preflight\",\"purpose\":\"{label}\",\"pid\":{}}}\n",
        std::process::id()
    );
    file.write_all(owner.as_bytes()).map_err(CacheError::Io)?;
    file.sync_all().map_err(CacheError::Io)?;
    Ok(Arc::new(file))
}

fn validate_plain_directory(path: &Path) -> Result<(), CacheError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(CacheError::SymlinkInManagedRoot(path.to_path_buf()))
        }
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(CacheError::UnexpectedEntry(path.to_path_buf())),
        Err(error) => Err(CacheError::Io(error)),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CacheKey {
    digest: String,
}

#[derive(Serialize)]
struct CacheKeyInput<'a> {
    schema_version: &'static str,
    project: &'a str,
    plan_digest: &'a str,
    image: &'a str,
    cache_id: &'a str,
}

impl CacheKey {
    pub fn for_plan_cache(
        envelope: &ExecutionPlanEnvelopeV1,
        cache: &NormalizedCache,
    ) -> Result<Self, CacheError> {
        let bytes = canonical_json(&CacheKeyInput {
            schema_version: "1.0",
            project: &envelope.plan.project,
            plan_digest: &envelope.plan_digest,
            image: &envelope.plan.runtime.image,
            cache_id: &cache.id,
        })
        .map_err(CacheError::Canonical)?;
        let digest = Sha256::digest(bytes);
        Ok(Self {
            digest: format!("sha256:{}", lowercase_hex(&digest)),
        })
    }

    pub fn directory_name(&self) -> String {
        format!("sha256-{}", &self.digest[7..])
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
}

fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn validated_digest_hex(digest: &str) -> Result<&str, CacheError> {
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return Err(CacheError::InvalidDigest);
    };
    if hex.len() != 64
        || !hex
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(CacheError::InvalidDigest);
    }
    Ok(hex)
}

fn is_cache_directory_name(name: &str) -> bool {
    validated_digest_hex(&name.replacen("sha256-", "sha256:", 1)).is_ok()
        && name.starts_with("sha256-")
}

fn entry_status(path: &Path, name: &str) -> Result<CacheEntryStatus, CacheError> {
    if name.starts_with(".entry-tmp-") {
        return Ok(CacheEntryStatus::Incomplete);
    }
    let marker = path.join(COMPLETE_FILE);
    match fs::symlink_metadata(&marker) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(CacheError::SymlinkInManagedRoot(marker))
        }
        Ok(metadata) if metadata.is_file() => {
            let bytes = fs::read(&marker).map_err(CacheError::Io)?;
            if bytes == COMPLETE_BYTES {
                let manifest = path.join(GENERATION_MANIFEST_FILE);
                if let Some(manifest_bytes) = read_optional_file(&manifest)? {
                    let parsed: CacheGenerationManifestV1 = serde_json::from_slice(&manifest_bytes)
                        .map_err(|error| {
                            CacheError::Io(io::Error::new(
                                io::ErrorKind::InvalidData,
                                error.to_string(),
                            ))
                        })?;
                    if parsed.state != "complete" {
                        return Err(CacheError::GenerationMismatch);
                    }
                }
                Ok(CacheEntryStatus::Complete)
            } else {
                Err(CacheError::UnexpectedEntry(marker))
            }
        }
        Ok(_) => Err(CacheError::UnexpectedEntry(marker)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(CacheEntryStatus::Incomplete),
        Err(error) => Err(CacheError::Io(error)),
    }
}

fn sorted_directory_entries(path: &Path) -> Result<Vec<fs::DirEntry>, CacheError> {
    let mut entries: Vec<_> = fs::read_dir(path)
        .map_err(CacheError::Io)?
        .collect::<Result<_, _>>()
        .map_err(CacheError::Io)?;
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
}

fn bounded_tree_size(path: &Path, nodes: &mut usize) -> Result<(u64, u64), CacheError> {
    *nodes = nodes.checked_add(1).ok_or(CacheError::SizeOverflow)?;
    if *nodes > MAX_INVENTORY_NODES {
        return Err(CacheError::InventoryLimitExceeded);
    }
    let metadata = fs::symlink_metadata(path).map_err(CacheError::Io)?;
    if metadata.file_type().is_symlink() {
        return Err(CacheError::SymlinkInManagedRoot(path.to_path_buf()));
    }
    if metadata.is_file() {
        return Ok((metadata.len(), 1));
    }
    if !metadata.is_dir() {
        return Err(CacheError::UnexpectedEntry(path.to_path_buf()));
    }
    let mut bytes = 0_u64;
    let mut files = 0_u64;
    for entry in sorted_directory_entries(path)? {
        let (entry_bytes, entry_files) = bounded_tree_size(&entry.path(), nodes)?;
        bytes = bytes
            .checked_add(entry_bytes)
            .ok_or(CacheError::SizeOverflow)?;
        files = files
            .checked_add(entry_files)
            .ok_or(CacheError::SizeOverflow)?;
    }
    Ok((bytes, files))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheEntryStatus {
    Complete,
    Incomplete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CacheEntryInventory {
    pub directory: String,
    pub status: CacheEntryStatus,
    pub bytes: u64,
    pub files: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CacheInventory {
    pub root: PathBuf,
    pub source: CacheRootSource,
    pub entries: Vec<CacheEntryInventory>,
    pub total_bytes: u64,
    pub disk_budget_bytes: u64,
    pub budget_exceeded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupReason {
    IncompleteEntry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CleanupCandidate {
    pub relative_path: PathBuf,
    pub reason: CleanupReason,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CleanupPlan {
    pub root: PathBuf,
    pub total_bytes: u64,
    pub disk_budget_bytes: u64,
    pub budget_exceeded: bool,
    pub candidates: Vec<CleanupCandidate>,
    pub reclaimable_bytes: u64,
    pub deletion_performed: bool,
}

#[derive(Debug)]
pub enum CacheError {
    NoPersistentDefault,
    UnsafePath(&'static str),
    OwnershipMissing(PathBuf),
    InvalidOwnershipMarker(PathBuf),
    SymlinkInManagedRoot(PathBuf),
    UnexpectedEntry(PathBuf),
    InventoryLimitExceeded,
    InvalidBudget,
    InvalidDigest,
    GenerationMismatch,
    LockBusy(PathBuf),
    PromotionUncertain,
    SizeOverflow,
    Canonical(ReceiptError),
    Io(io::Error),
}

impl CacheError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::NoPersistentDefault
            | Self::UnsafePath(_)
            | Self::InvalidBudget
            | Self::InvalidDigest => 2,
            Self::OwnershipMissing(_)
            | Self::InvalidOwnershipMarker(_)
            | Self::SymlinkInManagedRoot(_)
            | Self::UnexpectedEntry(_)
            | Self::InventoryLimitExceeded
            | Self::GenerationMismatch
            | Self::LockBusy(_)
            | Self::PromotionUncertain
            | Self::SizeOverflow
            | Self::Canonical(_)
            | Self::Io(_) => 70,
        }
    }
}

impl fmt::Display for CacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPersistentDefault => formatter.write_str(
                "no safe persistent cache directory is available; use --cache-dir or CCP_CACHE_DIR",
            ),
            Self::UnsafePath(message) => write!(formatter, "unsafe cache path: {message}"),
            Self::OwnershipMissing(_) => {
                formatter.write_str("cache root is not initialized or is not owned by this tool")
            }
            Self::InvalidOwnershipMarker(_) => {
                formatter.write_str("cache ownership marker is incomplete or invalid")
            }
            Self::SymlinkInManagedRoot(_) => {
                formatter.write_str("symbolic link found inside managed cache root")
            }
            Self::UnexpectedEntry(_) => {
                formatter.write_str("unexpected object found inside managed cache root")
            }
            Self::InventoryLimitExceeded => {
                formatter.write_str("cache inventory exceeded its bounded node limit")
            }
            Self::InvalidBudget => {
                formatter.write_str("cache disk budget must be greater than zero")
            }
            Self::InvalidDigest => formatter.write_str("cache key digest is invalid"),
            Self::GenerationMismatch => {
                formatter.write_str("cache generation manifest does not match the prepared entry")
            }
            Self::LockBusy(path) => write!(formatter, "cache lock is busy: {}", path.display()),
            Self::PromotionUncertain => {
                formatter.write_str("cache generation promotion could not be rolled back safely")
            }
            Self::SizeOverflow => formatter.write_str("cache size accounting overflowed"),
            Self::Canonical(_) => formatter.write_str("cache key could not be canonicalized"),
            Self::Io(_) => formatter.write_str("cache filesystem operation failed"),
        }
    }
}

impl std::error::Error for CacheError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Canonical(source) => Some(source),
            Self::Io(source) => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigV1;
    use std::sync::{Arc, Barrier};

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
            .join(format!(".ccp-cache-test-{}-{name}", std::process::id()))
    }

    fn clean(path: &Path) {
        if path.exists() {
            fs::remove_dir_all(path).expect("remove owned test directory");
        }
    }

    fn repository(name: &str) -> PathBuf {
        let path = test_root(&format!("repo-{name}"));
        clean(&path);
        fs::create_dir_all(&path).expect("repository fixture");
        path
    }

    fn options(candidate: PathBuf) -> CacheRootOptions {
        CacheRootOptions {
            explicit: Some(candidate),
            environment: None,
            home: None,
            xdg_cache_home: None,
            local_app_data: None,
            platform: PlatformFamily::Unix,
        }
    }

    fn resolved_fixture(name: &str) -> (PathBuf, ResolvedCacheRoot) {
        let repo = repository(name);
        let root = repo
            .parent()
            .expect("test parent")
            .join(format!("persistent-{name}"));
        clean(&root);
        let resolved = ResolvedCacheRoot::resolve(&repo, &options(root)).expect("resolve");
        (repo, resolved)
    }

    fn envelope() -> ExecutionPlanEnvelopeV1 {
        ConfigV1::parse(
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
"#,
        )
        .expect("config")
        .into_plan()
        .expect("plan")
    }

    #[test]
    fn explicit_precedes_environment_and_platform_default() {
        let repo = repository("precedence");
        let parent = repo.parent().expect("parent");
        let explicit = parent.join("explicit-cache");
        let environment = parent.join("environment-cache");
        let resolved = ResolvedCacheRoot::resolve(
            &repo,
            &CacheRootOptions {
                explicit: Some(explicit.clone()),
                environment: Some(environment.into_os_string()),
                home: Some(parent.to_path_buf()),
                xdg_cache_home: None,
                local_app_data: None,
                platform: PlatformFamily::MacOs,
            },
        )
        .expect("resolve");

        assert_eq!(resolved.path, explicit);
        assert_eq!(resolved.source, CacheRootSource::Explicit);
        clean(&repo);
    }

    #[test]
    fn platform_defaults_use_the_versioned_build_cache_namespace() {
        let base = PathBuf::from("/persistent/operator");
        let mut options = CacheRootOptions {
            explicit: None,
            environment: None,
            home: Some(base.clone()),
            xdg_cache_home: Some(base.join("xdg")),
            local_app_data: Some(base.join("local-app-data")),
            platform: PlatformFamily::MacOs,
        };

        assert_eq!(
            platform_default(&options).expect("macOS default"),
            base.join("Library")
                .join("Caches")
                .join(DEFAULT_CACHE_ROOT_NAME)
        );
        options.platform = PlatformFamily::Windows;
        assert_eq!(
            platform_default(&options).expect("Windows default"),
            base.join("local-app-data").join(DEFAULT_CACHE_ROOT_NAME)
        );
        options.platform = PlatformFamily::Unix;
        assert_eq!(
            platform_default(&options).expect("Unix default"),
            base.join("xdg").join(DEFAULT_CACHE_ROOT_NAME)
        );
    }

    #[test]
    fn versioned_default_leaves_the_legacy_admission_tree_untouched() {
        let repo = repository("legacy-admission-default");
        let parent = repo.parent().expect("test parent");
        let home = parent.join("legacy-admission-home");
        let legacy = home
            .join("Library")
            .join("Caches")
            .join("commit-ci-preflight");
        let admission = legacy.join("admission");
        clean(&home);
        fs::create_dir_all(&admission).expect("legacy admission fixture");
        fs::write(admission.join("next-ticket-v1"), b"1\n").expect("legacy counter");
        fs::write(admission.join("queue.lock"), b"").expect("legacy queue lock");
        fs::write(admission.join("slot.lock"), b"").expect("legacy slot lock");

        let resolved = ResolvedCacheRoot::resolve(
            &repo,
            &CacheRootOptions {
                explicit: None,
                environment: None,
                home: Some(home.clone()),
                xdg_cache_home: None,
                local_app_data: None,
                platform: PlatformFamily::MacOs,
            },
        )
        .expect("resolve versioned default");
        assert_eq!(
            resolved.path,
            home.join("Library")
                .join("Caches")
                .join(DEFAULT_CACHE_ROOT_NAME)
        );

        ManagedCache::initialize(resolved.clone()).expect("initialize versioned default");
        assert!(resolved.path.join(OWNER_FILE).is_file());
        assert!(!legacy.join(OWNER_FILE).exists());
        assert_eq!(
            fs::read(admission.join("next-ticket-v1")).expect("legacy counter remains"),
            b"1\n"
        );
        assert!(admission.join("queue.lock").is_file());
        assert!(admission.join("slot.lock").is_file());

        clean(&home);
        clean(&repo);
    }

    #[test]
    fn temporary_repository_relative_and_unresolved_paths_are_rejected() {
        let repo = repository("unsafe");
        for candidate in [
            std::env::temp_dir().join("ccp-forbidden"),
            repo.join("cache"),
            PathBuf::from("$HOME/cache"),
            PathBuf::from("relative/cache"),
        ] {
            assert!(matches!(
                ResolvedCacheRoot::resolve(&repo, &options(candidate)),
                Err(CacheError::UnsafePath(_))
            ));
        }
        clean(&repo);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_components_are_rejected() {
        use std::os::unix::fs::symlink;

        let repo = repository("symlink");
        let parent = repo.parent().expect("parent");
        let real = parent.join("real-cache-parent");
        let link = parent.join("cache-link");
        clean(&real);
        let _ = fs::remove_file(&link);
        fs::create_dir_all(&real).expect("real parent");
        symlink(&real, &link).expect("symlink");

        assert!(matches!(
            ResolvedCacheRoot::resolve(&repo, &options(link.join("cache"))),
            Err(CacheError::UnsafePath(_))
        ));
        fs::remove_file(link).expect("remove owned symlink");
        clean(&real);
        clean(&repo);
    }

    #[test]
    fn ownership_marker_survives_reopen_and_rejects_interrupted_bytes() {
        let (repo, resolved) = resolved_fixture("ownership");
        let cache = ManagedCache::initialize(resolved.clone()).expect("initialize");
        assert_eq!(cache.root(), &resolved);
        ManagedCache::open(resolved.clone()).expect("reopen after simulated reboot");

        fs::write(resolved.path.join(OWNER_FILE), b"partial").expect("corrupt marker fixture");
        assert!(matches!(
            ManagedCache::open(resolved.clone()),
            Err(CacheError::InvalidOwnershipMarker(_))
        ));
        clean(&resolved.path);
        clean(&repo);
    }

    #[test]
    fn concurrent_initializers_converge_on_one_owner_marker() {
        let (repo, resolved) = resolved_fixture("concurrent");
        let barrier = Arc::new(Barrier::new(3));
        let mut handles = Vec::new();
        for _ in 0..2 {
            let barrier = Arc::clone(&barrier);
            let resolved = resolved.clone();
            handles.push(thread::spawn(move || {
                barrier.wait();
                ManagedCache::initialize(resolved)
            }));
        }
        barrier.wait();
        for handle in handles {
            handle
                .join()
                .expect("initializer thread")
                .expect("initialize");
        }
        assert_eq!(
            fs::read(resolved.path.join(OWNER_FILE)).expect("marker"),
            OWNER_BYTES
        );
        clean(&resolved.path);
        clean(&repo);
    }

    #[test]
    fn cache_keys_are_stable_and_bound_to_cache_identity() {
        let envelope = envelope();
        let first = CacheKey::for_plan_cache(&envelope, &envelope.plan.caches[0]).expect("key");
        let second = CacheKey::for_plan_cache(&envelope, &envelope.plan.caches[0]).expect("key");
        assert_eq!(first, second);
        assert!(first.digest().starts_with("sha256:"));
        assert!(first.directory_name().starts_with("sha256-"));
        assert_eq!(first.directory_name().len(), 71);
    }

    #[test]
    fn inventory_and_cleanup_are_bounded_truthful_and_read_only() {
        let (repo, resolved) = resolved_fixture("inventory");
        let cache = ManagedCache::initialize(resolved.clone())
            .expect("initialize")
            .with_disk_budget(1)
            .expect("budget");
        let key = CacheKey::for_plan_cache(&envelope(), &envelope().plan.caches[0]).expect("key");
        let complete = cache.entry_path(&key);
        fs::create_dir_all(complete.join("data")).expect("complete data");
        fs::write(complete.join("data/value"), b"cache").expect("cache data");
        fs::write(complete.join(COMPLETE_FILE), COMPLETE_BYTES).expect("complete marker");
        let incomplete = resolved
            .path
            .join(ENTRIES_DIR)
            .join(".entry-tmp-interrupted");
        fs::create_dir_all(&incomplete).expect("incomplete entry");
        fs::write(incomplete.join("partial"), b"partial").expect("partial data");

        let inventory = cache.inventory().expect("inventory");
        assert_eq!(inventory.entries.len(), 2);
        assert!(inventory.budget_exceeded);
        let cleanup = cache.cleanup_dry_run().expect("cleanup plan");
        assert!(!cleanup.deletion_performed);
        assert_eq!(cleanup.candidates.len(), 1);
        assert!(incomplete.exists());
        assert!(complete.exists());
        clean(&resolved.path);
        clean(&repo);
    }

    #[cfg(unix)]
    #[test]
    fn inventory_never_follows_symlinks() {
        use std::os::unix::fs::symlink;

        let (repo, resolved) = resolved_fixture("inventory-symlink");
        let cache = ManagedCache::initialize(resolved.clone()).expect("initialize");
        let key = CacheKey::for_plan_cache(&envelope(), &envelope().plan.caches[0]).expect("key");
        let entry = cache.entry_path(&key);
        fs::create_dir_all(&entry).expect("entry");
        symlink(&repo, entry.join("escape")).expect("escape symlink");

        assert!(matches!(
            cache.inventory(),
            Err(CacheError::SymlinkInManagedRoot(_))
        ));
        clean(&resolved.path);
        clean(&repo);
    }

    #[cfg(unix)]
    #[test]
    fn ownership_marker_symlink_is_rejected() {
        use std::os::unix::fs::symlink;

        let (repo, resolved) = resolved_fixture("owner-symlink");
        let cache = ManagedCache::initialize(resolved.clone()).expect("initialize");
        let external = repo.join("external-owner");
        fs::write(&external, OWNER_BYTES).expect("external marker");
        fs::remove_file(cache.root.path.join(OWNER_FILE)).expect("remove owned marker");
        symlink(&external, cache.root.path.join(OWNER_FILE)).expect("marker symlink");

        assert!(matches!(
            ManagedCache::open(resolved.clone()),
            Err(CacheError::SymlinkInManagedRoot(_))
        ));
        clean(&resolved.path);
        clean(&repo);
    }

    #[cfg(unix)]
    #[test]
    fn managed_directory_symlink_is_rejected() {
        use std::os::unix::fs::symlink;

        let (repo, resolved) = resolved_fixture("directory-symlink");
        ManagedCache::initialize(resolved.clone()).expect("initialize");
        let entries = resolved.path.join(ENTRIES_DIR);
        fs::remove_dir(&entries).expect("remove empty entries directory");
        symlink(&repo, &entries).expect("entries symlink");

        assert!(matches!(
            ManagedCache::open(resolved.clone()),
            Err(CacheError::SymlinkInManagedRoot(_))
        ));
        clean(&resolved.path);
        clean(&repo);
    }

    #[test]
    fn prepared_entry_is_incomplete_until_atomically_marked() {
        let (repo, resolved) = resolved_fixture("prepare-entry");
        let cache = ManagedCache::initialize(resolved.clone()).expect("initialize");
        let envelope = envelope();
        let key = CacheKey::for_plan_cache(&envelope, &envelope.plan.caches[0]).expect("key");

        let prepared = cache
            .prepare_entry(&key, &envelope.plan_digest, 1)
            .expect("prepare");
        assert!(!prepared.was_complete);
        assert!(prepared.data_path.is_dir());
        cache.promote_entry(&prepared).expect("promote");
        assert_eq!(
            read_generation_manifest(&cache.entry_path(&key).join(GENERATION_MANIFEST_FILE))
                .expect("generation manifest")
                .state,
            "complete"
        );
        drop(prepared);
        assert!(
            cache
                .prepare_entry(&key, &envelope.plan_digest, 2)
                .expect("reopen")
                .was_complete
        );

        clean(&resolved.path);
        clean(&repo);
    }

    #[test]
    fn failed_generation_does_not_mutate_last_known_good() {
        let (repo, resolved) = resolved_fixture("complete-entry-mutation");
        let cache = ManagedCache::initialize(resolved.clone()).expect("initialize");
        let envelope = envelope();
        let key = CacheKey::for_plan_cache(&envelope, &envelope.plan.caches[0]).expect("key");
        let prepared = cache
            .prepare_entry(&key, &envelope.plan_digest, 1)
            .expect("prepare");
        let payload = prepared.data_path.join("payload.bin");
        fs::write(&payload, b"known-good").expect("write known-good payload");
        cache.promote_entry(&prepared).expect("promote known-good");
        drop(prepared);

        let failed = cache
            .prepare_entry(&key, &envelope.plan_digest, 2)
            .expect("prepare failed generation");
        fs::write(failed.data_path.join("payload.bin"), b"failed-run-mutation")
            .expect("simulate failed run mutation");
        let failed_staging = failed.staging_path.clone();
        drop(failed);

        assert_eq!(
            fs::read(cache.entry_data_path(&key).join("payload.bin")).expect("read current"),
            b"known-good"
        );
        assert!(!failed_staging.exists());
        let inventory = cache.inventory().expect("inventory");
        assert_eq!(inventory.entries.len(), 1);
        assert_eq!(inventory.entries[0].status, CacheEntryStatus::Complete);

        clean(&resolved.path);
        clean(&repo);
    }

    #[test]
    fn active_entry_lock_blocks_a_second_preparation_until_release() {
        let (repo, resolved) = resolved_fixture("entry-lock");
        let cache = ManagedCache::initialize(resolved.clone()).expect("initialize");
        let envelope = envelope();
        let key = CacheKey::for_plan_cache(&envelope, &envelope.plan.caches[0]).expect("key");

        let first = cache
            .prepare_entry(&key, &envelope.plan_digest, 1)
            .expect("first preparation");
        assert!(matches!(
            cache.prepare_entry(&key, &envelope.plan_digest, 2),
            Err(CacheError::LockBusy(_))
        ));
        drop(first);
        let second = cache
            .prepare_entry(&key, &envelope.plan_digest, 2)
            .expect("preparation after release");
        drop(second);

        clean(&resolved.path);
        clean(&repo);
    }
}
