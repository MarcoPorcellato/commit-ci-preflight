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
#[cfg(target_os = "macos")]
use std::ffi::CString;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicU8, AtomicU64, Ordering},
};
use std::thread;
use std::time::Duration;

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cache_payload::{copy_payload_tree, validate_payload_tree};
use crate::config::{ExecutionPlanEnvelopeV1, NormalizedCache};
use crate::durable_fs::{DurableFileSystem, DurableFsError};
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
const PROMOTION_JOURNAL_FILE: &str = ".promotion-journal-v1.json";
const PROMOTION_SCHEMA_VERSION: &str = "1.0";
const MAX_INVENTORY_NODES: usize = 100_000;
const INIT_RETRIES: usize = 40;
const INIT_RETRY_DELAY: Duration = Duration::from_millis(5);
const PREPARED_PHASE_PREPARING: u8 = 0;
const PREPARED_PHASE_STAGING: u8 = 1;
const PREPARED_PHASE_PROMOTED: u8 = 2;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn clonefile(
        source: *const std::os::raw::c_char,
        destination: *const std::os::raw::c_char,
        flags: u32,
    ) -> i32;
}

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

    /// Pins completed cache entries for the lifetime of the returned guards.
    ///
    /// Sources are deliberately accepted only in their canonical, exact
    /// `entries/sha256-<digest>/data` form.  This keeps the lock ownership
    /// bound to the entry which is actually going to be mounted.
    pub fn pin_completed_sources(
        &self,
        sources: &[PathBuf],
    ) -> Result<Vec<CacheUsePin>, CacheError> {
        validate_owner_marker(&self.root.path.join(OWNER_FILE))?;
        validate_plain_directory(&self.root.path.join(ENTRIES_DIR))?;
        validate_plain_directory(&self.root.path.join(WORKSPACES_DIR))?;

        let mut ordered = BTreeSet::new();
        for source in sources {
            ordered.insert(validate_pin_source_path(&self.root.path, source)?);
        }

        let mut pins = Vec::with_capacity(ordered.len());
        for source in ordered {
            let entry_path = source
                .parent()
                .ok_or(CacheError::UnsafePath("cache source has no entry parent"))?
                .to_path_buf();
            let lock_path = entry_path.join(ENTRY_LOCK_FILE);
            match fs::symlink_metadata(&lock_path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(CacheError::SymlinkInManagedRoot(lock_path));
                }
                Ok(metadata) if !metadata.is_file() => {
                    return Err(CacheError::UnexpectedEntry(lock_path));
                }
                Ok(_) => {}
                Err(error) => return Err(CacheError::Io(error)),
            }
            let entry_lock = acquire_existing_entry_lock(&entry_path)?;
            let pin = CacheUsePin::from_locked_source(self, source, entry_lock)?;
            pins.push(pin);
        }
        Ok(pins)
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
        let owner = Arc::new(PreparedCacheGenerationOwner {
            entry_path: path.clone(),
            staging_path: staging_path.clone(),
            key_digest: key.digest.clone(),
            plan_digest: plan_digest.to_owned(),
            generation,
            phase: AtomicU8::new(PREPARED_PHASE_PREPARING),
            _entry_lock: entry_lock,
        });
        if complete {
            let source = path.join("data");
            remove_if_present(&data_path)?;
            if !try_clone_tree(&source, &data_path)? {
                let mut nodes = 0;
                copy_payload_tree(&source, &data_path, &mut nodes, MAX_INVENTORY_NODES)?;
            }
        }
        let manifest = CacheGenerationManifestV1 {
            schema_version: GENERATION_SCHEMA_VERSION.to_owned(),
            key_digest: key.digest.clone(),
            plan_digest: plan_digest.to_owned(),
            generation,
            state: "staging".to_owned(),
        };
        write_generation_manifest(&staging_path, &manifest)?;
        owner.phase.store(PREPARED_PHASE_STAGING, Ordering::Release);
        Ok(PreparedCacheEntry {
            path,
            data_path,
            staging_path,
            key_digest: key.digest.clone(),
            plan_digest: plan_digest.to_owned(),
            generation,
            _generation_owner: owner,
            was_complete: complete,
        })
    }

    pub fn promote_entry(
        &self,
        prepared: &PreparedCacheEntry,
    ) -> Result<CachePromotionOutcome, CacheError> {
        self.promote_entries(std::slice::from_ref(prepared))
    }

    pub fn promote_entries(
        &self,
        prepared: &[PreparedCacheEntry],
    ) -> Result<CachePromotionOutcome, CacheError> {
        if prepared.is_empty() {
            return Ok(CachePromotionOutcome::NotAttempted);
        }
        validate_owner_marker(&self.root.path.join(OWNER_FILE))?;
        let _promotion_lock = acquire_promotion_lock(&self.root.path)?;
        self.recover_promotion_locked()?;
        let journal = self.create_promotion_journal(prepared)?;
        self.execute_promotion(prepared, &journal)?;
        self.finalize_promotion(&journal)?;
        Ok(CachePromotionOutcome::Promoted)
    }

    fn create_promotion_journal(
        &self,
        prepared: &[PreparedCacheEntry],
    ) -> Result<CachePromotionJournalV1, CacheError> {
        let mut entries = Vec::with_capacity(prepared.len());
        let mut paths = BTreeSet::new();
        for entry in prepared {
            if !paths.insert(entry.path.clone()) {
                return Err(CacheError::GenerationMismatch);
            }
            self.validate_prepared_entry(entry)?;
            let staging_name = entry
                .staging_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or(CacheError::GenerationMismatch)?
                .to_owned();
            validate_owned_name(&staging_name, ".staging-")?;
            if entry.staging_path.parent() != Some(entry.path.as_path()) {
                return Err(CacheError::GenerationMismatch);
            }
            let current = entry.path.join("data");
            let had_current = match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(CacheError::SymlinkInManagedRoot(current));
                }
                Ok(metadata) if metadata.is_dir() => true,
                Ok(_) => return Err(CacheError::UnexpectedEntry(current)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => false,
                Err(error) => return Err(CacheError::Io(error)),
            };
            let backup_name = format!(
                ".backup-{}-{}",
                std::process::id(),
                TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            );
            if entry.path.join(&backup_name).exists() {
                return Err(CacheError::PromotionUncertain);
            }
            entries.push(CachePromotionJournalEntryV1 {
                key_digest: entry.key_digest.clone(),
                plan_digest: entry.plan_digest.clone(),
                generation: entry.generation,
                staging_name,
                backup_name,
                had_current,
                previous_marker: read_optional_file(&entry.path.join(COMPLETE_FILE))?,
                previous_manifest: read_optional_file(&entry.path.join(GENERATION_MANIFEST_FILE))?,
            });
        }
        let journal = CachePromotionJournalV1 {
            schema_version: PROMOTION_SCHEMA_VERSION.to_owned(),
            operation_id: format!(
                "{}-{}",
                std::process::id(),
                TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ),
            state: "prepared".to_owned(),
            entries,
        };
        write_promotion_journal(&self.root.path, &journal)?;
        Ok(journal)
    }

    fn validate_prepared_entry(&self, prepared: &PreparedCacheEntry) -> Result<(), CacheError> {
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
        let mut nodes = 0;
        validate_payload_tree(&prepared.data_path, &mut nodes, MAX_INVENTORY_NODES)?;
        Ok(())
    }

    fn execute_promotion(
        &self,
        prepared: &[PreparedCacheEntry],
        journal: &CachePromotionJournalV1,
    ) -> Result<(), CacheError> {
        for (prepared, journal_entry) in prepared.iter().zip(&journal.entries) {
            let manifest_path = prepared.staging_path.join(GENERATION_MANIFEST_FILE);
            let mut complete_manifest = read_generation_manifest(&manifest_path)?;
            complete_manifest.state = "complete".to_owned();
            write_generation_manifest_replacing(&manifest_path, &complete_manifest)?;
            let current = prepared.path.join("data");
            let backup = prepared.path.join(&journal_entry.backup_name);
            let marker = prepared.path.join(COMPLETE_FILE);
            let previous_manifest_path = prepared.path.join(GENERATION_MANIFEST_FILE);
            if journal_entry.had_current {
                fs::rename(&current, &backup).map_err(CacheError::Io)?;
            }
            remove_if_present(&marker)?;
            remove_if_present(&previous_manifest_path)?;
            fs::rename(&prepared.data_path, &current).map_err(CacheError::Io)?;
            fs::rename(&manifest_path, &previous_manifest_path).map_err(CacheError::Io)?;
            write_complete_marker(&marker)?;
            prepared
                ._generation_owner
                .phase
                .store(PREPARED_PHASE_PROMOTED, Ordering::Release);
        }
        Ok(())
    }

    fn finalize_promotion(&self, journal: &CachePromotionJournalV1) -> Result<(), CacheError> {
        for entry in &journal.entries {
            let path = self.entry_path(&journal_key(entry)?);
            validate_plain_directory(&path)?;
            remove_if_present(&path.join(&entry.backup_name))?;
            remove_if_present(&path.join(&entry.staging_name))?;
        }
        remove_if_present(&self.root.path.join(PROMOTION_JOURNAL_FILE))
    }

    fn recover_promotion_locked(&self) -> Result<(), CacheError> {
        let path = self.root.path.join(PROMOTION_JOURNAL_FILE);
        let Some(bytes) = read_optional_file(&path)? else {
            return Ok(());
        };
        if bytes.len() > 1_048_576 {
            return Err(CacheError::PromotionUncertain);
        }
        let journal: CachePromotionJournalV1 = serde_json::from_slice(&bytes).map_err(|error| {
            CacheError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                error.to_string(),
            ))
        })?;
        if journal.schema_version != PROMOTION_SCHEMA_VERSION
            || journal.state != "prepared"
            || journal.entries.is_empty()
            || journal.operation_id.is_empty()
        {
            return Err(CacheError::PromotionUncertain);
        }
        let mut locks = Vec::with_capacity(journal.entries.len());
        for entry in &journal.entries {
            validate_owned_name(&entry.staging_name, ".staging-")?;
            validate_owned_name(&entry.backup_name, ".backup-")?;
            let path = self.entry_path(&journal_key(entry)?);
            validate_plain_directory(&path)?;
            locks.push(acquire_entry_lock(&path)?);
        }
        for entry in &journal.entries {
            let path = self.entry_path(&journal_key(entry)?);
            let current = path.join("data");
            let marker = path.join(COMPLETE_FILE);
            let manifest = path.join(GENERATION_MANIFEST_FILE);
            let backup = path.join(&entry.backup_name);
            let staging = path.join(&entry.staging_name);
            if current_generation_is_complete(&current, &marker, &manifest, entry)? {
                remove_if_present(&backup)?;
                remove_if_present(&staging)?;
                continue;
            }
            if entry.had_current {
                if backup.exists() {
                    remove_if_present(&current)?;
                    remove_if_present(&marker)?;
                    remove_if_present(&manifest)?;
                    fs::rename(&backup, &current).map_err(CacheError::Io)?;
                    restore_optional_file(&manifest, entry.previous_manifest.as_deref())?;
                    restore_optional_file(&marker, entry.previous_marker.as_deref())?;
                    remove_if_present(&staging)?;
                } else if current_matches_previous(&current, &marker, &manifest, entry)? {
                    remove_if_present(&staging)?;
                } else {
                    return Err(CacheError::PromotionUncertain);
                }
            } else if !backup.exists() && !current.exists() {
                remove_if_present(&marker)?;
                remove_if_present(&manifest)?;
                remove_if_present(&staging)?;
            } else {
                return Err(CacheError::PromotionUncertain);
            }
        }
        drop(locks);
        remove_if_present(&path)
    }

    pub fn mark_entry_complete(&self, key: &CacheKey) -> Result<(), CacheError> {
        validate_owner_marker(&self.root.path.join(OWNER_FILE))?;
        let entry = self.entry_path(key);
        validate_plain_directory(&entry)?;
        validate_plain_directory(&entry.join("data"))?;
        let mut nodes = 0;
        validate_payload_tree(&entry.join("data"), &mut nodes, MAX_INVENTORY_NODES)?;
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
            let (bytes, files) = bounded_entry_size(&entry.path(), &mut nodes)?;
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

#[derive(Debug)]
pub struct CacheUsePin {
    entry_path: PathBuf,
    data_path: PathBuf,
    key_digest: String,
    plan_digest: String,
    generation: u64,
    _entry_lock: Arc<File>,
}

impl CacheUsePin {
    fn from_locked_source(
        cache: &ManagedCache,
        source: PathBuf,
        entry_lock: Arc<File>,
    ) -> Result<Self, CacheError> {
        let entry_path = source
            .parent()
            .ok_or(CacheError::UnsafePath("cache source has no entry parent"))?
            .to_path_buf();
        let manifest = validate_completed_entry(&cache.root.path, &source, &entry_path)?;
        Ok(Self {
            key_digest: manifest.key_digest,
            plan_digest: manifest.plan_digest,
            generation: manifest.generation,
            entry_path,
            data_path: source,
            _entry_lock: entry_lock,
        })
    }

    pub fn revalidate(&self) -> Result<(), CacheError> {
        let root = self
            .entry_path
            .parent()
            .and_then(Path::parent)
            .ok_or(CacheError::UnsafePath("cache pin has no managed root"))?;
        validate_owner_marker(&root.join(OWNER_FILE))?;
        validate_plain_directory(&root.join(ENTRIES_DIR))?;
        validate_plain_directory(&root.join(WORKSPACES_DIR))?;
        let manifest = validate_completed_entry(root, &self.data_path, &self.entry_path)?;
        if manifest.key_digest != self.key_digest
            || manifest.plan_digest != self.plan_digest
            || manifest.generation != self.generation
        {
            return Err(CacheError::GenerationMismatch);
        }
        Ok(())
    }
}

fn validate_pin_source_path(root: &Path, source: &Path) -> Result<PathBuf, CacheError> {
    if !source.is_absolute() {
        return Err(CacheError::UnsafePath("cache source must be absolute"));
    }
    reject_lexical_escape(source)?;
    reject_symlink_components(source)?;
    let canonical = fs::canonicalize(source).map_err(CacheError::Io)?;
    if canonical != source {
        return Err(CacheError::UnsafePath("cache source must be canonical"));
    }
    let relative = source
        .strip_prefix(root)
        .map_err(|_| CacheError::UnsafePath("cache source is outside managed root"))?;
    let components: Vec<_> = relative.components().collect();
    if components.len() != 3
        || components[0].as_os_str() != ENTRIES_DIR
        || components[2].as_os_str() != "data"
    {
        return Err(CacheError::UnsafePath(
            "cache source must be an entry data path",
        ));
    }
    let key_name = components[1]
        .as_os_str()
        .to_str()
        .ok_or(CacheError::InvalidDigest)?;
    let key_hex = key_name
        .strip_prefix("sha256-")
        .ok_or(CacheError::InvalidDigest)?;
    if key_hex.len() != 64
        || !key_hex
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(CacheError::InvalidDigest);
    }
    Ok(source.to_path_buf())
}

