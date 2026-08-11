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

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::process::CancellationToken;

pub const ADMISSION_SCHEMA_VERSION: &str = "1.0";
pub const DEFAULT_QUEUE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
pub const MAX_QUEUE_TICKETS: usize = 1024;

const OWNER_FILE: &str = ".ccp-admission-root-v1.json";
const PLATFORM_DIRECTORY: &str = "commit-ci-preflight-admission";
const OWNER_BYTES: &[u8] =
    b"{\"owner\":\"commit-ci-preflight\",\"purpose\":\"host-admission-coordinator\",\"schema_version\":\"1.0\"}\n";
const QUEUE_LOCK: &str = "queue.lock";
const SLOT_LOCK: &str = "slot.lock";
const NEXT_TICKET: &str = "next-ticket-v1";
const TICKETS_DIR: &str = "tickets";
const TICKET_PREFIX: &str = "ticket-";
const TICKET_SUFFIX: &str = ".json";
const WAIT_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdmissionStatusV1 {
    pub schema_version: String,
    pub active: bool,
    pub queue_count: usize,
    pub ticket_ids: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TicketMarker {
    owner: String,
    purpose: String,
    schema_version: String,
    ticket_id: String,
}

#[derive(Debug)]
struct TicketInfo {
    id: String,
    path: PathBuf,
}

#[derive(Debug)]
struct StaleTicket {
    ticket: TicketInfo,
    file: File,
}

#[derive(Debug, Clone)]
pub struct AdmissionCoordinator {
    root: PathBuf,
}

impl AdmissionCoordinator {
    #[cfg(test)]
    fn test_at(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn platform() -> Result<Self, AdmissionError> {
        let root = platform_root()?;
        Self::at(root)
    }

    pub fn platform_for(repository: &Path) -> Result<Self, AdmissionError> {
        let coordinator = Self::platform()?;
        let repository = canonicalize_existing_prefix(repository)?;
        if coordinator.root == repository || coordinator.root.starts_with(&repository) {
            return Err(AdmissionError::UnsafePath(
                "coordinator root cannot be the repository or one of its descendants",
            ));
        }
        Ok(coordinator)
    }

    pub fn at(root: PathBuf) -> Result<Self, AdmissionError> {
        let root = validate_root_candidate(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn acquire(
        &self,
        timeout: Duration,
        cancellation: &CancellationToken,
    ) -> Result<AdmissionGuard, AdmissionError> {
        if timeout.is_zero() {
            return Err(AdmissionError::InvalidTimeout);
        }
        if cancellation.is_cancelled() {
            return Err(AdmissionError::Cancelled);
        }
        self.initialize()?;
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(AdmissionError::InvalidTimeout)?;
        let mut queue = self.lock_queue_until(deadline, cancellation)?;
        let (live, stale) = self.scan_tickets(None, true)?;
        self.remove_stale(stale)?;
        if live.len() >= MAX_QUEUE_TICKETS {
            unlock(&mut queue)?;
            return Err(AdmissionError::QueueFull);
        }
        let mut reservation = self.create_ticket()?;
        unlock(&mut queue)?;

        loop {
            if cancellation.is_cancelled() {
                reservation.cleanup(self)?;
                return Err(AdmissionError::Cancelled);
            }
            if Instant::now() >= deadline {
                reservation.cleanup(self)?;
                return Err(AdmissionError::Timeout);
            }

            let mut queue = self.lock_queue_until(deadline, cancellation)?;
            let (live, stale) = self.scan_tickets(
                Some(
                    reservation
                        .id
                        .as_deref()
                        .expect("reservation id is present"),
                ),
                true,
            )?;
            self.remove_stale(stale)?;
            let first_is_ours = live
                .first()
                .is_some_and(|ticket| Some(ticket.id.as_str()) == reservation.id.as_deref());
            if first_is_ours && let Some(slot) = self.try_lock_slot()? {
                unlock(&mut queue)?;
                return Ok(AdmissionGuard {
                    coordinator: self.clone(),
                    ticket_path: reservation
                        .path
                        .take()
                        .expect("reservation path is present"),
                    ticket: Some(
                        reservation
                            .file
                            .take()
                            .expect("reservation file is present"),
                    ),
                    slot: Some(slot),
                    ticket_id: reservation.id.take().expect("reservation id is present"),
                });
            }
            unlock(&mut queue)?;
            sleep_until(deadline, cancellation);
        }
    }

    pub fn status(&self) -> Result<AdmissionStatusV1, AdmissionError> {
        if !self.root_exists()? {
            return Ok(empty_status());
        }
        self.validate_layout(true)?;
        let mut queue = self.open_queue(false)?;
        queue
            .lock_exclusive()
            .map_err(|source| AdmissionError::Lock {
                path: self.root.join(QUEUE_LOCK),
                source,
            })?;
        let (live, _stale) = self.scan_tickets(None, false)?;
        let active = self.slot_is_busy()?;
        unlock(&mut queue)?;
        let first = usize::from(active && !live.is_empty());
        let ticket_ids: Vec<String> = live
            .into_iter()
            .skip(first)
            .map(|ticket| ticket.id)
            .collect();
        Ok(AdmissionStatusV1 {
            schema_version: ADMISSION_SCHEMA_VERSION.to_owned(),
            active,
            queue_count: ticket_ids.len(),
            ticket_ids,
        })
    }

    fn initialize(&self) -> Result<(), AdmissionError> {
        fs::create_dir_all(&self.root).map_err(|source| AdmissionError::Io {
            path: self.root.clone(),
            source,
        })?;
        self.validate_layout(false)?;
        let mut queue = self.open_queue(true)?;
        queue
            .lock_exclusive()
            .map_err(|source| AdmissionError::Lock {
                path: self.root.join(QUEUE_LOCK),
                source,
            })?;
        self.ensure_owner_marker()?;
        let tickets = self.root.join(TICKETS_DIR);
        if let Ok(metadata) = fs::symlink_metadata(&tickets) {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(AdmissionError::UnsafeLayout(tickets));
            }
        } else {
            fs::create_dir(&tickets).map_err(|source| AdmissionError::Io {
                path: tickets.clone(),
                source,
            })?;
        }
        self.validate_layout(true)?;
        unlock(&mut queue)
    }

    fn ensure_owner_marker(&self) -> Result<(), AdmissionError> {
        let path = self.root.join(OWNER_FILE);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(AdmissionError::UnsafeLayout(path));
                }
                let actual = fs::read(&path).map_err(|source| AdmissionError::Io {
                    path: path.clone(),
                    source,
                })?;
                if actual != OWNER_BYTES {
                    return Err(AdmissionError::ForeignOwner(path));
                }
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(|source| AdmissionError::Io {
                        path: path.clone(),
                        source,
                    })?;
                file.write_all(OWNER_BYTES)
                    .map_err(|source| AdmissionError::Io {
                        path: path.clone(),
                        source,
                    })?;
                file.sync_all()
                    .map_err(|source| AdmissionError::Io { path, source })
            }
            Err(source) => Err(AdmissionError::Io { path, source }),
        }
    }

    fn create_ticket(&self) -> Result<TicketReservation, AdmissionError> {
        let id = self.next_ticket_id()?;
        let path = self
            .root
            .join(TICKETS_DIR)
            .join(format!("{TICKET_PREFIX}{id}{TICKET_SUFFIX}"));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|source| AdmissionError::Io {
                path: path.clone(),
                source,
            })?;
        file.lock_exclusive()
            .map_err(|source| AdmissionError::Lock {
                path: path.clone(),
                source,
            })?;
        let marker = TicketMarker {
            owner: "commit-ci-preflight".to_owned(),
            purpose: "host-admission-ticket".to_owned(),
            schema_version: ADMISSION_SCHEMA_VERSION.to_owned(),
            ticket_id: id.clone(),
        };
        let bytes = serde_json::to_vec(&marker).map_err(AdmissionError::Json)?;
        file.write_all(&bytes)
            .map_err(|source| AdmissionError::Io {
                path: path.clone(),
                source,
            })?;
        file.write_all(b"\n").map_err(|source| AdmissionError::Io {
            path: path.clone(),
            source,
        })?;
        file.sync_all().map_err(|source| AdmissionError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(TicketReservation {
            id: Some(id),
            path: Some(path),
            file: Some(file),
        })
    }

    fn next_ticket_id(&self) -> Result<String, AdmissionError> {
        let path = self.root.join(NEXT_TICKET);
        let next = match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(AdmissionError::UnsafeLayout(path));
                }
                let text = fs::read_to_string(&path).map_err(|source| AdmissionError::Io {
                    path: path.clone(),
                    source,
                })?;
                text.trim()
                    .parse::<u64>()
                    .ok()
                    .filter(|value| *value > 0)
                    .ok_or_else(|| AdmissionError::MalformedCounter(path.clone()))?
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => 1,
            Err(source) => return Err(AdmissionError::Io { path, source }),
        };
        let next_after = next
            .checked_add(1)
            .ok_or(AdmissionError::TicketCounterExhausted)?;
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .map_err(|source| AdmissionError::Io {
                path: path.clone(),
                source,
            })?;
        writeln!(file, "{next_after}").map_err(|source| AdmissionError::Io {
            path: path.clone(),
            source,
        })?;
        file.sync_all()
            .map_err(|source| AdmissionError::Io { path, source })?;
        Ok(format!("{next:020}"))
    }

    fn lock_queue_until(
        &self,
        deadline: Instant,
        cancellation: &CancellationToken,
    ) -> Result<File, AdmissionError> {
        let path = self.root.join(QUEUE_LOCK);
        let file = self.open_queue(false)?;
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => return Ok(file),
                Err(source) if source.kind() == io::ErrorKind::WouldBlock => {
                    if cancellation.is_cancelled() {
                        return Err(AdmissionError::Cancelled);
                    }
                    if Instant::now() >= deadline {
                        return Err(AdmissionError::Timeout);
                    }
                    thread::sleep(WAIT_INTERVAL);
                }
                Err(source) => return Err(AdmissionError::Lock { path, source }),
            }
        }
    }

    fn scan_tickets(
        &self,
        own_id: Option<&str>,
        reclaim_stale: bool,
    ) -> Result<(Vec<TicketInfo>, Vec<StaleTicket>), AdmissionError> {
        let directory =
            fs::read_dir(self.root.join(TICKETS_DIR)).map_err(|source| AdmissionError::Io {
                path: self.root.join(TICKETS_DIR),
                source,
            })?;
        let mut live = Vec::new();
        let mut stale = Vec::new();
        let mut count = 0;
        for entry in directory {
            let entry = entry.map_err(AdmissionError::ReadDir)?;
            count += 1;
            if count > MAX_QUEUE_TICKETS {
                return Err(AdmissionError::QueueFull);
            }
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|source| AdmissionError::Io {
                path: path.clone(),
                source,
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(AdmissionError::MalformedTicket(path));
            }
            let id = parse_ticket_name(&path)?;
            let marker = read_ticket(&path)?;
            if marker.owner != "commit-ci-preflight"
                || marker.purpose != "host-admission-ticket"
                || marker.schema_version != ADMISSION_SCHEMA_VERSION
                || marker.ticket_id != id
            {
                return Err(AdmissionError::ForeignTicket(path));
            }
            let ticket = TicketInfo { id, path };
            if own_id == Some(ticket.id.as_str()) {
                live.push(ticket);
                continue;
            }
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&ticket.path)
                .map_err(|source| AdmissionError::Io {
                    path: ticket.path.clone(),
                    source,
                })?;
            match file.try_lock_exclusive() {
                Ok(()) => {
                    if reclaim_stale {
                        stale.push(StaleTicket { ticket, file });
                    } else {
                        FileExt::unlock(&file).map_err(|source| AdmissionError::Lock {
                            path: ticket.path.clone(),
                            source,
                        })?;
                    }
                }
                Err(source) if source.kind() == io::ErrorKind::WouldBlock => live.push(ticket),
                Err(source) => {
                    return Err(AdmissionError::Lock {
                        path: ticket.path,
                        source,
                    });
                }
            }
        }
        live.sort_by(|left, right| left.id.cmp(&right.id));
        stale.sort_by(|left, right| left.ticket.id.cmp(&right.ticket.id));
        Ok((live, stale))
    }

    fn remove_stale(&self, stale: Vec<StaleTicket>) -> Result<(), AdmissionError> {
        for stale in stale {
            fs::remove_file(&stale.ticket.path).map_err(|source| AdmissionError::Io {
                path: stale.ticket.path.clone(),
                source,
            })?;
            FileExt::unlock(&stale.file).map_err(|source| AdmissionError::Lock {
                path: stale.ticket.path,
                source,
            })?;
        }
        Ok(())
    }

    fn try_lock_slot(&self) -> Result<Option<File>, AdmissionError> {
        let path = self.root.join(SLOT_LOCK);
        let file = open_lock_file(&path, true)?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(file)),
            Err(source) if source.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(source) => Err(AdmissionError::Lock { path, source }),
        }
    }

    fn slot_is_busy(&self) -> Result<bool, AdmissionError> {
        let path = self.root.join(SLOT_LOCK);
        let Some(file) = open_existing_lock_file(&path)? else {
            return Ok(false);
        };
        match file.try_lock_exclusive() {
            Ok(()) => {
                FileExt::unlock(&file).map_err(|source| AdmissionError::Lock { path, source })?;
                Ok(false)
            }
            Err(source) if source.kind() == io::ErrorKind::WouldBlock => Ok(true),
            Err(source) => Err(AdmissionError::Lock { path, source }),
        }
    }

    fn open_queue(&self, create: bool) -> Result<File, AdmissionError> {
        let path = self.root.join(QUEUE_LOCK);
        if create {
            open_lock_file(&path, true)
        } else {
            open_existing_lock_file(&path)?.ok_or(AdmissionError::UnsafeLayout(path))
        }
    }

    fn root_exists(&self) -> Result<bool, AdmissionError> {
        match fs::symlink_metadata(&self.root) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(AdmissionError::UnsafeLayout(self.root.clone()));
                }
                Ok(true)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(AdmissionError::Io {
                path: self.root.clone(),
                source,
            }),
        }
    }

    fn validate_layout(&self, owner_required: bool) -> Result<(), AdmissionError> {
        if !self.root_exists()? {
            return Ok(());
        }
        let entries = fs::read_dir(&self.root).map_err(|source| AdmissionError::Io {
            path: self.root.clone(),
            source,
        })?;
        let mut owner = false;
        let mut tickets = false;
        for entry in entries {
            let entry = entry.map_err(AdmissionError::ReadDir)?;
            let name = entry.file_name();
            let path = entry.path();
            match name.to_str() {
                Some(OWNER_FILE) => {
                    owner = true;
                    validate_regular(&path)?;
                }
                Some(QUEUE_LOCK) | Some(SLOT_LOCK) | Some(NEXT_TICKET) => {
                    validate_regular(&path)?;
                }
                Some(TICKETS_DIR) => {
                    let metadata =
                        fs::symlink_metadata(&path).map_err(|source| AdmissionError::Io {
                            path: path.clone(),
                            source,
                        })?;
                    if metadata.file_type().is_symlink() || !metadata.is_dir() {
                        return Err(AdmissionError::UnsafeLayout(path));
                    }
                    tickets = true;
                }
                _ => return Err(AdmissionError::UnsafeLayout(path)),
            }
        }
        if owner_required && !owner {
            return Err(AdmissionError::ForeignOwner(self.root.join(OWNER_FILE)));
        }
        if owner_required && !tickets {
            return Err(AdmissionError::UnsafeLayout(self.root.join(TICKETS_DIR)));
        }
        Ok(())
    }
}

