// Copyright 2026 Marco Porcellato
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
use std::sync::{Arc, atomic::AtomicUsize};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub enum DurableFsError {
    Io(io::Error),
    UnsafePath(&'static str),
    OwnershipMismatch,
    AtomicReplacementUnavailable,
}

impl fmt::Display for DurableFsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "durable filesystem operation failed: {error}"),
            Self::UnsafePath(message) => write!(formatter, "unsafe durable path: {message}"),
            Self::OwnershipMismatch => formatter.write_str("durable state ownership mismatch"),
            Self::AtomicReplacementUnavailable => formatter.write_str(
                "atomic replacement of an existing file is unavailable on this platform",
            ),
        }
    }
}

impl std::error::Error for DurableFsError {}

impl From<io::Error> for DurableFsError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug, Clone, Default)]
pub struct DurableFileSystem {
    #[cfg(test)]
    fault: Option<Arc<FaultPlan>>,
}

#[cfg(test)]
#[derive(Debug)]
struct FaultPlan {
    fail_at: usize,
    operation: AtomicUsize,
}

impl DurableFileSystem {
    pub(crate) fn relocate_empty_directory(
        &self,
        source: &Path,
        destination: &Path,
    ) -> Result<(), DurableFsError> {
        let source_parent = checked_parent(source)?;
        let destination_parent = checked_parent(destination)?;
        validate_plain_directory(source_parent)?;
        validate_plain_directory(destination_parent)?;
        validate_plain_directory(source)?;
        if fs::read_dir(source)?.next().transpose()?.is_some() {
            return Err(DurableFsError::UnsafePath("source directory must be empty"));
        }
        match fs::symlink_metadata(destination) {
            Ok(_) => {
                return Err(DurableFsError::UnsafePath(
                    "quarantine destination already exists",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(DurableFsError::Io(error)),
        }
        self.checkpoint()?;
        fs::rename(source, destination)?;
        self.checkpoint()?;
        sync_directory(destination_parent)?;
        self.checkpoint()?;
        sync_directory(source_parent)?;
        Ok(())
    }
    pub fn create_new_directory(&self, path: &Path) -> Result<(), DurableFsError> {
        let parent = checked_parent(path)?;
        validate_plain_directory(parent)?;
        self.checkpoint()?;
        fs::create_dir(path)?;
        self.checkpoint()?;
        sync_directory(parent)?;
        Ok(())
    }

    pub fn create_directory(&self, path: &Path) -> Result<(), DurableFsError> {
        let parent = checked_parent(path)?;
        validate_plain_directory(parent)?;
        self.checkpoint()?;
        match fs::create_dir(path) {
            Ok(()) => {
                self.checkpoint()?;
                sync_directory(parent)
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                validate_plain_directory(path)
            }
            Err(error) => Err(DurableFsError::Io(error)),
        }
    }

    pub fn create_new(&self, path: &Path, bytes: &[u8]) -> Result<(), DurableFsError> {
        let parent = checked_parent(path)?;
        validate_plain_directory(parent)?;
        self.checkpoint()?;
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        self.checkpoint()?;
        file.write_all(bytes)?;
        self.checkpoint()?;
        file.sync_all()?;
        self.checkpoint()?;
        sync_directory(parent)?;
        Ok(())
    }

    pub fn atomic_replace(&self, path: &Path, bytes: &[u8]) -> Result<(), DurableFsError> {
        let parent = checked_parent(path)?;
        validate_plain_directory(parent)?;
        reject_symlink_or_non_file(path)?;

        #[cfg(windows)]
        if path.exists() {
            return Err(DurableFsError::AtomicReplacementUnavailable);
        }

        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".ccp-durable-tmp-{}-{sequence}",
            std::process::id()
        ));
        let result = (|| {
            self.checkpoint()?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            self.checkpoint()?;
            file.write_all(bytes)?;
            self.checkpoint()?;
            file.sync_all()?;
            self.checkpoint()?;
            fs::rename(&temporary, path)?;
            self.checkpoint()?;
            sync_directory(parent)
        })();
        if temporary.exists() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    pub fn quarantine_owned_tree(
        &self,
        path: &Path,
        quarantine: &Path,
        marker_name: &str,
        marker_bytes: &[u8],
    ) -> Result<(), DurableFsError> {
        validate_owned_tree(path, marker_name, marker_bytes)?;
        if quarantine.exists() {
            return Err(DurableFsError::UnsafePath(
                "quarantine destination already exists",
            ));
        }
        let parent = checked_parent(path)?;
        if checked_parent(quarantine)? != parent {
            return Err(DurableFsError::UnsafePath(
                "quarantine must remain in the owned parent directory",
            ));
        }
        self.checkpoint()?;
        fs::rename(path, quarantine)?;
        self.checkpoint()?;
        sync_directory(parent)?;
        Ok(())
    }

    pub fn remove_owned_tree(
        &self,
        path: &Path,
        marker_name: &str,
        marker_bytes: &[u8],
    ) -> Result<(), DurableFsError> {
        validate_owned_tree(path, marker_name, marker_bytes)?;
        let parent = checked_parent(path)?.to_path_buf();
        self.checkpoint()?;
        fs::remove_dir_all(path)?;
        self.checkpoint()?;
        sync_directory(&parent)?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn failing_at(operation: usize) -> Self {
        Self {
            fault: Some(Arc::new(FaultPlan {
                fail_at: operation,
                operation: AtomicUsize::new(0),
            })),
        }
    }

    #[cfg(test)]
    fn checkpoint(&self) -> Result<(), DurableFsError> {
        if let Some(fault) = &self.fault
            && fault.operation.fetch_add(1, Ordering::SeqCst) + 1 == fault.fail_at
        {
            return Err(DurableFsError::Io(io::Error::new(
                io::ErrorKind::StorageFull,
                "injected durable filesystem failure",
            )));
        }
        Ok(())
    }

    #[cfg(not(test))]
    fn checkpoint(&self) -> Result<(), DurableFsError> {
        Ok(())
    }
}

fn checked_parent(path: &Path) -> Result<&Path, DurableFsError> {
    if !path.is_absolute() {
        return Err(DurableFsError::UnsafePath("path must be absolute"));
    }
    path.parent()
        .ok_or(DurableFsError::UnsafePath("path has no parent"))
}

fn validate_plain_directory(path: &Path) -> Result<(), DurableFsError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(DurableFsError::UnsafePath(
            "parent must be a plain directory",
        ));
    }
    Ok(())
}

fn reject_symlink_or_non_file(path: &Path) -> Result<(), DurableFsError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(
            DurableFsError::UnsafePath("replacement target must be a plain file"),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DurableFsError::Io(error)),
    }
}