fn validate_completed_entry(
    root: &Path,
    data_path: &Path,
    entry_path: &Path,
) -> Result<CacheGenerationManifestV1, CacheError> {
    validate_pin_source_path(root, data_path)?;
    validate_plain_directory(entry_path)?;
    validate_plain_directory(data_path)?;
    let marker = entry_path.join(COMPLETE_FILE);
    match fs::symlink_metadata(&marker) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(CacheError::SymlinkInManagedRoot(marker));
        }
        Ok(metadata) if !metadata.is_file() => return Err(CacheError::UnexpectedEntry(marker)),
        Err(error) => return Err(CacheError::Io(error)),
        Ok(_) => {}
    }
    if fs::read(&marker).map_err(CacheError::Io)? != COMPLETE_BYTES {
        return Err(CacheError::GenerationMismatch);
    }
    let manifest_path = entry_path.join(GENERATION_MANIFEST_FILE);
    match fs::symlink_metadata(&manifest_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(CacheError::SymlinkInManagedRoot(manifest_path));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(CacheError::UnexpectedEntry(manifest_path));
        }
        Err(error) => return Err(CacheError::Io(error)),
        Ok(_) => {}
    }
    let manifest = read_generation_manifest(&manifest_path)?;
    let key_name = entry_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(CacheError::InvalidDigest)?;
    let key_digest = format!("sha256:{}", &key_name[7..]);
    validated_digest_hex(&manifest.plan_digest)?;
    if manifest.schema_version != GENERATION_SCHEMA_VERSION
        || manifest.key_digest != key_digest
        || manifest.state != "complete"
    {
        return Err(CacheError::GenerationMismatch);
    }
    Ok(manifest)
}