pub struct AdmissionGuard {
    coordinator: AdmissionCoordinator,
    ticket_path: PathBuf,
    ticket: Option<File>,
    slot: Option<File>,
    ticket_id: String,
}

impl AdmissionGuard {
    pub fn ticket_id(&self) -> &str {
        &self.ticket_id
    }

    pub fn release(mut self) -> Result<(), AdmissionError> {
        self.release_inner()
    }

    fn release_inner(&mut self) -> Result<(), AdmissionError> {
        let mut queue = self.coordinator.open_queue(false)?;
        queue
            .lock_exclusive()
            .map_err(|source| AdmissionError::Lock {
                path: self.coordinator.root.join(QUEUE_LOCK),
                source,
            })?;
        if let Some(slot) = self.slot.take() {
            FileExt::unlock(&slot).map_err(|source| AdmissionError::Lock {
                path: self.coordinator.root.join(SLOT_LOCK),
                source,
            })?;
        }
        if let Some(ticket) = self.ticket.take() {
            FileExt::unlock(&ticket).map_err(|source| AdmissionError::Lock {
                path: self.ticket_path.clone(),
                source,
            })?;
        }
        fs::remove_file(&self.ticket_path).map_err(|source| AdmissionError::Io {
            path: self.ticket_path.clone(),
            source,
        })?;
        unlock(&mut queue)
    }
}