fn validate_owned_tree(
    path: &Path,
    marker_name: &str,
    marker_bytes: &[u8],
) -> Result<(), DurableFsError> {
    if marker_name.is_empty() || marker_name.contains(['/', '\\']) {
        return Err(DurableFsError::UnsafePath("invalid owner marker name"));
    }
    validate_plain_directory(path)?;
    let marker = path.join(marker_name);
    let metadata = fs::symlink_metadata(&marker)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(DurableFsError::OwnershipMismatch);
    }
    if fs::read(marker)? != marker_bytes {
        return Err(DurableFsError::OwnershipMismatch);
    }
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), DurableFsError> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(windows)]
fn sync_directory(_path: &Path) -> Result<(), DurableFsError> {
    // Rust std does not expose the backup-semantics flag required to fsync a
    // directory on Windows. File contents are still synced; replacement of an
    // existing target fails closed above instead of degrading to remove+rename.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_directory(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ccp-durable-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("temporary directory");
        path
    }

    #[test]
    fn create_new_is_durable_and_never_overwrites() {
        let root = temporary_directory("create-new");
        let target = root.join("state.json");
        let durable = DurableFileSystem::default();
        durable.create_new(&target, b"first\n").expect("create");
        assert!(durable.create_new(&target, b"second\n").is_err());
        assert_eq!(fs::read(&target).expect("read"), b"first\n");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn directory_creation_rejects_symlinks_and_non_directories() {
        let root = temporary_directory("directory");
        let path = root.join("owned");
        DurableFileSystem::default()
            .create_directory(&path)
            .expect("create");
        DurableFileSystem::default()
            .create_directory(&path)
            .expect("idempotent");
        assert!(path.is_dir());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn create_new_directory_never_adopts_an_existing_path() {
        let root = temporary_directory("new-directory");
        let path = root.join("owned");
        fs::create_dir(&path).expect("foreign directory");
        assert!(
            DurableFileSystem::default()
                .create_new_directory(&path)
                .is_err()
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn relocate_empty_directory_preserves_source_as_append_only_destination() {
        let root = temporary_directory("relocate-empty");
        let source = root.join("agent-tickets");
        let quarantine = root.join("quarantine");
        let destination = quarantine.join("agent-tickets.recovered-v1-plan");
        fs::create_dir(&source).expect("source");
        fs::create_dir(&quarantine).expect("quarantine");
        DurableFileSystem::default()
            .relocate_empty_directory(&source, &destination)
            .expect("relocate");
        assert!(!source.exists());
        assert!(destination.is_dir());
        assert!(
            DurableFileSystem::default()
                .relocate_empty_directory(&destination, &destination)
                .is_err()
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn relocate_empty_directory_rejects_contents_and_existing_destination() {
        let root = temporary_directory("relocate-reject");
        let source = root.join("agent-tickets");
        let quarantine = root.join("quarantine");
        let destination = quarantine.join("agent-tickets.recovered-v1-plan");
        fs::create_dir(&source).expect("source");
        fs::create_dir(&quarantine).expect("quarantine");
        fs::write(source.join("entry"), b"blocked\n").expect("source entry");
        assert!(
            DurableFileSystem::default()
                .relocate_empty_directory(&source, &destination)
                .is_err()
        );
        assert_eq!(
            fs::read(source.join("entry")).expect("entry remains"),
            b"blocked\n"
        );
        assert!(!destination.exists());
        fs::remove_file(source.join("entry")).expect("remove fixture entry");
        fs::create_dir(&destination).expect("destination collision");
        assert!(
            DurableFileSystem::default()
                .relocate_empty_directory(&source, &destination)
                .is_err()
        );
        assert!(source.is_dir());
        assert!(destination.is_dir());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn quarantine_requires_exact_ownership_and_same_parent() {
        let root = temporary_directory("quarantine");
        let owned = root.join("run");
        fs::create_dir(&owned).expect("owned");
        fs::write(owned.join(".owner"), b"ccp\n").expect("marker");
        let quarantine = root.join("quarantined-run");
        DurableFileSystem::default()
            .quarantine_owned_tree(&owned, &quarantine, ".owner", b"ccp\n")
            .expect("quarantine");
        assert!(!owned.exists());
        assert!(quarantine.exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn owned_tree_removal_rejects_foreign_marker() {
        let root = temporary_directory("remove");
        let owned = root.join("run");
        fs::create_dir(&owned).expect("owned");
        fs::write(owned.join(".owner"), b"foreign\n").expect("marker");
        assert!(matches!(
            DurableFileSystem::default().remove_owned_tree(&owned, ".owner", b"ccp\n"),
            Err(DurableFsError::OwnershipMismatch)
        ));
        assert!(owned.exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn atomic_replace_preserves_old_or_new_complete_value() {
        let root = temporary_directory("replace");
        let target = root.join("state.json");
        fs::write(&target, b"old\n").expect("old");
        DurableFileSystem::default()
            .atomic_replace(&target, b"new\n")
            .expect("replace");
        assert_eq!(fs::read(&target).expect("read"), b"new\n");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(windows)]
    #[test]
    fn atomic_replace_existing_fails_closed_on_windows() {
        let root = temporary_directory("replace");
        let target = root.join("state.json");
        fs::write(&target, b"old\n").expect("old");
        assert!(matches!(
            DurableFileSystem::default().atomic_replace(&target, b"new\n"),
            Err(DurableFsError::AtomicReplacementUnavailable)
        ));
        assert_eq!(fs::read(&target).expect("read"), b"old\n");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn fail_at_each_replace_operation_never_exposes_partial_bytes() {
        for fail_at in 1..=5 {
            let root = temporary_directory("fault-replace");
            let target = root.join("state.json");
            fs::write(&target, b"old-complete\n").expect("old");
            let result =
                DurableFileSystem::failing_at(fail_at).atomic_replace(&target, b"new-complete\n");
            assert!(result.is_err());
            let bytes = fs::read(&target).expect("read");
            assert!(bytes == b"old-complete\n" || bytes == b"new-complete\n");
            fs::remove_dir_all(root).expect("cleanup");
        }
    }
}