#[derive(Debug, Clone)]
pub struct PreparedCacheEntry {
    pub path: PathBuf,
    pub data_path: PathBuf,
    staging_path: PathBuf,
    key_digest: String,
    plan_digest: String,
    generation: u64,
    _generation_owner: Arc<PreparedCacheGenerationOwner>,
    pub was_complete: bool,
}

#[derive(Debug)]
struct PreparedCacheGenerationOwner {
    entry_path: PathBuf,
    staging_path: PathBuf,
    key_digest: String,
    plan_digest: String,
    generation: u64,
    phase: AtomicU8,
    _entry_lock: Arc<File>,
}

impl Drop for PreparedCacheGenerationOwner {
    fn drop(&mut self) {
        let phase = self.phase.load(Ordering::Acquire);
        if phase == PREPARED_PHASE_PROMOTED {
            return;
        }
        if phase != PREPARED_PHASE_PREPARING && phase != PREPARED_PHASE_STAGING {
            return;
        }
        if validate_owned_staging_root(&self.entry_path, &self.staging_path).is_err() {
            return;
        }
        if phase == PREPARED_PHASE_STAGING {
            let Ok(manifest) =
                read_generation_manifest(&self.staging_path.join(GENERATION_MANIFEST_FILE))
            else {
                return;
            };
            if manifest.schema_version != GENERATION_SCHEMA_VERSION
                || manifest.key_digest != self.key_digest
                || manifest.plan_digest != self.plan_digest
                || manifest.generation != self.generation
                || manifest.state != "staging"
            {
                return;
            }
        }
        let _ = remove_owned_generation_directory(&self.staging_path);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CacheGenerationExpectation {
    pub key_digest: String,
    pub plan_digest: String,
    pub generation: u64,
    pub state: &'static str,
}

impl PreparedCacheEntry {
    pub(crate) fn generation_expectation(&self) -> CacheGenerationExpectation {
        CacheGenerationExpectation {
            key_digest: self.key_digest.clone(),
            plan_digest: self.plan_digest.clone(),
            generation: self.generation,
            state: "staging",
        }
    }
}

pub(crate) fn revalidate_generation_source(
    source: &Path,
    expected: &CacheGenerationExpectation,
) -> Result<(), CacheError> {
    let staging = source.parent().ok_or(CacheError::PromotionUncertain)?;
    let manifest = read_generation_manifest(&staging.join(GENERATION_MANIFEST_FILE))?;
    if manifest.schema_version != GENERATION_SCHEMA_VERSION
        || manifest.key_digest != expected.key_digest
        || manifest.plan_digest != expected.plan_digest
        || manifest.generation != expected.generation
        || manifest.state != expected.state
    {
        return Err(CacheError::PromotionUncertain);
    }
    Ok(())
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CachePromotionJournalV1 {
    schema_version: String,
    operation_id: String,
    state: String,
    entries: Vec<CachePromotionJournalEntryV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CachePromotionJournalEntryV1 {
    key_digest: String,
    plan_digest: String,
    generation: u64,
    staging_name: String,
    backup_name: String,
    had_current: bool,
    previous_marker: Option<Vec<u8>>,
    previous_manifest: Option<Vec<u8>>,
}

fn write_promotion_journal(
    root: &Path,
    journal: &CachePromotionJournalV1,
) -> Result<(), CacheError> {
    let bytes = canonical_json(journal).map_err(CacheError::Canonical)?;
    let path = root.join(PROMOTION_JOURNAL_FILE);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(CacheError::Io)?;
    file.write_all(&bytes).map_err(CacheError::Io)?;
    file.write_all(b"\n").map_err(CacheError::Io)?;
    file.sync_all().map_err(CacheError::Io)
}

fn validate_owned_name(name: &str, prefix: &str) -> Result<(), CacheError> {
    if name.len() <= prefix.len()
        || name.len() > 128
        || !name.starts_with(prefix)
        || name.contains(['/', '\\'])
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-".contains(&byte))
    {
        return Err(CacheError::GenerationMismatch);
    }
    Ok(())
}

fn journal_key(entry: &CachePromotionJournalEntryV1) -> Result<CacheKey, CacheError> {
    validated_digest_hex(&entry.key_digest)?;
    Ok(CacheKey {
        digest: entry.key_digest.clone(),
    })
}

fn current_generation_is_complete(
    current: &Path,
    marker: &Path,
    manifest_path: &Path,
    journal: &CachePromotionJournalEntryV1,
) -> Result<bool, CacheError> {
    match fs::symlink_metadata(current) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(CacheError::SymlinkInManagedRoot(current.to_path_buf()));
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(CacheError::UnexpectedEntry(current.to_path_buf()));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(CacheError::Io(error)),
        Ok(_) => {}
    }
    if read_optional_file(marker)?.as_deref() != Some(COMPLETE_BYTES) {
        return Ok(false);
    }
    let Some(bytes) = read_optional_file(manifest_path)? else {
        return Ok(false);
    };
    let manifest: CacheGenerationManifestV1 = serde_json::from_slice(&bytes).map_err(|error| {
        CacheError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            error.to_string(),
        ))
    })?;
    Ok(manifest.schema_version == GENERATION_SCHEMA_VERSION
        && manifest.key_digest == journal.key_digest
        && manifest.plan_digest == journal.plan_digest
        && manifest.generation == journal.generation
        && manifest.state == "complete")
}

fn current_matches_previous(
    current: &Path,
    marker: &Path,
    manifest: &Path,
    journal: &CachePromotionJournalEntryV1,
) -> Result<bool, CacheError> {
    match fs::symlink_metadata(current) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(CacheError::SymlinkInManagedRoot(current.to_path_buf()));
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(CacheError::UnexpectedEntry(current.to_path_buf()));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(CacheError::Io(error)),
        Ok(_) => {}
    }
    Ok(read_optional_file(marker)? == journal.previous_marker
        && read_optional_file(manifest)? == journal.previous_manifest)
}