impl Drop for AdmissionGuard {
    fn drop(&mut self) {
        let _ = self.release_inner();
    }
}

struct TicketReservation {
    id: Option<String>,
    path: Option<PathBuf>,
    file: Option<File>,
}

impl TicketReservation {
    fn cleanup(mut self, coordinator: &AdmissionCoordinator) -> Result<(), AdmissionError> {
        let path = self
            .path
            .as_ref()
            .expect("reservation path is present")
            .clone();
        let file = self.file.as_ref().expect("reservation file is present");
        let mut queue = coordinator.open_queue(false)?;
        queue
            .lock_exclusive()
            .map_err(|source| AdmissionError::Lock {
                path: coordinator.root.join(QUEUE_LOCK),
                source,
            })?;
        FileExt::unlock(file).map_err(|source| AdmissionError::Lock {
            path: path.clone(),
            source,
        })?;
        fs::remove_file(&path).map_err(|source| AdmissionError::Io {
            path: path.clone(),
            source,
        })?;
        self.file = None;
        self.path = None;
        self.id = None;
        unlock(&mut queue)
    }
}

impl Drop for TicketReservation {
    fn drop(&mut self) {
        if let Some(file) = &self.file {
            let _ = FileExt::unlock(file);
        }
        if let Some(path) = &self.path {
            let _ = fs::remove_file(path);
        }
    }
}

