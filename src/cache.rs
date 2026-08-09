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
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::config::{ExecutionPlanEnvelopeV1, NormalizedCache};
use crate::receipt::{ReceiptError, canonical_json};

pub const DEFAULT_DISK_BUDGET_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const OWNER_FILE: &str = ".ccp-cache-root-v1.json";
const OWNER_BYTES: &[u8] =
    b"{\"owner\":\"commit-ci-preflight\",\"purpose\":\"managed-cache-root\",\"schema_version\":\"1.0\"}\n";
const ENTRIES_DIR: &str = "entries";
const WORKSPACES_DIR: &str = "workspaces";
const COMPLETE_FILE: &str = ".complete-v1";
const COMPLETE_BYTES: &[u8] = b"complete-v1\n";
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
    let suffix = Path::new("commit-ci-preflight");
    match options.platform {
        PlatformFamily::MacOs => options
            .home
            .as_ref()
            .map(|home| home.join("Library").join("Caches").join(suffix)),
        PlatformFamily::Windows => options
            .local_app_data
            .as_ref()
            .map(|base| base.join(suffix).join("cache")),
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
            validate_owner_marker(&marker)?;
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
        std::env::current_dir()
            .expect("current directory")
            .parent()
            .expect("repository parent")
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
}