fn restore_optional_file(path: &Path, bytes: Option<&[u8]>) -> Result<(), CacheError> {
    remove_if_present(path)?;
    let Some(bytes) = bytes else {
        return Ok(());
    };
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(CacheError::Io)?;
    file.write_all(bytes).map_err(CacheError::Io)?;
    file.sync_all().map_err(CacheError::Io)
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
    let bytes = canonical_json(manifest).map_err(CacheError::Canonical)?;
    let mut durable_bytes = bytes;
    durable_bytes.push(b'\n');
    #[cfg(unix)]
    {
        DurableFileSystem::default()
            .atomic_replace(path, &durable_bytes)
            .map_err(map_durable_manifest_error)
    }
    #[cfg(not(unix))]
    {
        let _ = durable_bytes;
        remove_if_present(path)?;
        return write_generation_manifest(
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
        });
    }
}

fn map_durable_manifest_error(error: DurableFsError) -> CacheError {
    match error {
        DurableFsError::Io(error) => CacheError::Io(error),
        DurableFsError::UnsafePath(_)
        | DurableFsError::OwnershipMismatch
        | DurableFsError::AtomicReplacementUnavailable => CacheError::PromotionUncertain,
    }
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

fn remove_owned_generation_directory(path: &Path) -> Result<(), CacheError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(CacheError::SymlinkInManagedRoot(path.to_path_buf()))
        }
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path).map_err(CacheError::Io),
        Ok(_) => Err(CacheError::UnexpectedEntry(path.to_path_buf())),
        Err(error) => Err(CacheError::Io(error)),
    }
}