fn empty_status() -> AdmissionStatusV1 {
    AdmissionStatusV1 {
        schema_version: ADMISSION_SCHEMA_VERSION.to_owned(),
        active: false,
        queue_count: 0,
        ticket_ids: Vec::new(),
    }
}

fn sleep_until(deadline: Instant, cancellation: &CancellationToken) {
    if cancellation.is_cancelled() {
        return;
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    thread::sleep(remaining.min(WAIT_INTERVAL));
}

fn unlock(file: &mut File) -> Result<(), AdmissionError> {
    FileExt::unlock(file).map_err(|source| AdmissionError::Lock {
        path: PathBuf::from("admission lock"),
        source,
    })
}

fn open_lock_file(path: &Path, create: bool) -> Result<File, AdmissionError> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (metadata.file_type().is_symlink() || !metadata.is_file())
    {
        return Err(AdmissionError::UnsafeLayout(path.to_path_buf()));
    }
    let result = if create {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
    } else {
        OpenOptions::new().read(true).write(true).open(path)
    };
    result.map_err(|source| AdmissionError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn open_existing_lock_file(path: &Path) -> Result<Option<File>, AdmissionError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(AdmissionError::UnsafeLayout(path.to_path_buf()));
            }
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map(Some)
                .map_err(|source| AdmissionError::Io {
                    path: path.to_path_buf(),
                    source,
                })
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(AdmissionError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn validate_regular(path: &Path) -> Result<(), AdmissionError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| AdmissionError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AdmissionError::UnsafeLayout(path.to_path_buf()));
    }
    Ok(())
}

fn read_ticket(path: &Path) -> Result<TicketMarker, AdmissionError> {
    let bytes = fs::read(path).map_err(|source| AdmissionError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|_| AdmissionError::MalformedTicket(path.to_path_buf()))
}

fn parse_ticket_name(path: &Path) -> Result<String, AdmissionError> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AdmissionError::MalformedTicket(path.to_path_buf()))?;
    let id = name
        .strip_prefix(TICKET_PREFIX)
        .and_then(|name| name.strip_suffix(TICKET_SUFFIX))
        .filter(|id| id.len() == 20 && id.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| AdmissionError::MalformedTicket(path.to_path_buf()))?;
    Ok(id.to_owned())
}

fn platform_root() -> Result<PathBuf, AdmissionError> {
    let platform = if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library").join("Caches").join(PLATFORM_DIRECTORY))
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|base| base.join(PLATFORM_DIRECTORY))
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
            .map(|base| base.join(PLATFORM_DIRECTORY))
    };
    platform.ok_or(AdmissionError::NoPersistentDefault)
}

fn validate_root_candidate(path: &Path) -> Result<PathBuf, AdmissionError> {
    if !path.is_absolute() {
        return Err(AdmissionError::UnsafePath(
            "coordinator root must be absolute",
        ));
    }
    if path
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(AdmissionError::UnsafePath(
            "coordinator root cannot contain dot or parent components",
        ));
    }
    let text = path
        .to_str()
        .ok_or(AdmissionError::UnsafePath("coordinator root must be UTF-8"))?;
    if text.starts_with('~')
        || text.contains('$')
        || (text.contains('%') && text.split('%').count() >= 3)
    {
        return Err(AdmissionError::UnsafePath(
            "coordinator root contains an unresolved variable or home shorthand",
        ));
    }
    reject_symlink_components(path)?;
    let resolved = canonicalize_existing_prefix(path)?;
    let temporary = canonicalize_existing_prefix(&std::env::temp_dir())?;
    if resolved == temporary || resolved.starts_with(&temporary) {
        return Err(AdmissionError::UnsafePath(
            "coordinator root cannot be temporary",
        ));
    }
    let current = std::env::current_dir().map_err(AdmissionError::CurrentDirectory)?;
    let current = canonicalize_existing_prefix(&current)?;
    if resolved == current || resolved.starts_with(&current) {
        return Err(AdmissionError::UnsafePath(
            "coordinator root cannot be the current directory",
        ));
    }
    if resolved.parent().is_none() {
        return Err(AdmissionError::UnsafePath(
            "coordinator root cannot be the filesystem root",
        ));
    }
    Ok(resolved)
}