fn validate_owned_staging_root(entry_path: &Path, staging_path: &Path) -> Result<(), CacheError> {
    if staging_path.parent() != Some(entry_path) {
        return Err(CacheError::GenerationMismatch);
    }
    let name = staging_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(CacheError::GenerationMismatch)?;
    validate_owned_name(name, ".staging-")?;
    validate_plain_directory(staging_path)
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

fn try_clone_tree(source: &Path, destination: &Path) -> Result<bool, CacheError> {
    let mut nodes = 0;
    validate_payload_tree(source, &mut nodes, MAX_INVENTORY_NODES)?;
    #[cfg(target_os = "macos")]
    {
        let source = CString::new(
            source
                .to_str()
                .ok_or(CacheError::UnsafePath("cache path must be UTF-8"))?,
        )
        .map_err(|_| CacheError::UnsafePath("cache path contains NUL"))?;
        let destination_c = CString::new(
            destination
                .to_str()
                .ok_or(CacheError::UnsafePath("cache path must be UTF-8"))?,
        )
        .map_err(|_| CacheError::UnsafePath("cache path contains NUL"))?;
        // clonefile is an optimization only. Unsupported filesystems fall
        // back to the deterministic link-preserving copy path below.
        let result = unsafe { clonefile(source.as_ptr(), destination_c.as_ptr(), 0) };
        if result == 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        if matches!(error.raw_os_error(), Some(18 | 22 | 45 | 95)) {
            remove_if_present(destination)?;
            return Ok(false);
        }
        Err(CacheError::Io(error))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (source, destination);
        Ok(false)
    }
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

fn acquire_existing_entry_lock(entry: &Path) -> Result<Arc<File>, CacheError> {
    acquire_advisory_lock_existing(&entry.join(ENTRY_LOCK_FILE), "cache entry")
}

fn acquire_promotion_lock(root: &Path) -> Result<Arc<File>, CacheError> {
    acquire_advisory_lock(&root.join(PROMOTION_LOCK_FILE), "cache promotion")
}

fn acquire_advisory_lock(path: &Path, label: &'static str) -> Result<Arc<File>, CacheError> {
    acquire_advisory_lock_with_mode(path, label, true)
}

fn acquire_advisory_lock_existing(
    path: &Path,
    label: &'static str,
) -> Result<Arc<File>, CacheError> {
    acquire_advisory_lock_with_mode(path, label, false)
}

fn acquire_advisory_lock_with_mode(
    path: &Path,
    label: &'static str,
    create: bool,
) -> Result<Arc<File>, CacheError> {
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
    let mut options = OpenOptions::new();
    options.read(true).write(true).truncate(false);
    if create {
        options.create(true);
    }
    let mut file = options.open(path).map_err(CacheError::Io)?;
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

fn bounded_entry_size(path: &Path, nodes: &mut usize) -> Result<(u64, u64), CacheError> {
    bounded_entry_size_at(path, path, nodes)
}

fn bounded_entry_size_at(
    entry_root: &Path,
    path: &Path,
    nodes: &mut usize,
) -> Result<(u64, u64), CacheError> {
    if is_payload_root(entry_root, path) {
        let metadata = fs::symlink_metadata(path).map_err(CacheError::Io)?;
        if metadata.file_type().is_symlink() {
            return Err(CacheError::SymlinkInManagedRoot(path.to_path_buf()));
        }
        if !metadata.is_dir() {
            return Err(CacheError::UnexpectedEntry(path.to_path_buf()));
        }
        let stats = crate::cache_payload::measure_payload_tree(path, nodes, MAX_INVENTORY_NODES)?;
        return Ok((stats.bytes, stats.files));
    }
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
        let (entry_bytes, entry_files) = bounded_entry_size_at(entry_root, &entry.path(), nodes)?;
        bytes = bytes
            .checked_add(entry_bytes)
            .ok_or(CacheError::SizeOverflow)?;
        files = files
            .checked_add(entry_files)
            .ok_or(CacheError::SizeOverflow)?;
    }
    Ok((bytes, files))
}

fn is_payload_root(entry_root: &Path, candidate: &Path) -> bool {
    let Ok(relative) = candidate.strip_prefix(entry_root) else {
        return false;
    };
    let components: Vec<_> = relative.components().collect();
    match components.as_slice() {
        [Component::Normal(data)] => *data == "data",
        [Component::Normal(generation), Component::Normal(data)] => {
            let generation = generation.to_string_lossy();
            *data == "data"
                && (generation.starts_with(".staging-") || generation.starts_with(".backup-"))
        }
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheEntryStatus {
    Complete,
    Incomplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CachePromotionOutcome {
    NotAttempted,
    Promoted,
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
    PayloadSymlinkUnsupported(PathBuf),
    PayloadSymlinkRead { path: PathBuf, source: io::Error },
    PayloadSymlinkCreate { path: PathBuf, source: io::Error },
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
            Self::PayloadSymlinkUnsupported(_)
            | Self::PayloadSymlinkRead { .. }
            | Self::PayloadSymlinkCreate { .. } => 70,
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
            Self::PayloadSymlinkUnsupported(_) => {
                formatter.write_str("cache payload symbolic links are unsupported on this platform")
            }
            Self::PayloadSymlinkRead { .. } => {
                formatter.write_str("cache payload symbolic-link target could not be read")
            }
            Self::PayloadSymlinkCreate { .. } => {
                formatter.write_str("cache payload symbolic link could not be created")
            }
        }
    }
}

impl std::error::Error for CacheError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Canonical(source) => Some(source),
            Self::Io(source) => Some(source),
            Self::PayloadSymlinkRead { source, .. } | Self::PayloadSymlinkCreate { source, .. } => {
                Some(source)
            }
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
    fn inventory_counts_payload_links_without_following_targets() {
        use std::os::unix::{ffi::OsStrExt, fs::symlink};
        let fixture = completed_entry_fixture("inventory-payload-links");
        let before = fixture.cache.inventory().unwrap().entries.remove(0);
        let outside = fixture.repo.join("inventory-sentinel");
        fs::write(&outside, b"sentinel").unwrap();
        symlink(&outside, fixture.data_path.join("external-link")).unwrap();
        let after = fixture.cache.inventory().unwrap().entries.remove(0);
        assert_eq!(after.files, before.files + 1);
        assert_eq!(
            after.bytes,
            before.bytes + outside.as_os_str().as_bytes().len() as u64
        );
        assert_eq!(fs::read(&outside).unwrap(), b"sentinel");
        finish_fixture(fixture);
    }

    #[test]
    fn inventory_counts_each_payload_root_once() {
        let (_repo, resolved) = resolved_fixture("inventory-node-count");
        let entry = resolved.path.join(ENTRIES_DIR).join("entry");
        fs::create_dir_all(entry.join("data")).unwrap();
        fs::write(entry.join("data/payload"), b"payload").unwrap();
        let mut nodes = 0;
        let stats = bounded_entry_size(&entry, &mut nodes).unwrap();
        assert_eq!(nodes, 3);
        assert_eq!(stats, (7, 1));
        clean(&resolved.path);
    }

    #[cfg(unix)]
    #[test]
    fn inventory_rejects_a_symlink_at_the_payload_root() {
        use std::os::unix::fs::symlink;
        let fixture = completed_entry_fixture("inventory-payload-root-link");
        let real = fixture.entry_path.join("real-data");
        fs::rename(&fixture.data_path, &real).unwrap();
        symlink(&real, &fixture.data_path).unwrap();
        assert!(matches!(
            fixture.cache.inventory(),
            Err(CacheError::SymlinkInManagedRoot(_))
        ));
        finish_fixture(fixture);
    }

    #[cfg(unix)]
    #[test]
    fn inventory_switches_mode_only_at_exact_generation_data_roots() {
        use std::os::unix::fs::symlink;
        let (repo, resolved) = resolved_fixture("inventory-generation-payloads");
        let cache = ManagedCache::initialize(resolved.clone()).unwrap();
        let plan = envelope();
        let key = CacheKey::for_plan_cache(&plan, &plan.plan.caches[0]).unwrap();
        let entry = cache.entry_path(&key);
        let staging = entry.join(".staging-1-1");
        let backup = entry.join(".backup-1-1");
        fs::create_dir_all(staging.join("data")).unwrap();
        fs::create_dir_all(backup.join("data")).unwrap();
        let outside = repo.join("generation-sentinel");
        fs::write(&outside, b"sentinel").unwrap();
        symlink(&outside, staging.join("data/external")).unwrap();
        symlink(&outside, backup.join("data/external")).unwrap();
        let accepted = cache.inventory().unwrap();
        assert_eq!(accepted.entries.len(), 1);
        assert!(accepted.entries[0].files >= 2);
        symlink(&outside, staging.join("control-link")).unwrap();
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
        assert_eq!(
            cache.promote_entry(&prepared).expect("promote"),
            CachePromotionOutcome::Promoted
        );
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

    #[cfg(unix)]
    #[test]
    fn promotion_rejects_an_unsupported_payload_object_before_journaling() {
        use std::os::unix::{fs::symlink, net::UnixListener};

        let (repo, resolved) = resolved_fixture("promotion-payload-object");
        let cache = ManagedCache::initialize(resolved.clone()).unwrap();
        let plan = envelope();
        let key = CacheKey::for_plan_cache(&plan, &plan.plan.caches[0]).unwrap();
        let prepared = cache.prepare_entry(&key, &plan.plan_digest, 1).unwrap();
        let socket_path = prepared.data_path.join("socket");
        let socket_parent = PathBuf::from("/private/tmp").join(format!(
            "ccp-socket-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        symlink(&prepared.data_path, &socket_parent).unwrap();
        let listener = UnixListener::bind(socket_parent.join("socket")).unwrap();

        assert!(matches!(
            cache.promote_entry(&prepared),
            Err(CacheError::UnexpectedEntry(_))
        ));
        assert!(!resolved.path.join(PROMOTION_JOURNAL_FILE).exists());
        assert!(!cache.entry_path(&key).join(COMPLETE_FILE).exists());

        drop(listener);
        fs::remove_file(socket_path).unwrap();
        fs::remove_file(socket_parent).unwrap();
        drop(prepared);
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
        assert_eq!(
            cache.promote_entry(&prepared).expect("promote known-good"),
            CachePromotionOutcome::Promoted
        );
        drop(prepared);

        let failed = cache
            .prepare_entry(&key, &envelope.plan_digest, 2)
            .expect("prepare failed generation");
        assert_eq!(
            fs::read(failed.data_path.join("payload.bin")).expect("copy current payload"),
            b"known-good"
        );
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

    #[cfg(unix)]
    #[test]
    fn complete_payload_symlinks_are_preserved_across_generation_reuse() {
        use std::os::unix::fs::symlink;

        let (repo, resolved) = resolved_fixture("payload-link-reuse");
        let cache = ManagedCache::initialize(resolved.clone()).expect("initialize");
        let plan = envelope();
        let key = CacheKey::for_plan_cache(&plan, &plan.plan.caches[0]).expect("cache key");
        let outside = repo.join("reuse-sentinel");
        fs::write(&outside, b"unchanged").expect("write external sentinel");

        let first = cache
            .prepare_entry(&key, &plan.plan_digest, 1)
            .expect("prepare first generation");
        fs::write(first.data_path.join("regular"), b"value").expect("write regular payload");
        symlink("regular", first.data_path.join("relative")).expect("create relative link");
        symlink("missing", first.data_path.join("broken")).expect("create broken link");
        symlink(&outside, first.data_path.join("external")).expect("create external link");
        cache
            .promote_entry(&first)
            .expect("promote first generation");
        drop(first);

        let second = cache
            .prepare_entry(&key, &plan.plan_digest, 2)
            .expect("prepare second generation");
        for name in ["relative", "broken", "external"] {
            assert_eq!(
                fs::read_link(second.data_path.join(name)).expect("read reused link"),
                fs::read_link(cache.entry_data_path(&key).join(name)).expect("read current link")
            );
        }
        assert_eq!(
            fs::read(&outside).expect("read external sentinel"),
            b"unchanged"
        );
        drop(second);
        clean(&resolved.path);
        clean(&repo);
    }

    #[cfg(unix)]
    #[test]
    fn failed_payload_preflight_removes_the_new_staging_generation() {
        use std::os::unix::{fs::symlink, net::UnixListener};

        let fixture = completed_entry_fixture("failed-payload-preflight-cleanup");
        let plan = envelope();
        let key = CacheKey::for_plan_cache(&plan, &plan.plan.caches[0]).unwrap();
        let socket_path = fixture.data_path.join("unsupported.socket");
        let socket_parent = PathBuf::from("/private/tmp").join(format!(
            "ccp-socket-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        symlink(&fixture.data_path, &socket_parent).unwrap();
        let listener = UnixListener::bind(socket_parent.join("unsupported.socket")).unwrap();

        let preparation = fixture.cache.prepare_entry(&key, &plan.plan_digest, 2);
        let staging: Vec<_> = fs::read_dir(&fixture.entry_path)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".staging-"))
            .collect();
        drop(listener);
        fs::remove_file(socket_path).unwrap();
        fs::remove_file(socket_parent).unwrap();
        finish_fixture(fixture);

        assert!(matches!(preparation, Err(CacheError::UnexpectedEntry(_))));
        assert!(staging.is_empty(), "failed preparation leaked {staging:?}");
    }

    #[cfg(unix)]
    #[test]
    fn staging_cleanup_unlinks_payload_links_without_touching_targets() {
        use std::os::unix::fs::symlink;

        let (repo, resolved) = resolved_fixture("staging-cleanup-links");
        let cache = ManagedCache::initialize(resolved.clone()).unwrap();
        let plan = envelope();
        let key = CacheKey::for_plan_cache(&plan, &plan.plan.caches[0]).unwrap();
        let outside = repo.join("cleanup-sentinel");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("first"), b"first").unwrap();
        fs::write(outside.join("second"), b"second").unwrap();

        let prepared = cache.prepare_entry(&key, &plan.plan_digest, 1).unwrap();
        fs::create_dir(prepared.data_path.join("nested")).unwrap();
        symlink(&outside, prepared.data_path.join("external")).unwrap();
        symlink(&outside, prepared.data_path.join("nested/external")).unwrap();
        let staging = prepared.staging_path.clone();
        drop(prepared);

        assert!(!staging.exists());
        assert_eq!(fs::read(outside.join("first")).unwrap(), b"first");
        assert_eq!(fs::read(outside.join("second")).unwrap(), b"second");
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

    #[test]
    fn prepared_entry_clones_share_cleanup_and_lock_until_final_drop() {
        let (repo, resolved) = resolved_fixture("entry-clone-lifetime");
        let cache = ManagedCache::initialize(resolved.clone()).expect("initialize");
        let envelope = envelope();
        let key = CacheKey::for_plan_cache(&envelope, &envelope.plan.caches[0]).expect("key");
        let first = cache
            .prepare_entry(&key, &envelope.plan_digest, 7)
            .expect("prepare");
        let staging = first.staging_path.clone();
        let clone = first.clone();

        drop(first);
        assert!(staging.is_dir(), "one clone must not remove live staging");
        assert!(matches!(
            cache.prepare_entry(&key, &envelope.plan_digest, 8),
            Err(CacheError::LockBusy(_))
        ));

        drop(clone);
        assert!(!staging.exists(), "final owner removes matching staging");
        let next = cache
            .prepare_entry(&key, &envelope.plan_digest, 8)
            .expect("lock released after final owner");
        drop(next);
        clean(&resolved.path);
        clean(&repo);
    }

    #[test]
    fn multi_entry_promotion_is_journaled_and_cleans_only_after_success() {
        let (repo, resolved) = resolved_fixture("multi-entry-promotion");
        let cache = ManagedCache::initialize(resolved.clone()).expect("initialize");
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

[[caches]]
id = "target"
mount_path = ".cache/target"

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
        .expect("plan");
        let first_key = CacheKey::for_plan_cache(&envelope, &envelope.plan.caches[0]).expect("key");
        let second_key =
            CacheKey::for_plan_cache(&envelope, &envelope.plan.caches[1]).expect("key");
        let first = cache
            .prepare_entry(&first_key, &envelope.plan_digest, 1)
            .expect("first");
        let second = cache
            .prepare_entry(&second_key, &envelope.plan_digest, 1)
            .expect("second");
        fs::write(first.data_path.join("first"), b"one").expect("first data");
        fs::write(second.data_path.join("second"), b"two").expect("second data");
        #[cfg(unix)]
        std::os::unix::fs::symlink("second", second.data_path.join("relative"))
            .expect("relative payload link");
        cache
            .promote_entries(&[first, second])
            .expect("promote both");

        assert!(!resolved.path.join(PROMOTION_JOURNAL_FILE).exists());
        assert_eq!(
            fs::read(cache.entry_data_path(&first_key).join("first")).expect("first current"),
            b"one"
        );
        assert_eq!(
            fs::read(cache.entry_data_path(&second_key).join("second")).expect("second current"),
            b"two"
        );
        #[cfg(unix)]
        {
            let relative = cache.entry_data_path(&second_key).join("relative");
            assert!(
                fs::symlink_metadata(&relative)
                    .expect("relative metadata")
                    .file_type()
                    .is_symlink(),
                "promoted payload link must remain a link"
            );
            assert_eq!(
                fs::read_link(relative).expect("read promoted relative link"),
                Path::new("second")
            );
        }
        clean(&resolved.path);
        clean(&repo);
    }

    #[test]
    fn interrupted_prepared_journal_is_recovered_without_adopting_data() {
        let (repo, resolved) = resolved_fixture("recover-prepared-journal");
        let cache = ManagedCache::initialize(resolved.clone()).expect("initialize");
        let envelope = envelope();
        let key = CacheKey::for_plan_cache(&envelope, &envelope.plan.caches[0]).expect("key");
        let prepared = cache
            .prepare_entry(&key, &envelope.plan_digest, 1)
            .expect("prepare");
        #[cfg(unix)]
        let outside = {
            let outside = repo.join("recovery-sentinel");
            fs::write(&outside, b"recovery").expect("write recovery sentinel");
            std::os::unix::fs::symlink(&outside, prepared.data_path.join("external"))
                .expect("external recovery link");
            outside
        };
        let journal = cache
            .create_promotion_journal(std::slice::from_ref(&prepared))
            .expect("journal");
        drop(prepared);
        let lock = acquire_promotion_lock(&resolved.path).expect("promotion lock");
        cache.recover_promotion_locked().expect("recover");
        drop(lock);

        assert!(!resolved.path.join(PROMOTION_JOURNAL_FILE).exists());
        assert!(!cache.entry_path(&key).join("data").exists());
        #[cfg(unix)]
        assert_eq!(
            fs::read(outside).expect("read recovery sentinel"),
            b"recovery"
        );
        assert_eq!(journal.entries.len(), 1);
        clean(&resolved.path);
        clean(&repo);
    }

    struct CompletedSourceFixture {
        repo: PathBuf,
        resolved: ResolvedCacheRoot,
        cache: ManagedCache,
        data_path: PathBuf,
        entry_path: PathBuf,
    }

    fn completed_entry_fixture(name: &str) -> CompletedSourceFixture {
        let (repo, resolved) = resolved_fixture(name);
        let cache = ManagedCache::initialize(resolved.clone()).expect("initialize");
        let plan = envelope();
        let key = CacheKey::for_plan_cache(&plan, &plan.plan.caches[0]).expect("key");
        let prepared = cache
            .prepare_entry(&key, &plan.plan_digest, 1)
            .expect("prepare");
        fs::write(prepared.data_path.join("payload"), b"owned fixture").expect("payload");
        cache.promote_entry(&prepared).expect("promote");
        let entry_path = cache.entry_path(&key);
        CompletedSourceFixture {
            repo,
            resolved,
            cache,
            data_path: entry_path.join("data"),
            entry_path,
        }
    }

    fn finish_fixture(fixture: CompletedSourceFixture) {
        clean(&fixture.resolved.path);
        clean(&fixture.repo);
    }

    #[test]
    fn completed_source_pin_holds_entry_lock_until_drop() {
        let fixture = completed_entry_fixture("completed-source-pin");
        let pins = fixture
            .cache
            .pin_completed_sources(std::slice::from_ref(&fixture.data_path))
            .expect("pin completed source");
        assert_eq!(pins.len(), 1);
        assert!(matches!(
            fixture
                .cache
                .pin_completed_sources(std::slice::from_ref(&fixture.data_path)),
            Err(CacheError::LockBusy(_))
        ));
        drop(pins);
        assert_eq!(
            fixture
                .cache
                .pin_completed_sources(std::slice::from_ref(&fixture.data_path))
                .expect("re-pin")
                .len(),
            1
        );
        finish_fixture(fixture);
    }

    #[test]
    fn completed_source_pin_rejects_missing_entry_lock_without_recreating_it() {
        let fixture = completed_entry_fixture("completed-source-pin-missing-lock");
        let lock = fixture.entry_path.join(ENTRY_LOCK_FILE);
        fs::remove_file(&lock).expect("remove entry lock");
        assert!(
            fixture
                .cache
                .pin_completed_sources(std::slice::from_ref(&fixture.data_path))
                .is_err()
        );
        assert!(!lock.exists(), "pin must not recreate missing lock");
        finish_fixture(fixture);
    }

    #[test]
    fn completed_source_pin_rejects_missing_workspaces_directory() {
        let fixture = completed_entry_fixture("completed-source-pin-missing-workspaces");
        let workspaces = fixture.resolved.path.join(WORKSPACES_DIR);
        fs::remove_dir(&workspaces).expect("remove empty workspaces directory");
        assert!(
            fixture
                .cache
                .pin_completed_sources(std::slice::from_ref(&fixture.data_path))
                .is_err()
        );
        finish_fixture(fixture);
    }

    #[test]
    fn completed_source_pin_deduplicates_and_orders_sources() {
        let first = completed_entry_fixture("completed-source-pin-order");
        let second_key = CacheKey {
            digest: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .to_owned(),
        };
        let plan = envelope();
        let prepared = first
            .cache
            .prepare_entry(&second_key, &plan.plan_digest, 1)
            .expect("second prepare");
        fs::write(prepared.data_path.join("payload"), b"second fixture").expect("payload");
        first
            .cache
            .promote_entry(&prepared)
            .expect("second promote");
        drop(prepared);
        let second_data_path = first.cache.entry_data_path(&second_key);
        let sources = [
            second_data_path.clone(),
            first.data_path.clone(),
            second_data_path,
        ];
        let pins = first.cache.pin_completed_sources(&sources).expect("pins");
        assert_eq!(pins.len(), 2);
        assert!(pins[0].entry_path < pins[1].entry_path);
        drop(pins);
        finish_fixture(first);
    }

    #[test]
    fn completed_source_pin_rejects_invalid_source_shapes() {
        let fixture = completed_entry_fixture("completed-source-pin-invalid");
        let root = fixture.resolved.path.clone();
        let cases = [
            root.join("other/entries/sha256-").join("data"),
            fixture.entry_path.join("data/extra"),
            fixture.entry_path.join("not-data"),
            root.join("entries/invalid-key/data"),
            root.join("entries/sha256-").join("data"),
            root.join("entries/sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/staging/data"),
        ];
        for source in cases {
            assert!(
                fixture
                    .cache
                    .pin_completed_sources(std::slice::from_ref(&source))
                    .is_err(),
                "accepted {source:?}"
            );
        }
        finish_fixture(fixture);
    }

    #[test]
    fn completed_source_pin_rejects_non_absolute_and_other_root_sources() {
        let fixture = completed_entry_fixture("completed-source-pin-roots");
        let relative = PathBuf::from("relative/data");
        assert!(
            fixture
                .cache
                .pin_completed_sources(std::slice::from_ref(&relative))
                .is_err()
        );
        let other = completed_entry_fixture("completed-source-pin-other-root");
        assert!(
            fixture
                .cache
                .pin_completed_sources(std::slice::from_ref(&other.data_path))
                .is_err()
        );
        finish_fixture(other);
        finish_fixture(fixture);
    }

    #[test]
    fn completed_source_pin_rejects_valid_missing_key_source() {
        let fixture = completed_entry_fixture("completed-source-pin-missing-key");
        let missing = fixture.resolved.path.join(
            "entries/sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/data",
        );
        assert!(
            fixture
                .cache
                .pin_completed_sources(std::slice::from_ref(&missing))
                .is_err()
        );
        finish_fixture(fixture);
    }

    #[test]
    fn completed_source_pin_rejects_missing_or_mismatched_generation_manifest() {
        let fixture = completed_entry_fixture("completed-source-pin-manifest-missing");
        fs::remove_file(fixture.entry_path.join(GENERATION_MANIFEST_FILE)).expect("manifest");
        assert!(
            fixture
                .cache
                .pin_completed_sources(std::slice::from_ref(&fixture.data_path))
                .is_err()
        );
        finish_fixture(fixture);

        let fixture = completed_entry_fixture("completed-source-pin-manifest-mismatch");
        let manifest = fixture.entry_path.join(GENERATION_MANIFEST_FILE);
        let mut parsed = read_generation_manifest(&manifest).expect("manifest");
        parsed.key_digest =
            "sha256-cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_owned();
        fs::write(&manifest, serde_json::to_vec(&parsed).expect("encode")).expect("write");
        assert!(
            fixture
                .cache
                .pin_completed_sources(std::slice::from_ref(&fixture.data_path))
                .is_err()
        );
        finish_fixture(fixture);
    }

    #[test]
    fn completed_source_pin_rejects_invalid_plan_digest_and_non_complete_state() {
        let fixture = completed_entry_fixture("completed-source-pin-plan-invalid");
        let manifest = fixture.entry_path.join(GENERATION_MANIFEST_FILE);
        let mut parsed = read_generation_manifest(&manifest).expect("manifest");
        parsed.plan_digest = "not-a-digest".to_owned();
        fs::write(&manifest, serde_json::to_vec(&parsed).expect("encode")).expect("write");
        assert!(
            fixture
                .cache
                .pin_completed_sources(std::slice::from_ref(&fixture.data_path))
                .is_err()
        );
        finish_fixture(fixture);

        let fixture = completed_entry_fixture("completed-source-pin-state");
        let manifest = fixture.entry_path.join(GENERATION_MANIFEST_FILE);
        let mut parsed = read_generation_manifest(&manifest).expect("manifest");
        parsed.state = "staging".to_owned();
        fs::write(&manifest, serde_json::to_vec(&parsed).expect("encode")).expect("write");
        assert!(
            fixture
                .cache
                .pin_completed_sources(std::slice::from_ref(&fixture.data_path))
                .is_err()
        );
        finish_fixture(fixture);
    }

    #[test]
    fn completed_source_pin_rejects_missing_source_and_incomplete_entry() {
        let fixture = completed_entry_fixture("completed-source-pin-incomplete");
        let missing = fixture.resolved.path.join("entries/sha256-").join("data");
        assert!(
            fixture
                .cache
                .pin_completed_sources(std::slice::from_ref(&missing))
                .is_err()
        );
        fs::remove_file(fixture.entry_path.join(COMPLETE_FILE)).expect("marker");
        assert!(
            fixture
                .cache
                .pin_completed_sources(std::slice::from_ref(&fixture.data_path))
                .is_err()
        );
        finish_fixture(fixture);
    }

    #[cfg(unix)]
    #[test]
    fn completed_source_pin_rejects_symlink_component_and_wrong_type() {
        use std::os::unix::fs::symlink;
        let fixture = completed_entry_fixture("completed-source-pin-symlink");
        let link = fixture.repo.join("link");
        symlink(&fixture.resolved.path, &link).expect("symlink");
        let escaped = link
            .join("entries")
            .join(fixture.entry_path.file_name().unwrap())
            .join("data");
        assert!(
            fixture
                .cache
                .pin_completed_sources(std::slice::from_ref(&escaped))
                .is_err()
        );
        fs::remove_dir_all(&fixture.data_path).expect("data dir");
        fs::write(&fixture.data_path, b"wrong type").expect("wrong type");
        assert!(
            fixture
                .cache
                .pin_completed_sources(std::slice::from_ref(&fixture.data_path))
                .is_err()
        );
        finish_fixture(fixture);
    }

    #[test]
    fn active_prepared_generation_makes_completed_source_pin_busy_and_failure_releases_prior_pins()
    {
        let fixture = completed_entry_fixture("completed-source-pin-busy");
        let plan = envelope();
        let key = CacheKey::for_plan_cache(&plan, &plan.plan.caches[0]).expect("key");
        let prepared = fixture
            .cache
            .prepare_entry(&key, &plan.plan_digest, 2)
            .expect("prepare");
        assert!(matches!(
            fixture
                .cache
                .pin_completed_sources(std::slice::from_ref(&fixture.data_path)),
            Err(CacheError::LockBusy(_))
        ));
        drop(prepared);
        let bad = fixture.resolved.path.join("missing");
        assert!(
            fixture
                .cache
                .pin_completed_sources(&[fixture.data_path.clone(), bad])
                .is_err()
        );
        assert!(
            fixture
                .cache
                .pin_completed_sources(std::slice::from_ref(&fixture.data_path))
                .is_ok()
        );
        finish_fixture(fixture);
    }
}