fn reject_symlink_components(path: &Path) -> Result<(), AdmissionError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(AdmissionError::UnsafePath(
                    "coordinator root cannot traverse a symbolic link",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(AdmissionError::Io {
                    path: current,
                    source,
                });
            }
        }
    }
    Ok(())
}

fn canonicalize_existing_prefix(path: &Path) -> Result<PathBuf, AdmissionError> {
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
                let name = ancestor.file_name().ok_or(AdmissionError::UnsafePath(
                    "coordinator root has no existing absolute ancestor",
                ))?;
                tail.push(name.to_os_string());
                if !ancestor.pop() {
                    return Err(AdmissionError::UnsafePath(
                        "coordinator root has no existing absolute ancestor",
                    ));
                }
            }
            Err(source) => {
                return Err(AdmissionError::Io {
                    path: ancestor,
                    source,
                });
            }
        }
    }
}

#[derive(Debug)]
pub enum AdmissionError {
    NoPersistentDefault,
    InvalidTimeout,
    Timeout,
    Cancelled,
    QueueFull,
    UnsafePath(&'static str),
    UnsafeLayout(PathBuf),
    ForeignOwner(PathBuf),
    ForeignTicket(PathBuf),
    MalformedTicket(PathBuf),
    MalformedCounter(PathBuf),
    TicketCounterExhausted,
    CurrentDirectory(io::Error),
    ReadDir(io::Error),
    Io { path: PathBuf, source: io::Error },
    Lock { path: PathBuf, source: io::Error },
    Json(serde_json::Error),
}

impl AdmissionError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Timeout | Self::Cancelled => 5,
            Self::QueueFull => 4,
            Self::NoPersistentDefault
            | Self::InvalidTimeout
            | Self::UnsafePath(_)
            | Self::UnsafeLayout(_)
            | Self::ForeignOwner(_)
            | Self::ForeignTicket(_)
            | Self::MalformedTicket(_)
            | Self::MalformedCounter(_)
            | Self::TicketCounterExhausted
            | Self::CurrentDirectory(_)
            | Self::ReadDir(_)
            | Self::Io { .. }
            | Self::Lock { .. }
            | Self::Json(_) => 70,
        }
    }
}

impl fmt::Display for AdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPersistentDefault => formatter
                .write_str("no persistent platform cache is available for admission coordination"),
            Self::InvalidTimeout => {
                formatter.write_str("admission timeout must be greater than zero and representable")
            }
            Self::Timeout => formatter.write_str("admission queue timeout expired"),
            Self::Cancelled => formatter.write_str("admission wait was cancelled"),
            Self::QueueFull => formatter.write_str("admission queue is full"),
            Self::UnsafePath(message) => formatter.write_str(message),
            Self::UnsafeLayout(path) => write!(
                formatter,
                "unsafe admission coordinator layout at {}",
                path.display()
            ),
            Self::ForeignOwner(path) => write!(
                formatter,
                "admission coordinator ownership marker rejected at {}",
                path.display()
            ),
            Self::ForeignTicket(path) => write!(
                formatter,
                "foreign admission ticket rejected at {}",
                path.display()
            ),
            Self::MalformedTicket(path) => write!(
                formatter,
                "malformed admission ticket rejected at {}",
                path.display()
            ),
            Self::MalformedCounter(path) => write!(
                formatter,
                "malformed admission ticket counter at {}",
                path.display()
            ),
            Self::TicketCounterExhausted => {
                formatter.write_str("admission ticket counter exhausted")
            }
            Self::CurrentDirectory(_) => {
                formatter.write_str("current directory could not be resolved for admission safety")
            }
            Self::ReadDir(_) => {
                formatter.write_str("admission coordinator directory could not be read")
            }
            Self::Io { path, .. } => write!(
                formatter,
                "admission filesystem operation failed at {}",
                path.display()
            ),
            Self::Lock { path, .. } => write!(
                formatter,
                "admission lock operation failed at {}",
                path.display()
            ),
            Self::Json(_) => formatter.write_str("admission ticket serialization failed"),
        }
    }
}

impl std::error::Error for AdmissionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CurrentDirectory(error) | Self::ReadDir(error) => Some(error),
            Self::Io { source, .. } | Self::Lock { source, .. } => Some(source),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Child, ChildStdout, Command, Stdio};
    use std::thread;
    use std::time::Duration;

    use super::*;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "commit-ci-preflight-admission-test-{name}-{}",
            std::process::id()
        ))
    }

    fn coordinator(name: &str) -> AdmissionCoordinator {
        let root = test_root(name);
        let _ = fs::remove_dir_all(&root);
        AdmissionCoordinator::test_at(root)
    }

    struct ChildHandle {
        child: Child,
        output: BufReader<ChildStdout>,
    }

    impl ChildHandle {
        fn acquired(&mut self) -> String {
            loop {
                let mut line = String::new();
                let read = self.output.read_line(&mut line).expect("child output");
                assert!(read > 0, "child exited before admission marker");
                if line.starts_with("ACQUIRED ") {
                    return line.trim().to_owned();
                }
            }
        }

        fn finish(mut self) {
            let status = self.child.wait().expect("child exit");
            assert!(status.success(), "child failed: {status}");
        }
    }

    fn child(root: &Path, mode: &str, test_name: &str) -> ChildHandle {
        let mut command = Command::new(std::env::current_exe().expect("test executable"));
        let filter = format!("admission::tests::{test_name}");
        command
            .args(["--exact", &filter, "--nocapture"])
            .env("CCP_ADMISSION_TEST_ROOT", root)
            .env("CCP_ADMISSION_TEST_MODE", mode)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut child = command.spawn().expect("spawn admission child");
        let output = BufReader::new(child.stdout.take().expect("child stdout"));
        ChildHandle { child, output }
    }

    fn child_mode(test_name: &str) -> bool {
        let Some(root) = std::env::var_os("CCP_ADMISSION_TEST_ROOT") else {
            return false;
        };
        let mode = std::env::var("CCP_ADMISSION_TEST_MODE").expect("child mode");
        let coordinator = AdmissionCoordinator::test_at(PathBuf::from(root));
        let guard = coordinator
            .acquire(Duration::from_secs(10), &CancellationToken::default())
            .expect("child admission");
        println!("ACQUIRED {}", guard.ticket_id());
        std::io::stdout().flush().expect("flush child output");
        if mode == "hold" {
            thread::sleep(Duration::from_millis(250));
        } else {
            thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(test_name, "two_processes_serialize_and_preserve_fifo_order");
        true
    }

    #[test]
    fn two_processes_serialize_and_preserve_fifo_order() {
        if child_mode("two_processes_serialize_and_preserve_fifo_order") {
            return;
        }
        let root = test_root("fifo");
        let _ = fs::remove_dir_all(&root);
        let coordinator = AdmissionCoordinator::test_at(root.clone());
        let holder = coordinator
            .acquire(Duration::from_secs(2), &CancellationToken::default())
            .expect("holder");
        let mut first = child(
            &root,
            "short",
            "two_processes_serialize_and_preserve_fifo_order",
        );
        thread::sleep(Duration::from_millis(40));
        let mut second = child(
            &root,
            "short",
            "two_processes_serialize_and_preserve_fifo_order",
        );
        thread::sleep(Duration::from_millis(60));
        assert!(second.child.try_wait().expect("poll second").is_none());
        drop(holder);
        let first_id = first.acquired();
        let second_id = second.acquired();
        assert!(first_id < second_id, "FIFO ticket order was not preserved");
        first.finish();
        second.finish();
        fs::remove_dir_all(root).expect("remove test coordinator");
    }

    #[test]
    fn cancellation_and_timeout_remove_owned_tickets() {
        let coordinator = coordinator("cancel-timeout");
        let holder = coordinator
            .acquire(Duration::from_secs(2), &CancellationToken::default())
            .expect("holder");

        let cancellation = CancellationToken::default();
        let waiter_coordinator = coordinator.clone();
        let waiter_cancellation = cancellation.clone();
        let waiter = thread::spawn(move || {
            waiter_coordinator.acquire(Duration::from_secs(2), &waiter_cancellation)
        });
        thread::sleep(Duration::from_millis(60));
        cancellation.cancel();
        assert!(matches!(
            waiter.join().expect("cancellation waiter"),
            Err(AdmissionError::Cancelled)
        ));

        let timeout = coordinator.acquire(Duration::from_millis(80), &CancellationToken::default());
        assert!(matches!(timeout, Err(AdmissionError::Timeout)));
        assert_eq!(coordinator.status().expect("status").queue_count, 0);
        drop(holder);
        fs::remove_dir_all(coordinator.root()).expect("remove test coordinator");
    }

    #[test]
    fn crash_released_lock_reclaims_stale_ticket() {
        let root = test_root("crash");
        let _ = fs::remove_dir_all(&root);
        let mut crashed = child(
            &root,
            "hold",
            "two_processes_serialize_and_preserve_fifo_order",
        );
        crashed.acquired();
        crashed.child.kill().expect("kill child");
        let _ = crashed.child.wait().expect("wait crashed child");
        let coordinator = AdmissionCoordinator::test_at(root.clone());
        let guard = coordinator
            .acquire(Duration::from_secs(2), &CancellationToken::default())
            .expect("reclaim after released locks");
        drop(guard);
        fs::remove_dir_all(root).expect("remove test coordinator");
    }

    #[test]
    fn malformed_and_foreign_tickets_fail_closed() {
        let coordinator = coordinator("markers");
        let tickets = coordinator.root().join("tickets");
        fs::create_dir_all(&tickets).expect("ticket directory");
        let malformed = tickets.join("ticket-00000000000000000001.json");
        fs::write(&malformed, b"not-json\n").expect("malformed ticket");
        assert!(matches!(
            coordinator.acquire(Duration::from_secs(1), &CancellationToken::default()),
            Err(AdmissionError::MalformedTicket(_))
        ));
        fs::remove_file(malformed).expect("remove malformed marker");
        let foreign = tickets.join("ticket-00000000000000000001.json");
        fs::write(
            &foreign,
            br#"{"owner":"other","purpose":"host-admission-ticket","schema_version":"1.0","ticket_id":"00000000000000000001"}
"#,
        )
        .expect("foreign ticket");
        assert!(matches!(
            coordinator.acquire(Duration::from_secs(1), &CancellationToken::default()),
            Err(AdmissionError::ForeignTicket(_))
        ));
        fs::remove_dir_all(coordinator.root()).expect("remove test coordinator");
    }

    #[test]
    fn foreign_root_marker_fails_closed() {
        let coordinator = coordinator("owner");
        fs::create_dir_all(coordinator.root()).expect("coordinator root");
        fs::write(coordinator.root().join(OWNER_FILE), b"foreign\n").expect("foreign owner");
        assert!(matches!(
            coordinator.acquire(Duration::from_secs(1), &CancellationToken::default()),
            Err(AdmissionError::ForeignOwner(_))
        ));
        fs::remove_dir_all(coordinator.root()).expect("remove test coordinator");
    }

    #[test]
    fn status_is_bounded_and_excludes_sensitive_paths() {
        let coordinator = coordinator("status");
        let holder = coordinator
            .acquire(Duration::from_secs(2), &CancellationToken::default())
            .expect("holder");
        let cancellation = CancellationToken::default();
        let waiter_coordinator = coordinator.clone();
        let waiter_cancellation = cancellation.clone();
        let waiter = thread::spawn(move || {
            waiter_coordinator.acquire(Duration::from_secs(2), &waiter_cancellation)
        });
        thread::sleep(Duration::from_millis(60));
        let status = coordinator.status().expect("status");
        assert_eq!(status.schema_version, ADMISSION_SCHEMA_VERSION);
        assert!(status.active);
        assert_eq!(status.queue_count, 1);
        assert_eq!(status.ticket_ids.len(), 1);
        assert!(status.ticket_ids[0].starts_with("000"));
        let json = serde_json::to_string(&status).expect("status JSON");
        assert!(!json.contains(coordinator.root().to_str().expect("UTF-8 root")));
        assert!(!json.contains("commit-ci-preflight"));
        assert!(status.queue_count <= MAX_QUEUE_TICKETS);
        cancellation.cancel();
        assert!(matches!(
            waiter.join().expect("waiter"),
            Err(AdmissionError::Cancelled)
        ));
        drop(holder);
        fs::remove_dir_all(coordinator.root()).expect("remove test coordinator");
    }

    #[test]
    fn rejects_temporary_and_repository_roots() {
        assert!(AdmissionCoordinator::at(std::env::temp_dir()).is_err());
        let current = std::env::current_dir().expect("current directory");
        assert!(AdmissionCoordinator::at(current.clone()).is_err());
        assert!(AdmissionCoordinator::at(current.join("ccp-admission-descendant-test")).is_err());
    }

    #[test]
    fn status_schema_contains_only_bounded_safe_fields() {
        let status = empty_status();
        let value = serde_json::to_value(status).expect("status JSON");
        assert_eq!(value.as_object().expect("object").len(), 4);
        assert!(value.get("ticket_ids").expect("ticket ids").is_array());
        assert!(value.to_string().find("/").is_none());
    }

    #[test]
    fn platform_coordinator_uses_a_dedicated_cache_sibling() {
        let root = platform_root().expect("persistent platform cache");
        assert_eq!(
            root.file_name().and_then(|name| name.to_str()),
            Some(PLATFORM_DIRECTORY)
        );
        assert_ne!(
            root.parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str()),
            Some("commit-ci-preflight"),
            "the coordinator must not live inside the managed cache root"
        );
    }
}
