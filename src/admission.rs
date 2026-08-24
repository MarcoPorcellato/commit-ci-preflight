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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::durable_fs::{DurableFileSystem, DurableFsError};
use crate::process::CancellationToken;

pub const ADMISSION_SCHEMA_VERSION: &str = "1.0";
pub const ADMISSION_STATUS_SCHEMA_VERSION: &str = "2.0";
pub const DEFAULT_QUEUE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
pub const DEFAULT_STATUS_TIMEOUT: Duration = Duration::from_secs(5);
pub const MAX_QUEUE_TICKETS: usize = 1024;
pub const ADMISSION_LAYOUT_RECOVERY_SCHEMA_VERSION: &str = "admission-layout-recovery/1.0";
pub const DEFAULT_LAYOUT_RECOVERY_TIMEOUT: Duration = Duration::from_secs(5);
pub const MAX_LAYOUT_RECOVERY_TIMEOUT_SECONDS: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionLayoutRecoveryClassificationV1 {
    NotNeeded,
    RecoverableEmptyHistoricalAgentTickets,
    OperatorRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionLayoutRecoveryReasonV1 {
    CanonicalLayout,
    EmptyHistoricalAgentTickets,
    LockTimeout,
    ForeignOwner,
    UnsupportedLayout,
    TargetNotEmpty,
    CoordinatorNotIdle,
    QuarantineCollision,
    PlanMismatch,
    FilesystemUncertain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdmissionLayoutRecoveryStatusV1 {
    pub schema_version: String,
    pub classification: AdmissionLayoutRecoveryClassificationV1,
    pub target_kind: Option<String>,
    pub reason: AdmissionLayoutRecoveryReasonV1,
    pub plan_sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionLayoutRecoveryOutcomeV1 {
    Recovered,
    NotApplied,
    RecoveryUncertain,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdmissionLayoutRecoveryApplyV1 {
    pub schema_version: String,
    pub outcome: AdmissionLayoutRecoveryOutcomeV1,
    pub reason: AdmissionLayoutRecoveryReasonV1,
    pub quarantine_entry: Option<String>,
}

#[derive(Serialize)]
struct AdmissionLayoutRecoveryPlanV1 {
    schema_version: &'static str,
    recovery_kind: &'static str,
    owner: String,
    purpose: String,
    owner_schema_version: String,
    root_entries: Vec<RecoveryRootEntryV1>,
    queue_lock_name: &'static str,
    queue_lock_kind: &'static str,
    queue_lock_exclusively_held: bool,
    slot_lock_name: &'static str,
    slot_lock_kind: &'static str,
    slot_lock_was_free: bool,
    ticket_count: usize,
    lease_count: usize,
    target_entry_count: usize,
}
#[derive(Serialize)]
struct RecoveryRootEntryV1 {
    name: String,
    kind: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CoordinatorOwnerMarkerV1 {
    owner: String,
    purpose: String,
    schema_version: String,
}

const OWNER_FILE: &str = ".ccp-admission-root-v1.json";
const PLATFORM_DIRECTORY: &str = "commit-ci-preflight-admission";
const OWNER_BYTES: &[u8] =
    b"{\"owner\":\"commit-ci-preflight\",\"purpose\":\"host-admission-coordinator\",\"schema_version\":\"1.0\"}\n";
const QUEUE_LOCK: &str = "queue.lock";
const SLOT_LOCK: &str = "slot.lock";
const NEXT_TICKET: &str = "next-ticket-v1";
const TICKETS_DIR: &str = "tickets";
const TICKET_STAGING_PREFIX: &str = ".ticket-staging-";
const TICKET_PREFIX: &str = "ticket-";
const TICKET_SUFFIX: &str = ".json";
const QUARANTINE_DIR: &str = "quarantine";
const LEASES_DIR: &str = "leases";
const LEASE_PREFIX: &str = "lease-";
const LEASE_SUFFIX: &str = ".json";
const LEASE_DURATION: Duration = Duration::from_secs(30);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const WAIT_INTERVAL: Duration = Duration::from_millis(25);
const PROCESS_VISIBILITY_NOTE: &str =
    "No process visible in the local shell does not prove global inactivity.";
static QUARANTINE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy)]
struct AdmissionDeadline {
    at: Instant,
}

impl AdmissionDeadline {
    fn from_timeout(timeout: Duration) -> Result<Self, AdmissionError> {
        if timeout.is_zero() {
            return Err(AdmissionError::InvalidTimeout);
        }
        Ok(Self {
            at: Instant::now()
                .checked_add(timeout)
                .ok_or(AdmissionError::InvalidTimeout)?,
        })
    }

    fn check(&self, cancellation: &CancellationToken) -> Result<(), AdmissionError> {
        if cancellation.is_cancelled() {
            return Err(AdmissionError::Cancelled);
        }
        if Instant::now() >= self.at {
            return Err(AdmissionError::Timeout);
        }
        Ok(())
    }

    fn remaining(&self) -> Duration {
        self.at.saturating_duration_since(Instant::now())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdmissionStatusV1 {
    pub schema_version: String,
    pub active: bool,
    pub queue_count: usize,
    pub ticket_ids: Vec<String>,
    pub slot: AdmissionLockStatusV1,
    pub queue_lock: AdmissionLockStatusV1,
    pub process_visibility_note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdmissionLockStatusV1 {
    pub kind: String,
    pub state: String,
    pub owner_run_id: Option<String>,
    pub acquired_at_unix_seconds: Option<u64>,
    pub heartbeat_at_unix_seconds: Option<u64>,
    pub lease_state: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TicketMarker {
    owner: String,
    purpose: String,
    schema_version: String,
    ticket_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LeaseMarker {
    owner: String,
    purpose: String,
    schema_version: String,
    owner_run_id: String,
    acquired_at_unix_seconds: u64,
    heartbeat_at_unix_seconds: u64,
    state: String,
}

#[derive(Debug)]
struct TicketInfo {
    id: String,
    path: PathBuf,
}

#[derive(Debug)]
struct HeartbeatHandle {
    stop: Option<Sender<()>>,
    join: Option<thread::JoinHandle<()>>,
}

impl HeartbeatHandle {
    fn start(path: PathBuf, owner_run_id: String) -> Self {
        let (stop, thread_stop) = mpsc::channel();
        let join = thread::spawn(move || {
            loop {
                match thread_stop.recv_timeout(HEARTBEAT_INTERVAL) {
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {}
                }
                let Ok(mut lease) = read_lease(&path) else {
                    break;
                };
                if lease.owner_run_id != owner_run_id || lease.state != "active" {
                    break;
                }
                let Ok(now) = unix_seconds() else {
                    break;
                };
                lease.heartbeat_at_unix_seconds = now;
                if write_lease(&path, &lease).is_err() {
                    break;
                }
            }
        });
        Self {
            stop: Some(stop),
            join: Some(join),
        }
    }

    fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for HeartbeatHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Debug)]
struct StaleTicket {
    ticket: TicketInfo,
    file: File,
}

#[derive(Debug, Clone)]
pub struct AdmissionCoordinator {
    root: PathBuf,
    #[cfg(test)]
    durable_fault: Option<usize>,
}

impl AdmissionCoordinator {
    #[cfg(test)]
    pub(crate) fn test_at(root: PathBuf) -> Self {
        Self {
            root,
            durable_fault: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn test_at_with_durable_fault(root: PathBuf, fail_at: usize) -> Self {
        Self {
            root,
            durable_fault: Some(fail_at),
        }
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
        Ok(Self {
            root,
            #[cfg(test)]
            durable_fault: None,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn acquire(
        &self,
        timeout: Duration,
        cancellation: &CancellationToken,
    ) -> Result<AdmissionGuard, AdmissionError> {
        let deadline = AdmissionDeadline::from_timeout(timeout)?;
        deadline.check(cancellation)?;
        self.initialize_until(&deadline, cancellation)?;
        let mut queue = self.lock_queue_until(&deadline, cancellation)?;
        let (live, stale) = self.scan_tickets(None, true)?;
        self.remove_stale(stale)?;
        if live.len() >= MAX_QUEUE_TICKETS {
            unlock(&mut queue)?;
            return Err(AdmissionError::QueueFull);
        }
        let mut reservation = self.create_ticket(&deadline, cancellation)?;
        unlock(&mut queue)?;

        loop {
            if let Err(error) = deadline.check(cancellation) {
                reservation.cleanup(self)?;
                return Err(error);
            }

            let mut queue = self.lock_queue_until(&deadline, cancellation)?;
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
            if first_is_ours && let Some(slot) = self.try_lock_slot(&deadline, cancellation)? {
                let ticket_id = reservation
                    .id
                    .as_deref()
                    .expect("reservation id is present");
                let lease_path = reservation
                    .lease_path
                    .as_ref()
                    .expect("reservation lease path is present")
                    .clone();
                if let Err(error) = self.activate_lease(&lease_path, ticket_id) {
                    let _ = FileExt::unlock(&slot);
                    unlock(&mut queue)?;
                    reservation.cleanup(self)?;
                    return Err(error);
                }
                let lease_path = reservation
                    .lease_path
                    .take()
                    .expect("reservation lease path is present");
                let heartbeat = HeartbeatHandle::start(lease_path.clone(), ticket_id.to_owned());
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
                    heartbeat: Some(heartbeat),
                });
            }
            unlock(&mut queue)?;
            sleep_until(deadline.at, cancellation);
        }
    }

    pub fn status(&self) -> Result<AdmissionStatusV1, AdmissionError> {
        let cancellation = CancellationToken::default();
        self.status_with_timeout(DEFAULT_STATUS_TIMEOUT, &cancellation)
    }

    pub fn status_with_timeout(
        &self,
        timeout: Duration,
        cancellation: &CancellationToken,
    ) -> Result<AdmissionStatusV1, AdmissionError> {
        let deadline = AdmissionDeadline::from_timeout(timeout)?;
        deadline.check(cancellation)?;
        if !self.root_exists()? {
            return Ok(empty_status());
        }
        self.validate_layout(true)?;
        let queue_lock = self.lock_status(QUEUE_LOCK, "queue_lock")?;
        let mut queue = self.lock_queue_until(&deadline, cancellation)?;
        let (live, _stale) = self.scan_tickets(None, false)?;
        let slot = self.slot_status()?;
        let active = slot.state == "held";
        unlock(&mut queue)?;
        let first = usize::from(active && !live.is_empty());
        let ticket_ids: Vec<String> = live
            .into_iter()
            .skip(first)
            .map(|ticket| ticket.id)
            .collect();
        Ok(AdmissionStatusV1 {
            schema_version: ADMISSION_STATUS_SCHEMA_VERSION.to_owned(),
            active,
            queue_count: ticket_ids.len(),
            ticket_ids,
            slot,
            queue_lock,
            process_visibility_note: PROCESS_VISIBILITY_NOTE.to_owned(),
        })
    }

    pub fn layout_recovery_status_with_timeout(
        &self,
        timeout: Duration,
        cancellation: &CancellationToken,
    ) -> AdmissionLayoutRecoveryStatusV1 {
        let base = || AdmissionLayoutRecoveryStatusV1 {
            schema_version: ADMISSION_LAYOUT_RECOVERY_SCHEMA_VERSION.to_owned(),
            classification: AdmissionLayoutRecoveryClassificationV1::OperatorRequired,
            target_kind: None,
            reason: AdmissionLayoutRecoveryReasonV1::FilesystemUncertain,
            plan_sha256: None,
        };
        let Ok(deadline) = AdmissionDeadline::from_timeout(timeout) else {
            return base();
        };
        if !self.root_exists().unwrap_or(false) {
            return AdmissionLayoutRecoveryStatusV1 {
                reason: AdmissionLayoutRecoveryReasonV1::UnsupportedLayout,
                ..base()
            };
        }
        let Ok(entries) = fs::read_dir(&self.root) else {
            return base();
        };
        let mut root_entries = Vec::new();
        let mut target = false;
        let mut required = [false; 5];
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == "agent-tickets" {
                target = true;
                continue;
            }
            let known = match name.as_str() {
                OWNER_FILE => {
                    required[0] = true;
                    true
                }
                QUEUE_LOCK => {
                    required[1] = true;
                    true
                }
                SLOT_LOCK => {
                    required[2] = true;
                    true
                }
                NEXT_TICKET => {
                    required[3] = true;
                    true
                }
                TICKETS_DIR => {
                    required[4] = true;
                    true
                }
                LEASES_DIR | QUARANTINE_DIR => true,
                _ => false,
            };
            if !known {
                return AdmissionLayoutRecoveryStatusV1 {
                    reason: AdmissionLayoutRecoveryReasonV1::UnsupportedLayout,
                    ..base()
                };
            }
            let Ok(meta) = fs::symlink_metadata(&path) else {
                return base();
            };
            if meta.file_type().is_symlink() || (!meta.is_dir() && !meta.is_file()) {
                return AdmissionLayoutRecoveryStatusV1 {
                    reason: AdmissionLayoutRecoveryReasonV1::UnsupportedLayout,
                    ..base()
                };
            }
            if matches!(name.as_str(), TICKETS_DIR | LEASES_DIR | QUARANTINE_DIR) && !meta.is_dir()
            {
                return AdmissionLayoutRecoveryStatusV1 {
                    reason: AdmissionLayoutRecoveryReasonV1::UnsupportedLayout,
                    ..base()
                };
            }
            if matches!(
                name.as_str(),
                OWNER_FILE | QUEUE_LOCK | SLOT_LOCK | NEXT_TICKET
            ) && !meta.is_file()
            {
                return AdmissionLayoutRecoveryStatusV1 {
                    reason: AdmissionLayoutRecoveryReasonV1::UnsupportedLayout,
                    ..base()
                };
            }
            root_entries.push(RecoveryRootEntryV1 {
                name,
                kind: if meta.is_dir() { "directory" } else { "file" }.to_owned(),
            });
        }
        if !required.into_iter().all(|value| value) {
            return AdmissionLayoutRecoveryStatusV1 {
                reason: AdmissionLayoutRecoveryReasonV1::UnsupportedLayout,
                ..base()
            };
        }
        let Ok(owner_bytes) = fs::read(self.root.join(OWNER_FILE)) else {
            return AdmissionLayoutRecoveryStatusV1 {
                reason: AdmissionLayoutRecoveryReasonV1::ForeignOwner,
                ..base()
            };
        };
        if owner_bytes != OWNER_BYTES {
            return AdmissionLayoutRecoveryStatusV1 {
                reason: AdmissionLayoutRecoveryReasonV1::ForeignOwner,
                ..base()
            };
        }
        if serde_json::from_slice::<CoordinatorOwnerMarkerV1>(&owner_bytes).is_err() {
            return AdmissionLayoutRecoveryStatusV1 {
                reason: AdmissionLayoutRecoveryReasonV1::ForeignOwner,
                ..base()
            };
        }
        let Ok(counter) = fs::read_to_string(self.root.join(NEXT_TICKET)) else {
            return AdmissionLayoutRecoveryStatusV1 {
                reason: AdmissionLayoutRecoveryReasonV1::UnsupportedLayout,
                ..base()
            };
        };
        if counter
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)
            .is_none()
        {
            return AdmissionLayoutRecoveryStatusV1 {
                reason: AdmissionLayoutRecoveryReasonV1::UnsupportedLayout,
                ..base()
            };
        }
        for directory in [TICKETS_DIR, LEASES_DIR] {
            let path = self.root.join(directory);
            if fs::read_dir(path)
                .map(|mut entries| entries.next().is_some())
                .unwrap_or(true)
            {
                return AdmissionLayoutRecoveryStatusV1 {
                    reason: AdmissionLayoutRecoveryReasonV1::CoordinatorNotIdle,
                    ..base()
                };
            }
        }
        let Ok(mut snapshot_queue) = self.open_queue(false) else {
            return AdmissionLayoutRecoveryStatusV1 {
                reason: AdmissionLayoutRecoveryReasonV1::UnsupportedLayout,
                ..base()
            };
        };
        if lock_exclusive_until(
            &snapshot_queue,
            &self.root.join(QUEUE_LOCK),
            &deadline,
            cancellation,
        )
        .is_err()
        {
            return AdmissionLayoutRecoveryStatusV1 {
                reason: AdmissionLayoutRecoveryReasonV1::LockTimeout,
                ..base()
            };
        }
        let Ok(snapshot_slot) = open_existing_lock_file(&self.root.join(SLOT_LOCK))
            .and_then(|x| x.ok_or(AdmissionError::UnsafeLayout(self.root.join(SLOT_LOCK))))
        else {
            let _ = unlock(&mut snapshot_queue);
            return AdmissionLayoutRecoveryStatusV1 {
                reason: AdmissionLayoutRecoveryReasonV1::UnsupportedLayout,
                ..base()
            };
        };
        if snapshot_slot.try_lock_exclusive().is_err() {
            let _ = unlock(&mut snapshot_queue);
            return AdmissionLayoutRecoveryStatusV1 {
                reason: AdmissionLayoutRecoveryReasonV1::LockTimeout,
                ..base()
            };
        }
        let mut snapshot_slot = snapshot_slot;
        let _ = unlock(&mut snapshot_slot);
        let _ = unlock(&mut snapshot_queue);
        if !target {
            return AdmissionLayoutRecoveryStatusV1 {
                classification: AdmissionLayoutRecoveryClassificationV1::NotNeeded,
                reason: AdmissionLayoutRecoveryReasonV1::CanonicalLayout,
                ..base()
            };
        }
        let target_path = self.root.join("agent-tickets");
        let Ok(meta) = fs::symlink_metadata(&target_path) else {
            return base();
        };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return AdmissionLayoutRecoveryStatusV1 {
                reason: AdmissionLayoutRecoveryReasonV1::UnsupportedLayout,
                ..base()
            };
        }
        if fs::read_dir(&target_path)
            .map(|mut d| d.next().is_some())
            .unwrap_or(true)
        {
            return AdmissionLayoutRecoveryStatusV1 {
                reason: AdmissionLayoutRecoveryReasonV1::TargetNotEmpty,
                ..base()
            };
        }
        let Ok(owner) = fs::read(self.root.join(OWNER_FILE)) else {
            return AdmissionLayoutRecoveryStatusV1 {
                reason: AdmissionLayoutRecoveryReasonV1::ForeignOwner,
                ..base()
            };
        };
        if owner != OWNER_BYTES {
            return AdmissionLayoutRecoveryStatusV1 {
                reason: AdmissionLayoutRecoveryReasonV1::ForeignOwner,
                ..base()
            };
        }
        let Ok(owner_marker) = serde_json::from_slice::<CoordinatorOwnerMarkerV1>(&owner) else {
            return AdmissionLayoutRecoveryStatusV1 {
                reason: AdmissionLayoutRecoveryReasonV1::ForeignOwner,
                ..base()
            };
        };
        let Ok(mut queue) = self.open_queue(false) else {
            return base();
        };
        if lock_exclusive_until(&queue, &self.root.join(QUEUE_LOCK), &deadline, cancellation)
            .is_err()
        {
            return AdmissionLayoutRecoveryStatusV1 {
                reason: AdmissionLayoutRecoveryReasonV1::LockTimeout,
                ..base()
            };
        }
        let slot = match open_existing_lock_file(&self.root.join(SLOT_LOCK)) {
            Ok(Some(file)) => file,
            Ok(None) => {
                let _ = unlock(&mut queue);
                return AdmissionLayoutRecoveryStatusV1 {
                    reason: AdmissionLayoutRecoveryReasonV1::UnsupportedLayout,
                    ..base()
                };
            }
            Err(_) => {
                let _ = unlock(&mut queue);
                return base();
            }
        };
        let slot_free = slot.try_lock_exclusive().is_ok();
        if !slot_free {
            let _ = unlock(&mut queue);
            return AdmissionLayoutRecoveryStatusV1 {
                reason: AdmissionLayoutRecoveryReasonV1::CoordinatorNotIdle,
                ..base()
            };
        }
        let mut slot = slot;
        let _ = unlock(&mut slot);
        let _ = unlock(&mut queue);
        root_entries.sort_by(|a, b| a.name.cmp(&b.name));
        let plan = AdmissionLayoutRecoveryPlanV1 {
            schema_version: ADMISSION_LAYOUT_RECOVERY_SCHEMA_VERSION,
            recovery_kind: "empty_historical_agent_tickets",
            owner: owner_marker.owner,
            purpose: owner_marker.purpose,
            owner_schema_version: owner_marker.schema_version,
            root_entries,
            queue_lock_name: QUEUE_LOCK,
            queue_lock_kind: "queue_lock",
            queue_lock_exclusively_held: true,
            slot_lock_name: SLOT_LOCK,
            slot_lock_kind: "slot_lock",
            slot_lock_was_free: true,
            ticket_count: 0,
            lease_count: 0,
            target_entry_count: 0,
        };
        let Ok(bytes) = serde_json::to_vec(&plan) else {
            return base();
        };
        let digest = Sha256::digest(bytes);
        let plan_sha256 = digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let quarantine = self
            .root
            .join(QUARANTINE_DIR)
            .join(format!("agent-tickets.recovered-v1-{plan_sha256}"));
        if quarantine.exists() {
            return AdmissionLayoutRecoveryStatusV1 {
                reason: AdmissionLayoutRecoveryReasonV1::QuarantineCollision,
                ..base()
            };
        }
        AdmissionLayoutRecoveryStatusV1 {
            schema_version: ADMISSION_LAYOUT_RECOVERY_SCHEMA_VERSION.to_owned(),
            classification:
                AdmissionLayoutRecoveryClassificationV1::RecoverableEmptyHistoricalAgentTickets,
            target_kind: Some("historical_agent_tickets".into()),
            reason: AdmissionLayoutRecoveryReasonV1::EmptyHistoricalAgentTickets,
            plan_sha256: Some(plan_sha256),
        }
    }

    fn locked_layout_plan_sha256(&self) -> Option<String> {
        let owner = serde_json::from_slice::<CoordinatorOwnerMarkerV1>(
            &fs::read(self.root.join(OWNER_FILE)).ok()?,
        )
        .ok()?;
        let mut root_entries = Vec::new();
        for entry in fs::read_dir(&self.root).ok()?.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let meta = fs::symlink_metadata(&path).ok()?;
            if meta.file_type().is_symlink() {
                return None;
            }
            if name == "agent-tickets" {
                continue;
            }
            root_entries.push(RecoveryRootEntryV1 {
                name,
                kind: if meta.is_dir() {
                    "directory".into()
                } else {
                    "file".into()
                },
            });
        }
        root_entries.sort_by(|a, b| a.name.cmp(&b.name));
        let plan = AdmissionLayoutRecoveryPlanV1 {
            schema_version: ADMISSION_LAYOUT_RECOVERY_SCHEMA_VERSION,
            recovery_kind: "empty_historical_agent_tickets",
            owner: owner.owner,
            purpose: owner.purpose,
            owner_schema_version: owner.schema_version,
            root_entries,
            queue_lock_name: QUEUE_LOCK,
            queue_lock_kind: "queue_lock",
            queue_lock_exclusively_held: true,
            slot_lock_name: SLOT_LOCK,
            slot_lock_kind: "slot_lock",
            slot_lock_was_free: true,
            ticket_count: 0,
            lease_count: 0,
            target_entry_count: 0,
        };
        let bytes = serde_json::to_vec(&plan).ok()?;
        Some(
            Sha256::digest(bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        )
    }

    pub fn apply_layout_recovery_with_timeout(
        &self,
        expected_plan: &str,
        timeout: Duration,
        cancellation: &CancellationToken,
    ) -> AdmissionLayoutRecoveryApplyV1 {
        let report = |outcome, reason, entry: Option<String>| AdmissionLayoutRecoveryApplyV1 {
            schema_version: ADMISSION_LAYOUT_RECOVERY_SCHEMA_VERSION.to_owned(),
            outcome,
            reason,
            quarantine_entry: entry,
        };
        if expected_plan.len() != 64
            || !expected_plan
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return report(
                AdmissionLayoutRecoveryOutcomeV1::NotApplied,
                AdmissionLayoutRecoveryReasonV1::PlanMismatch,
                None,
            );
        }
        let Ok(deadline) = AdmissionDeadline::from_timeout(timeout) else {
            return report(
                AdmissionLayoutRecoveryOutcomeV1::NotApplied,
                AdmissionLayoutRecoveryReasonV1::LockTimeout,
                None,
            );
        };
        let Ok(mut queue) = self.open_queue(false) else {
            return report(
                AdmissionLayoutRecoveryOutcomeV1::NotApplied,
                AdmissionLayoutRecoveryReasonV1::FilesystemUncertain,
                None,
            );
        };
        if lock_exclusive_until(&queue, &self.root.join(QUEUE_LOCK), &deadline, cancellation)
            .is_err()
        {
            return report(
                AdmissionLayoutRecoveryOutcomeV1::NotApplied,
                AdmissionLayoutRecoveryReasonV1::LockTimeout,
                None,
            );
        }
        let mut slot = match open_existing_lock_file(&self.root.join(SLOT_LOCK)) {
            Ok(Some(f)) => f,
            _ => {
                let _ = unlock(&mut queue);
                return report(
                    AdmissionLayoutRecoveryOutcomeV1::NotApplied,
                    AdmissionLayoutRecoveryReasonV1::FilesystemUncertain,
                    None,
                );
            }
        };
        if slot.try_lock_exclusive().is_err() {
            let _ = unlock(&mut queue);
            return report(
                AdmissionLayoutRecoveryOutcomeV1::NotApplied,
                AdmissionLayoutRecoveryReasonV1::CoordinatorNotIdle,
                None,
            );
        }
        if !self.root.join("agent-tickets").exists() {
            let _ = unlock(&mut slot);
            let _ = unlock(&mut queue);
            return report(
                AdmissionLayoutRecoveryOutcomeV1::NotApplied,
                AdmissionLayoutRecoveryReasonV1::CanonicalLayout,
                None,
            );
        }
        let Some(plan) = self.locked_layout_plan_sha256() else {
            let _ = unlock(&mut slot);
            let _ = unlock(&mut queue);
            return report(
                AdmissionLayoutRecoveryOutcomeV1::NotApplied,
                AdmissionLayoutRecoveryReasonV1::FilesystemUncertain,
                None,
            );
        };
        if plan != expected_plan {
            let _ = unlock(&mut slot);
            let _ = unlock(&mut queue);
            return report(
                AdmissionLayoutRecoveryOutcomeV1::NotApplied,
                AdmissionLayoutRecoveryReasonV1::PlanMismatch,
                None,
            );
        }
        let entry = format!("agent-tickets.recovered-v1-{plan}");
        let source = self.root.join("agent-tickets");
        let quarantine = self.root.join(QUARANTINE_DIR);
        let destination = quarantine.join(&entry);
        #[cfg(test)]
        let durable_fs = self
            .durable_fault
            .map(DurableFileSystem::failing_at)
            .unwrap_or_default();
        #[cfg(not(test))]
        let durable_fs = DurableFileSystem::default();
        let result = durable_fs.relocate_empty_directory(&source, &destination);
        let outcome = match result {
            Ok(()) => {
                if self.validate_layout(true).is_ok() {
                    (
                        AdmissionLayoutRecoveryOutcomeV1::Recovered,
                        AdmissionLayoutRecoveryReasonV1::EmptyHistoricalAgentTickets,
                        Some(entry),
                    )
                } else {
                    (
                        AdmissionLayoutRecoveryOutcomeV1::RecoveryUncertain,
                        AdmissionLayoutRecoveryReasonV1::FilesystemUncertain,
                        Some(entry),
                    )
                }
            }
            Err(_) => {
                let outcome = outcome_after_relocation_error(&source, &destination);
                let entry =
                    if matches!(outcome, AdmissionLayoutRecoveryOutcomeV1::RecoveryUncertain)
                        && destination.exists()
                    {
                        Some(entry)
                    } else {
                        None
                    };
                (
                    outcome,
                    AdmissionLayoutRecoveryReasonV1::FilesystemUncertain,
                    entry,
                )
            }
        };
        let unlock_slot = unlock(&mut slot);
        let unlock_queue = unlock(&mut queue);
        if unlock_slot.is_err() || unlock_queue.is_err() {
            return report(
                AdmissionLayoutRecoveryOutcomeV1::RecoveryUncertain,
                AdmissionLayoutRecoveryReasonV1::FilesystemUncertain,
                outcome.2,
            );
        }
        report(outcome.0, outcome.1, outcome.2)
    }

    #[cfg(test)]
    fn initialize(&self) -> Result<(), AdmissionError> {
        let cancellation = CancellationToken::default();
        let deadline = AdmissionDeadline::from_timeout(DEFAULT_QUEUE_TIMEOUT)
            .expect("default admission timeout is representable");
        self.initialize_until(&deadline, &cancellation)
    }

    fn initialize_until(
        &self,
        deadline: &AdmissionDeadline,
        cancellation: &CancellationToken,
    ) -> Result<(), AdmissionError> {
        deadline.check(cancellation)?;
        fs::create_dir_all(&self.root).map_err(|source| AdmissionError::Io {
            path: self.root.clone(),
            source,
        })?;
        self.validate_layout(false)?;
        let mut queue = self.open_queue(true)?;
        lock_exclusive_until(&queue, &self.root.join(QUEUE_LOCK), deadline, cancellation)?;
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
        let leases = self.root.join(LEASES_DIR);
        if let Ok(metadata) = fs::symlink_metadata(&leases) {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(AdmissionError::UnsafeLayout(leases));
            }
        } else {
            fs::create_dir(&leases).map_err(|source| AdmissionError::Io {
                path: leases.clone(),
                source,
            })?;
        }
        let quarantine = self.root.join(QUARANTINE_DIR);
        if let Ok(metadata) = fs::symlink_metadata(&quarantine) {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(AdmissionError::UnsafeLayout(quarantine));
            }
        } else {
            fs::create_dir(&quarantine).map_err(|source| AdmissionError::Io {
                path: quarantine.clone(),
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

    fn create_ticket(
        &self,
        deadline: &AdmissionDeadline,
        cancellation: &CancellationToken,
    ) -> Result<TicketReservation, AdmissionError> {
        let id = self.next_ticket_id(deadline, cancellation)?;
        let path = self
            .root
            .join(TICKETS_DIR)
            .join(format!("{TICKET_PREFIX}{id}{TICKET_SUFFIX}"));
        let staging_path = self.root.join(TICKETS_DIR).join(format!(
            "{TICKET_STAGING_PREFIX}{id}-{}-{}.json",
            std::process::id(),
            QUARANTINE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let marker = TicketMarker {
            owner: "commit-ci-preflight".to_owned(),
            purpose: "host-admission-ticket".to_owned(),
            schema_version: ADMISSION_SCHEMA_VERSION.to_owned(),
            ticket_id: id.clone(),
        };
        let mut bytes = serde_json::to_vec(&marker).map_err(AdmissionError::Json)?;
        bytes.push(b'\n');
        DurableFileSystem::default()
            .create_new(&staging_path, &bytes)
            .map_err(|error| durable_error(staging_path.clone(), error))?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&staging_path)
            .map_err(|source| AdmissionError::Io {
                path: staging_path.clone(),
                source,
            })?;
        lock_exclusive_until(&file, &staging_path, deadline, cancellation)?;
        fs::rename(&staging_path, &path).map_err(|source| AdmissionError::Io {
            path: path.clone(),
            source,
        })?;
        let now = unix_seconds()?;
        let lease_path = self.lease_path(&id);
        let lease = LeaseMarker {
            owner: "commit-ci-preflight".to_owned(),
            purpose: "host-admission-lease".to_owned(),
            schema_version: ADMISSION_SCHEMA_VERSION.to_owned(),
            owner_run_id: id.clone(),
            acquired_at_unix_seconds: now,
            heartbeat_at_unix_seconds: now,
            state: "queued".to_owned(),
        };
        if let Err(error) = create_lease(&lease_path, &lease) {
            let _ = FileExt::unlock(&file);
            let _ = fs::remove_file(&path);
            return Err(error);
        }
        Ok(TicketReservation {
            id: Some(id),
            path: Some(path),
            file: Some(file),
            lease_path: Some(lease_path),
        })
    }

    fn next_ticket_id(
        &self,
        deadline: &AdmissionDeadline,
        cancellation: &CancellationToken,
    ) -> Result<String, AdmissionError> {
        deadline.check(cancellation)?;
        let directory = self.root.join(TICKETS_DIR);
        let entries = fs::read_dir(&directory).map_err(|source| AdmissionError::Io {
            path: directory.clone(),
            source,
        })?;
        let mut highest = 0u64;
        for entry in entries {
            deadline.check(cancellation)?;
            let entry = entry.map_err(AdmissionError::ReadDir)?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|source| AdmissionError::Io {
                path: path.clone(),
                source,
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(AdmissionError::MalformedTicket(path));
            }
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| AdmissionError::MalformedTicket(path.clone()))?;
            if name.starts_with(TICKET_STAGING_PREFIX) {
                continue;
            }
            let id = parse_ticket_name(&path)?;
            let value = id
                .parse::<u64>()
                .map_err(|_| AdmissionError::MalformedTicket(path.clone()))?;
            highest = highest.max(value);
        }
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
        let next = highest.max(next.saturating_sub(1));
        let next = next
            .checked_add(1)
            .ok_or(AdmissionError::TicketCounterExhausted)?;
        let next_after = next
            .checked_add(1)
            .ok_or(AdmissionError::TicketCounterExhausted)?;
        let counter = format!("{next_after}\n");
        DurableFileSystem::default()
            .atomic_replace(&path, counter.as_bytes())
            .map_err(|error| durable_error(path.clone(), error))?;
        Ok(format!("{next:020}"))
    }

    fn lock_queue_until(
        &self,
        deadline: &AdmissionDeadline,
        cancellation: &CancellationToken,
    ) -> Result<File, AdmissionError> {
        let path = self.root.join(QUEUE_LOCK);
        let file = self.open_queue(false)?;
        lock_exclusive_until(&file, &path, deadline, cancellation)?;
        Ok(file)
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
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| AdmissionError::MalformedTicket(path.clone()))?;
            if name.starts_with(TICKET_STAGING_PREFIX) {
                if reclaim_stale {
                    self.quarantine_file(&path)?;
                    continue;
                }
                return Err(AdmissionError::RecoveryRequired(path));
            }
            let id = parse_ticket_name(&path)?;
            let marker = match read_ticket(&path) {
                Ok(marker) => marker,
                Err(AdmissionError::MalformedTicket(_)) if reclaim_stale => {
                    let file = OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(&path)
                        .map_err(|source| AdmissionError::Io {
                            path: path.clone(),
                            source,
                        })?;
                    match file.try_lock_exclusive() {
                        Ok(()) => {
                            FileExt::unlock(&file).map_err(|source| AdmissionError::Lock {
                                path: path.clone(),
                                source,
                            })?;
                            self.quarantine_file(&path)?;
                            continue;
                        }
                        Err(source) if source.kind() == io::ErrorKind::WouldBlock => {
                            return Err(AdmissionError::MalformedTicket(path));
                        }
                        Err(source) => return Err(AdmissionError::Lock { path, source }),
                    }
                }
                Err(error) => return Err(error),
            };
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
                    let lease = self.read_lease(&ticket.id)?;
                    if reclaim_stale
                        && (lease.is_none() || lease.as_ref().is_some_and(lease_is_expired))
                    {
                        stale.push(StaleTicket { ticket, file });
                    } else {
                        FileExt::unlock(&file).map_err(|source| AdmissionError::Lock {
                            path: ticket.path.clone(),
                            source,
                        })?;
                        live.push(ticket);
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
            FileExt::unlock(&stale.file).map_err(|source| AdmissionError::Lock {
                path: stale.ticket.path.clone(),
                source,
            })?;
            self.quarantine_file(&stale.ticket.path)?;
            self.remove_lease(&stale.ticket.id)?;
        }
        Ok(())
    }

    fn quarantine_file(&self, path: &Path) -> Result<(), AdmissionError> {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| AdmissionError::MalformedTicket(path.to_path_buf()))?;
        let destination = self.root.join(QUARANTINE_DIR).join(format!(
            "{name}.{}",
            QUARANTINE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::rename(path, &destination).map_err(|source| AdmissionError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    fn lease_path(&self, ticket_id: &str) -> PathBuf {
        self.root
            .join(LEASES_DIR)
            .join(format!("{LEASE_PREFIX}{ticket_id}{LEASE_SUFFIX}"))
    }

    fn read_lease(&self, ticket_id: &str) -> Result<Option<LeaseMarker>, AdmissionError> {
        let path = self.lease_path(ticket_id);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(AdmissionError::UnsafeLayout(path));
                }
                let lease = read_lease(&path)?;
                if lease.owner != "commit-ci-preflight"
                    || lease.purpose != "host-admission-lease"
                    || lease.schema_version != ADMISSION_SCHEMA_VERSION
                    || lease.owner_run_id != ticket_id
                {
                    return Err(AdmissionError::ForeignLease(path));
                }
                Ok(Some(lease))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(AdmissionError::Io { path, source }),
        }
    }

    fn activate_lease(&self, path: &Path, ticket_id: &str) -> Result<(), AdmissionError> {
        let mut lease = read_lease(path)?;
        if lease.owner_run_id != ticket_id || lease.state != "queued" {
            return Err(AdmissionError::ForeignLease(path.to_path_buf()));
        }
        let now = unix_seconds()?;
        lease.state = "active".to_owned();
        lease.acquired_at_unix_seconds = now;
        lease.heartbeat_at_unix_seconds = now;
        write_lease(path, &lease)
    }

    fn remove_lease(&self, ticket_id: &str) -> Result<(), AdmissionError> {
        let path = self.lease_path(ticket_id);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(AdmissionError::Io { path, source }),
        }
    }

    fn try_lock_slot(
        &self,
        _deadline: &AdmissionDeadline,
        _cancellation: &CancellationToken,
    ) -> Result<Option<File>, AdmissionError> {
        let path = self.root.join(SLOT_LOCK);
        let file = open_lock_file(&path, true)?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(file)),
            Err(source) if source.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(source) => Err(AdmissionError::Lock { path, source }),
        }
    }

    fn lock_status(&self, name: &str, kind: &str) -> Result<AdmissionLockStatusV1, AdmissionError> {
        let path = self.root.join(name);
        let state = match open_existing_lock_file(&path)? {
            None => "unknown".to_owned(),
            Some(file) => match file.try_lock_exclusive() {
                Ok(()) => {
                    FileExt::unlock(&file).map_err(|source| AdmissionError::Lock {
                        path: path.clone(),
                        source,
                    })?;
                    "free".to_owned()
                }
                Err(source) if source.kind() == io::ErrorKind::WouldBlock => "held".to_owned(),
                Err(source) => return Err(AdmissionError::Lock { path, source }),
            },
        };
        Ok(AdmissionLockStatusV1 {
            kind: kind.to_owned(),
            state,
            owner_run_id: None,
            acquired_at_unix_seconds: None,
            heartbeat_at_unix_seconds: None,
            lease_state: "not_applicable".to_owned(),
        })
    }

    fn slot_status(&self) -> Result<AdmissionLockStatusV1, AdmissionError> {
        let lock = self.lock_status(SLOT_LOCK, "slot_lock")?;
        let leases = self.active_leases()?;
        let candidate = if leases.len() == 1 {
            Some(&leases[0])
        } else {
            None
        };
        let lease_state = match candidate {
            Some(lease) if lease_is_expired(lease) => "expired",
            Some(_) => "active",
            None if leases.is_empty() => "absent",
            None => "unknown",
        };
        let contradictory = lock.state == "free" && !leases.is_empty();
        Ok(AdmissionLockStatusV1 {
            kind: "slot_lock".to_owned(),
            state: if contradictory {
                "unknown".to_owned()
            } else {
                lock.state
            },
            owner_run_id: candidate.map(|lease| lease.owner_run_id.clone()),
            acquired_at_unix_seconds: candidate.map(|lease| lease.acquired_at_unix_seconds),
            heartbeat_at_unix_seconds: candidate.map(|lease| lease.heartbeat_at_unix_seconds),
            lease_state: lease_state.to_owned(),
        })
    }

    fn active_leases(&self) -> Result<Vec<LeaseMarker>, AdmissionError> {
        let directory = self.root.join(LEASES_DIR);
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(AdmissionError::Io {
                    path: directory,
                    source,
                });
            }
        };
        let mut leases = Vec::new();
        for entry in entries {
            let entry = entry.map_err(AdmissionError::ReadDir)?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|source| AdmissionError::Io {
                path: path.clone(),
                source,
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(AdmissionError::UnsafeLayout(path));
            }
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| AdmissionError::MalformedLease(path.clone()))?;
            if !name.starts_with(LEASE_PREFIX) || !name.ends_with(LEASE_SUFFIX) {
                return Err(AdmissionError::MalformedLease(path));
            }
            let lease = read_lease(&path)?;
            if lease.owner != "commit-ci-preflight"
                || lease.purpose != "host-admission-lease"
                || lease.schema_version != ADMISSION_SCHEMA_VERSION
            {
                return Err(AdmissionError::ForeignLease(path));
            }
            if lease.state == "queued" {
                continue;
            }
            if lease.state != "active" {
                return Err(AdmissionError::ForeignLease(path));
            }
            leases.push(lease);
        }
        Ok(leases)
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
                Some(LEASES_DIR) => {
                    let metadata =
                        fs::symlink_metadata(&path).map_err(|source| AdmissionError::Io {
                            path: path.clone(),
                            source,
                        })?;
                    if metadata.file_type().is_symlink() || !metadata.is_dir() {
                        return Err(AdmissionError::UnsafeLayout(path));
                    }
                }
                Some(QUARANTINE_DIR) => {
                    let metadata =
                        fs::symlink_metadata(&path).map_err(|source| AdmissionError::Io {
                            path: path.clone(),
                            source,
                        })?;
                    if metadata.file_type().is_symlink() || !metadata.is_dir() {
                        return Err(AdmissionError::UnsafeLayout(path));
                    }
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
    heartbeat: Option<HeartbeatHandle>,
}

impl AdmissionGuard {
    pub fn ticket_id(&self) -> &str {
        &self.ticket_id
    }

    pub fn release(mut self) -> Result<(), AdmissionError> {
        self.release_inner()
    }

    fn release_inner(&mut self) -> Result<(), AdmissionError> {
        let cancellation = CancellationToken::default();
        let deadline = AdmissionDeadline::from_timeout(DEFAULT_STATUS_TIMEOUT)
            .expect("default status timeout is representable");
        let mut queue = self.coordinator.open_queue(false)?;
        lock_exclusive_until(
            &queue,
            &self.coordinator.root.join(QUEUE_LOCK),
            &deadline,
            &cancellation,
        )?;
        let mut cleanup_error = None;
        if let Some(mut heartbeat) = self.heartbeat.take() {
            heartbeat.stop();
        }
        if let Err(error) = self.coordinator.remove_lease(&self.ticket_id) {
            keep_first_error(&mut cleanup_error, error);
        }
        if let Some(slot) = self.slot.take() {
            if let Err(source) = FileExt::unlock(&slot) {
                keep_first_error(
                    &mut cleanup_error,
                    AdmissionError::Lock {
                        path: self.coordinator.root.join(SLOT_LOCK),
                        source,
                    },
                );
            }
        }
        if let Some(ticket) = self.ticket.take() {
            if let Err(source) = FileExt::unlock(&ticket) {
                keep_first_error(
                    &mut cleanup_error,
                    AdmissionError::Lock {
                        path: self.ticket_path.clone(),
                        source,
                    },
                );
            }
        }
        if let Err(source) = fs::remove_file(&self.ticket_path)
            && source.kind() != io::ErrorKind::NotFound
        {
            keep_first_error(
                &mut cleanup_error,
                AdmissionError::Io {
                    path: self.ticket_path.clone(),
                    source,
                },
            );
        }
        if let Err(error) = unlock(&mut queue) {
            keep_first_error(&mut cleanup_error, error);
        }
        cleanup_error.map_or(Ok(()), Err)
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
    lease_path: Option<PathBuf>,
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
        let cancellation = CancellationToken::default();
        let deadline = AdmissionDeadline::from_timeout(DEFAULT_STATUS_TIMEOUT)
            .expect("default status timeout is representable");
        lock_exclusive_until(
            &queue,
            &coordinator.root.join(QUEUE_LOCK),
            &deadline,
            &cancellation,
        )?;
        let mut cleanup_error = None;
        if let Err(source) = FileExt::unlock(file) {
            keep_first_error(
                &mut cleanup_error,
                AdmissionError::Lock {
                    path: path.clone(),
                    source,
                },
            );
        }
        if let Some(lease_path) = self.lease_path.take() {
            if let Err(source) = fs::remove_file(&lease_path)
                && source.kind() != io::ErrorKind::NotFound
            {
                keep_first_error(
                    &mut cleanup_error,
                    AdmissionError::Io {
                        path: lease_path,
                        source,
                    },
                );
            }
        }
        if let Err(source) = fs::remove_file(&path)
            && source.kind() != io::ErrorKind::NotFound
        {
            keep_first_error(
                &mut cleanup_error,
                AdmissionError::Io {
                    path: path.clone(),
                    source,
                },
            );
        }
        self.file = None;
        self.path = None;
        self.id = None;
        if let Err(error) = unlock(&mut queue) {
            keep_first_error(&mut cleanup_error, error);
        }
        cleanup_error.map_or(Ok(()), Err)
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
        if let Some(path) = &self.lease_path {
            let _ = fs::remove_file(path);
        }
    }
}

fn empty_status() -> AdmissionStatusV1 {
    AdmissionStatusV1 {
        schema_version: ADMISSION_STATUS_SCHEMA_VERSION.to_owned(),
        active: false,
        queue_count: 0,
        ticket_ids: Vec::new(),
        slot: AdmissionLockStatusV1 {
            kind: "slot_lock".to_owned(),
            state: "free".to_owned(),
            owner_run_id: None,
            acquired_at_unix_seconds: None,
            heartbeat_at_unix_seconds: None,
            lease_state: "absent".to_owned(),
        },
        queue_lock: AdmissionLockStatusV1 {
            kind: "queue_lock".to_owned(),
            state: "free".to_owned(),
            owner_run_id: None,
            acquired_at_unix_seconds: None,
            heartbeat_at_unix_seconds: None,
            lease_state: "not_applicable".to_owned(),
        },
        process_visibility_note: PROCESS_VISIBILITY_NOTE.to_owned(),
    }
}

fn sleep_until(deadline: Instant, cancellation: &CancellationToken) {
    if cancellation.is_cancelled() {
        return;
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    thread::sleep(remaining.min(WAIT_INTERVAL));
}

fn lock_exclusive_until(
    file: &File,
    path: &Path,
    deadline: &AdmissionDeadline,
    cancellation: &CancellationToken,
) -> Result<(), AdmissionError> {
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(()),
            Err(source) if source.kind() == io::ErrorKind::WouldBlock => {
                deadline.check(cancellation)?;
                thread::sleep(deadline.remaining().min(WAIT_INTERVAL));
            }
            Err(source) => {
                return Err(AdmissionError::Lock {
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
    }
}

fn unlock(file: &mut File) -> Result<(), AdmissionError> {
    FileExt::unlock(file).map_err(|source| AdmissionError::Lock {
        path: PathBuf::from("admission lock"),
        source,
    })
}

fn outcome_after_relocation_error(
    source: &Path,
    destination: &Path,
) -> AdmissionLayoutRecoveryOutcomeV1 {
    match (
        fs::symlink_metadata(source),
        fs::symlink_metadata(destination),
    ) {
        (Ok(source_meta), Err(error))
            if source_meta.is_dir()
                && !source_meta.file_type().is_symlink()
                && error.kind() == io::ErrorKind::NotFound =>
        {
            AdmissionLayoutRecoveryOutcomeV1::NotApplied
        }
        _ => AdmissionLayoutRecoveryOutcomeV1::RecoveryUncertain,
    }
}

fn durable_error(path: PathBuf, error: DurableFsError) -> AdmissionError {
    match error {
        DurableFsError::Io(source) => AdmissionError::Io { path, source },
        DurableFsError::UnsafePath(_) | DurableFsError::OwnershipMismatch => {
            AdmissionError::UnsafeLayout(path)
        }
        DurableFsError::AtomicReplacementUnavailable => AdmissionError::Io {
            path,
            source: io::Error::new(
                io::ErrorKind::Unsupported,
                "atomic admission record replacement is unavailable",
            ),
        },
    }
}

fn keep_first_error(target: &mut Option<AdmissionError>, error: AdmissionError) {
    if target.is_none() {
        *target = Some(error);
    }
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

fn read_lease(path: &Path) -> Result<LeaseMarker, AdmissionError> {
    let bytes = fs::read(path).map_err(|source| AdmissionError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|_| AdmissionError::MalformedLease(path.to_path_buf()))
}

fn create_lease(path: &Path, lease: &LeaseMarker) -> Result<(), AdmissionError> {
    let bytes = serde_json::to_vec(lease).map_err(AdmissionError::Json)?;
    let mut bytes = bytes;
    bytes.push(b'\n');
    DurableFileSystem::default()
        .create_new(path, &bytes)
        .map_err(|error| durable_error(path.to_path_buf(), error))
}

fn write_lease(path: &Path, lease: &LeaseMarker) -> Result<(), AdmissionError> {
    let bytes = serde_json::to_vec(lease).map_err(AdmissionError::Json)?;
    let mut bytes = bytes;
    bytes.push(b'\n');
    DurableFileSystem::default()
        .atomic_replace(path, &bytes)
        .map_err(|error| durable_error(path.to_path_buf(), error))
}

fn unix_seconds() -> Result<u64, AdmissionError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| AdmissionError::Clock)
}

fn lease_is_expired(lease: &LeaseMarker) -> bool {
    let Ok(now) = unix_seconds() else {
        return false;
    };
    now.saturating_sub(lease.heartbeat_at_unix_seconds) >= LEASE_DURATION.as_secs()
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
    ForeignLease(PathBuf),
    MalformedLease(PathBuf),
    RecoveryRequired(PathBuf),
    MalformedCounter(PathBuf),
    TicketCounterExhausted,
    Clock,
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
            | Self::ForeignLease(_)
            | Self::MalformedLease(_)
            | Self::RecoveryRequired(_)
            | Self::MalformedCounter(_)
            | Self::TicketCounterExhausted
            | Self::Clock
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
            Self::ForeignLease(path) => write!(
                formatter,
                "foreign admission lease rejected at {}",
                path.display()
            ),
            Self::MalformedLease(path) => write!(
                formatter,
                "malformed admission lease rejected at {}",
                path.display()
            ),
            Self::RecoveryRequired(path) => write!(
                formatter,
                "admission recovery is required for staged state at {}",
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
            Self::Clock => {
                formatter.write_str("system clock could not be read for admission lease")
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

    fn coordinator_with_empty_historical_agent_tickets(label: &str) -> AdmissionCoordinator {
        let coordinator = coordinator(label);
        coordinator.initialize().expect("canonical coordinator");
        File::create(coordinator.root().join(SLOT_LOCK)).expect("pre-existing slot lock");
        fs::write(coordinator.root().join(NEXT_TICKET), b"1\n")
            .expect("pre-existing ticket counter");
        fs::create_dir(coordinator.root().join("agent-tickets"))
            .expect("historical empty directory");
        coordinator
    }

    #[test]
    fn layout_recovery_missing_required_lock_is_operator_required_without_plan() {
        let coordinator = coordinator_with_empty_historical_agent_tickets("layout-missing-lock");
        fs::remove_file(coordinator.root().join(SLOT_LOCK)).expect("remove slot lock");
        let before = tree_fingerprint(coordinator.root());
        let report = coordinator.layout_recovery_status_with_timeout(
            Duration::from_secs(1),
            &CancellationToken::default(),
        );
        assert_eq!(
            report.classification,
            AdmissionLayoutRecoveryClassificationV1::OperatorRequired
        );
        assert_eq!(
            report.reason,
            AdmissionLayoutRecoveryReasonV1::UnsupportedLayout
        );
        assert!(report.plan_sha256.is_none());
        assert_eq!(before, tree_fingerprint(coordinator.root()));
    }

    #[test]
    fn layout_recovery_unknown_sibling_is_operator_required() {
        let coordinator = coordinator_with_empty_historical_agent_tickets("layout-unknown");
        fs::write(coordinator.root().join("unexpected"), b"x").expect("unknown sibling");
        let report = coordinator.layout_recovery_status_with_timeout(
            Duration::from_secs(1),
            &CancellationToken::default(),
        );
        assert_eq!(
            report.classification,
            AdmissionLayoutRecoveryClassificationV1::OperatorRequired
        );
        assert_eq!(
            report.reason,
            AdmissionLayoutRecoveryReasonV1::UnsupportedLayout
        );
        assert!(report.plan_sha256.is_none());
    }

    #[test]
    fn layout_recovery_malformed_canonical_without_target_is_not_canonical() {
        let coordinator = coordinator("layout-malformed-canonical");
        fs::create_dir_all(coordinator.root()).expect("root");
        fs::write(coordinator.root().join(OWNER_FILE), OWNER_BYTES).expect("owner");
        fs::write(coordinator.root().join(QUEUE_LOCK), b"locked").expect("queue lock");
        let report = coordinator.layout_recovery_status_with_timeout(
            Duration::from_secs(1),
            &CancellationToken::default(),
        );
        assert_eq!(
            report.classification,
            AdmissionLayoutRecoveryClassificationV1::OperatorRequired
        );
        assert_ne!(
            report.reason,
            AdmissionLayoutRecoveryReasonV1::CanonicalLayout
        );
        assert!(report.plan_sha256.is_none());
    }

    #[test]
    fn layout_recovery_target_absent_wrong_owner_is_operator_required() {
        let coordinator = coordinator_with_empty_historical_agent_tickets("layout-owner");
        fs::remove_dir(coordinator.root().join("agent-tickets")).expect("remove target");
        fs::write(coordinator.root().join(OWNER_FILE), b"{}\n").expect("wrong owner");
        let report = coordinator.layout_recovery_status_with_timeout(
            Duration::from_secs(1),
            &CancellationToken::default(),
        );
        assert_eq!(
            report.classification,
            AdmissionLayoutRecoveryClassificationV1::OperatorRequired
        );
        assert_eq!(report.reason, AdmissionLayoutRecoveryReasonV1::ForeignOwner);
        assert!(report.plan_sha256.is_none());
    }

    #[test]
    fn layout_recovery_target_absent_malformed_counter_is_operator_required() {
        let coordinator = coordinator_with_empty_historical_agent_tickets("layout-counter");
        fs::remove_dir(coordinator.root().join("agent-tickets")).expect("remove target");
        fs::write(coordinator.root().join(NEXT_TICKET), b"invalid\n").expect("bad counter");
        let report = coordinator.layout_recovery_status_with_timeout(
            Duration::from_secs(1),
            &CancellationToken::default(),
        );
        assert_eq!(
            report.classification,
            AdmissionLayoutRecoveryClassificationV1::OperatorRequired
        );
        assert!(report.plan_sha256.is_none());
    }

    #[test]
    fn layout_recovery_target_absent_held_queue_lock_is_operator_required() {
        let coordinator = coordinator_with_empty_historical_agent_tickets("layout-held-queue");
        fs::remove_dir(coordinator.root().join("agent-tickets")).expect("remove target");
        let queue = OpenOptions::new()
            .read(true)
            .write(true)
            .open(coordinator.root().join(QUEUE_LOCK))
            .expect("queue");
        queue.try_lock_exclusive().expect("hold queue");
        let report = coordinator.layout_recovery_status_with_timeout(
            Duration::from_millis(30),
            &CancellationToken::default(),
        );
        assert_eq!(report.reason, AdmissionLayoutRecoveryReasonV1::LockTimeout);
        assert!(report.plan_sha256.is_none());
    }

    #[test]
    fn layout_recovery_target_absent_held_slot_lock_is_operator_required() {
        let coordinator = coordinator_with_empty_historical_agent_tickets("layout-held-slot");
        fs::remove_dir(coordinator.root().join("agent-tickets")).expect("remove target");
        let slot = OpenOptions::new()
            .read(true)
            .write(true)
            .open(coordinator.root().join(SLOT_LOCK))
            .expect("slot");
        slot.try_lock_exclusive().expect("hold slot");
        let report = coordinator.layout_recovery_status_with_timeout(
            Duration::from_millis(30),
            &CancellationToken::default(),
        );
        assert_eq!(report.reason, AdmissionLayoutRecoveryReasonV1::LockTimeout);
        assert!(report.plan_sha256.is_none());
    }

    fn tree_fingerprint(root: &Path) -> Vec<(PathBuf, &'static str, Vec<u8>)> {
        fn walk(root: &Path, path: &Path, out: &mut Vec<(PathBuf, &'static str, Vec<u8>)>) {
            let mut entries = fs::read_dir(path)
                .expect("read fingerprint directory")
                .collect::<Result<Vec<_>, _>>()
                .expect("fingerprint entries");
            entries.sort_by_key(fs::DirEntry::file_name);
            for entry in entries {
                let path = entry.path();
                let relative = path
                    .strip_prefix(root)
                    .expect("relative path")
                    .to_path_buf();
                let metadata = fs::symlink_metadata(&path).expect("fingerprint metadata");
                if metadata.file_type().is_symlink() {
                    out.push((
                        relative,
                        "symlink",
                        fs::read_link(&path)
                            .expect("symlink target")
                            .to_string_lossy()
                            .as_bytes()
                            .to_vec(),
                    ));
                } else if metadata.is_dir() {
                    out.push((relative, "directory", Vec::new()));
                    walk(root, &path, out);
                } else {
                    out.push((relative, "file", fs::read(&path).expect("file bytes")));
                }
            }
        }
        let mut entries = Vec::new();
        walk(root, root, &mut entries);
        entries
    }

    #[test]
    fn apply_requires_exact_plan_and_preserves_empty_directory_in_quarantine() {
        let coordinator = coordinator_with_empty_historical_agent_tickets("layout-apply");
        let status = coordinator.layout_recovery_status_with_timeout(
            Duration::from_secs(1),
            &CancellationToken::default(),
        );
        let plan = status.plan_sha256.expect("plan");
        let before = tree_fingerprint(coordinator.root());
        let wrong = coordinator.apply_layout_recovery_with_timeout(
            &"0".repeat(64),
            Duration::from_secs(1),
            &CancellationToken::default(),
        );
        assert_eq!(wrong.outcome, AdmissionLayoutRecoveryOutcomeV1::NotApplied);
        assert_eq!(before, tree_fingerprint(coordinator.root()));
        let applied = coordinator.apply_layout_recovery_with_timeout(
            &plan,
            Duration::from_secs(1),
            &CancellationToken::default(),
        );
        assert_eq!(applied.outcome, AdmissionLayoutRecoveryOutcomeV1::Recovered);
        let entry = applied.quarantine_entry.expect("entry");
        assert_eq!(entry, format!("agent-tickets.recovered-v1-{plan}"));
        assert!(coordinator.root().join("quarantine").join(entry).is_dir());
        assert!(!coordinator.root().join("agent-tickets").exists());
        assert_eq!(
            coordinator
                .layout_recovery_status_with_timeout(
                    Duration::from_secs(1),
                    &CancellationToken::default()
                )
                .classification,
            AdmissionLayoutRecoveryClassificationV1::NotNeeded
        );
        let repeated = coordinator.apply_layout_recovery_with_timeout(
            &plan,
            Duration::from_secs(1),
            &CancellationToken::default(),
        );
        assert_eq!(
            repeated.outcome,
            AdmissionLayoutRecoveryOutcomeV1::NotApplied
        );
    }

    #[test]
    fn apply_rejects_changed_plan_without_mutation() {
        let coordinator = coordinator_with_empty_historical_agent_tickets("layout-changed-plan");
        let plan = coordinator
            .layout_recovery_status_with_timeout(
                Duration::from_secs(1),
                &CancellationToken::default(),
            )
            .plan_sha256
            .expect("plan");
        fs::write(coordinator.root().join("unexpected-change"), b"changed\n").expect("change");
        let before = tree_fingerprint(coordinator.root());
        let result = coordinator.apply_layout_recovery_with_timeout(
            &plan,
            Duration::from_secs(1),
            &CancellationToken::default(),
        );
        assert_eq!(result.outcome, AdmissionLayoutRecoveryOutcomeV1::NotApplied);
        assert_eq!(result.reason, AdmissionLayoutRecoveryReasonV1::PlanMismatch);
        assert_eq!(before, tree_fingerprint(coordinator.root()));
    }

    #[test]
    fn status_rejects_existing_real_quarantine_destination() {
        let coordinator = coordinator_with_empty_historical_agent_tickets("layout-collision");
        let plan = coordinator
            .layout_recovery_status_with_timeout(
                Duration::from_secs(1),
                &CancellationToken::default(),
            )
            .plan_sha256
            .expect("plan");
        fs::create_dir(
            coordinator
                .root()
                .join(QUARANTINE_DIR)
                .join(format!("agent-tickets.recovered-v1-{plan}")),
        )
        .expect("collision");
        let status = coordinator.layout_recovery_status_with_timeout(
            Duration::from_secs(1),
            &CancellationToken::default(),
        );
        assert_eq!(
            status.classification,
            AdmissionLayoutRecoveryClassificationV1::OperatorRequired
        );
        assert_eq!(
            status.reason,
            AdmissionLayoutRecoveryReasonV1::QuarantineCollision
        );
        assert!(status.plan_sha256.is_none());
    }

    #[test]
    fn apply_reports_uncertain_when_durability_fails_after_rename() {
        let base = coordinator_with_empty_historical_agent_tickets("layout-post-rename");
        let coordinator =
            AdmissionCoordinator::test_at_with_durable_fault(base.root().to_path_buf(), 2);
        let plan = coordinator
            .layout_recovery_status_with_timeout(
                Duration::from_secs(1),
                &CancellationToken::default(),
            )
            .plan_sha256
            .expect("plan");
        let result = coordinator.apply_layout_recovery_with_timeout(
            &plan,
            Duration::from_secs(1),
            &CancellationToken::default(),
        );
        assert_eq!(
            result.outcome,
            AdmissionLayoutRecoveryOutcomeV1::RecoveryUncertain
        );
        assert_eq!(
            result.quarantine_entry,
            Some(format!("agent-tickets.recovered-v1-{plan}"))
        );
        assert!(
            coordinator
                .root()
                .join(QUARANTINE_DIR)
                .join(result.quarantine_entry.expect("entry"))
                .is_dir()
        );
    }

    #[test]
    fn layout_recovery_normal_status_stays_closed_but_plans_empty_historical_directory() {
        let coordinator = coordinator_with_empty_historical_agent_tickets("layout-status");
        assert!(matches!(
            coordinator.status(),
            Err(AdmissionError::UnsafeLayout(_))
        ));
        let before = tree_fingerprint(coordinator.root());
        let report = coordinator.layout_recovery_status_with_timeout(
            Duration::from_secs(1),
            &CancellationToken::default(),
        );
        assert_eq!(
            report.classification,
            AdmissionLayoutRecoveryClassificationV1::RecoverableEmptyHistoricalAgentTickets
        );
        let digest = report.plan_sha256.expect("recovery plan");
        assert_eq!(digest.len(), 64);
        assert!(
            digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        assert_eq!(before, tree_fingerprint(coordinator.root()));
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

        fn acquired_id(&mut self) -> String {
            self.acquired()
                .strip_prefix("ACQUIRED ")
                .expect("admission marker")
                .to_owned()
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
            thread::sleep(Duration::from_secs(5));
        } else {
            thread::sleep(Duration::from_millis(50));
        }
        assert!(matches!(
            test_name,
            "two_processes_serialize_and_preserve_fifo_order"
                | "cross_activity_status_reports_slot_owner_and_lock_roles"
                | "status_is_bounded_and_excludes_sensitive_paths"
        ));
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
        let crashed_id = crashed.acquired_id();
        crashed.child.kill().expect("kill child");
        let _ = crashed.child.wait().expect("wait crashed child");
        let coordinator = AdmissionCoordinator::test_at(root.clone());
        let lease_path = coordinator.lease_path(&crashed_id);
        let mut lease = read_lease(&lease_path).expect("crashed lease");
        lease.heartbeat_at_unix_seconds = 0;
        write_lease(&lease_path, &lease).expect("expire crashed lease");
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
        let guard = coordinator
            .acquire(Duration::from_secs(1), &CancellationToken::default())
            .expect("quarantine unlocked malformed marker");
        drop(guard);
        assert!(!malformed.exists());
        assert_eq!(
            fs::read_dir(coordinator.root().join(QUARANTINE_DIR))
                .expect("quarantine directory")
                .count(),
            1
        );
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
    fn characterizes_partial_ticket_counter_blocking_admission_before_t6() {
        let coordinator = coordinator("partial-counter");
        coordinator.initialize().expect("initialize coordinator");
        fs::write(coordinator.root().join(NEXT_TICKET), b"").expect("partial counter fixture");

        let result = coordinator.acquire(Duration::from_secs(1), &CancellationToken::default());

        assert!(matches!(result, Err(AdmissionError::MalformedCounter(_))));
        assert!(!coordinator.status().expect("status").active);
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
        if child_mode("status_is_bounded_and_excludes_sensitive_paths") {
            return;
        }
        let coordinator = coordinator("status");
        let mut holder = child(
            coordinator.root(),
            "hold",
            "status_is_bounded_and_excludes_sensitive_paths",
        );
        let _holder_id = holder.acquired_id();
        let cancellation = CancellationToken::default();
        let waiter_coordinator = coordinator.clone();
        let waiter_cancellation = cancellation.clone();
        let waiter = thread::spawn(move || {
            waiter_coordinator.acquire(Duration::from_secs(2), &waiter_cancellation)
        });
        thread::sleep(Duration::from_millis(60));
        let status = coordinator.status().expect("status");
        assert_eq!(status.schema_version, ADMISSION_STATUS_SCHEMA_VERSION);
        assert!(status.active);
        assert_eq!(status.queue_count, 1);
        assert_eq!(status.ticket_ids.len(), 1);
        assert!(status.ticket_ids[0].starts_with("000"));
        assert_eq!(status.slot.kind, "slot_lock");
        assert_eq!(status.slot.state, "held");
        assert_eq!(status.slot.lease_state, "active");
        assert!(status.slot.owner_run_id.is_some());
        assert_eq!(status.queue_lock.kind, "queue_lock");
        assert_eq!(status.queue_lock.lease_state, "not_applicable");
        assert!(
            status
                .process_visibility_note
                .contains("does not prove global inactivity")
        );
        let json = serde_json::to_string(&status).expect("status JSON");
        assert!(!json.contains(coordinator.root().to_str().expect("UTF-8 root")));
        assert!(!json.contains("commit-ci-preflight"));
        assert!(status.queue_count <= MAX_QUEUE_TICKETS);
        cancellation.cancel();
        assert!(matches!(
            waiter.join().expect("waiter"),
            Err(AdmissionError::Cancelled)
        ));
        holder.finish();
        fs::remove_dir_all(coordinator.root()).expect("remove test coordinator");
    }

    #[test]
    fn status_timeout_and_cancellation_bound_queue_lock_waits() {
        let coordinator = coordinator("status-deadline");
        coordinator.initialize().expect("initialize coordinator");
        let queue_path = coordinator.root().join(QUEUE_LOCK);
        let queue = open_existing_lock_file(&queue_path)
            .expect("open queue")
            .expect("queue exists");
        queue.lock_exclusive().expect("hold queue");

        let started = Instant::now();
        let cancellation = CancellationToken::default();
        let result = coordinator.status_with_timeout(Duration::from_millis(80), &cancellation);
        assert!(matches!(result, Err(AdmissionError::Timeout)));
        assert!(started.elapsed() < Duration::from_secs(1));

        let cancellation = CancellationToken::default();
        let waiter_cancellation = cancellation.clone();
        let waiter_coordinator = coordinator.clone();
        let waiter = thread::spawn(move || {
            waiter_coordinator.status_with_timeout(Duration::from_secs(2), &waiter_cancellation)
        });
        thread::sleep(Duration::from_millis(30));
        cancellation.cancel();
        assert!(matches!(
            waiter.join().expect("status waiter"),
            Err(AdmissionError::Cancelled)
        ));
        FileExt::unlock(&queue).expect("unlock queue");
        fs::remove_dir_all(coordinator.root()).expect("remove test coordinator");
    }

    #[test]
    fn cross_activity_status_reports_slot_owner_and_lock_roles() {
        if child_mode("cross_activity_status_reports_slot_owner_and_lock_roles") {
            return;
        }
        let root = test_root("cross-activity");
        let _ = fs::remove_dir_all(&root);
        let observer_activity = AdmissionCoordinator::test_at(root.clone());
        let mut owner_activity = child(
            &root,
            "hold",
            "cross_activity_status_reports_slot_owner_and_lock_roles",
        );
        let owner_id = owner_activity.acquired_id();

        let status = observer_activity.status().expect("observer status");
        assert!(status.active);
        assert_eq!(status.slot.state, "held");
        assert_eq!(status.slot.owner_run_id.as_deref(), Some(owner_id.as_str()));
        assert!(status.slot.acquired_at_unix_seconds.is_some());
        assert!(status.slot.heartbeat_at_unix_seconds.is_some());
        assert_eq!(status.slot.lease_state, "active");
        assert_eq!(status.queue_lock.state, "free");
        assert!(
            status
                .process_visibility_note
                .contains("does not prove global inactivity")
        );

        owner_activity.finish();
        fs::remove_dir_all(root).expect("remove test coordinator");
    }

    #[test]
    fn unlocked_ticket_without_lease_is_quarantined() {
        let coordinator = coordinator("unknown-lease");
        coordinator.initialize().expect("initialize coordinator");
        let path = coordinator
            .root()
            .join(TICKETS_DIR)
            .join("ticket-00000000000000000001.json");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("legacy ticket");
        let marker = TicketMarker {
            owner: "commit-ci-preflight".to_owned(),
            purpose: "host-admission-ticket".to_owned(),
            schema_version: ADMISSION_SCHEMA_VERSION.to_owned(),
            ticket_id: "00000000000000000001".to_owned(),
        };
        file.write_all(&serde_json::to_vec(&marker).expect("marker JSON"))
            .expect("write marker");
        file.sync_all().expect("sync marker");
        FileExt::unlock(&file).expect("unlock legacy ticket");
        drop(file);
        fs::write(coordinator.root().join(NEXT_TICKET), b"2\n").expect("next ticket");

        let guard = coordinator
            .acquire(Duration::from_secs(2), &CancellationToken::default())
            .expect("unlocked ticket without lease is certainly abandoned");
        drop(guard);
        assert!(!path.exists());
        assert_eq!(
            fs::read_dir(coordinator.root().join(QUARANTINE_DIR))
                .expect("quarantine directory")
                .count(),
            1
        );
        fs::remove_dir_all(coordinator.root()).expect("remove test coordinator");
    }

    #[test]
    fn abandoned_ticket_staging_is_quarantined_before_acquisition() {
        let coordinator = coordinator("staged-ticket");
        coordinator.initialize().expect("initialize coordinator");
        let staging = coordinator
            .root()
            .join(TICKETS_DIR)
            .join(".ticket-staging-00000000000000000001-crashed.json");
        fs::write(&staging, b"partial\n").expect("staging fixture");
        assert!(matches!(
            coordinator.status(),
            Err(AdmissionError::RecoveryRequired(_))
        ));

        let guard = coordinator
            .acquire(Duration::from_secs(2), &CancellationToken::default())
            .expect("quarantine abandoned staging");
        drop(guard);
        assert!(!staging.exists());
        assert_eq!(
            fs::read_dir(coordinator.root().join(QUARANTINE_DIR))
                .expect("quarantine directory")
                .count(),
            1
        );
        fs::remove_dir_all(coordinator.root()).expect("remove test coordinator");
    }

    #[test]
    fn locked_malformed_ticket_remains_fail_closed() {
        let coordinator = coordinator("locked-malformed");
        coordinator.initialize().expect("initialize coordinator");
        let path = coordinator
            .root()
            .join(TICKETS_DIR)
            .join("ticket-00000000000000000001.json");
        fs::write(&path, b"partial\n").expect("malformed ticket");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .expect("open malformed ticket");
        file.lock_exclusive().expect("lock malformed ticket");

        assert!(matches!(
            coordinator.acquire(Duration::from_secs(1), &CancellationToken::default()),
            Err(AdmissionError::MalformedTicket(_))
        ));
        assert!(path.exists());
        FileExt::unlock(&file).expect("unlock malformed ticket");
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
        assert_eq!(value.as_object().expect("object").len(), 7);
        assert!(value.get("ticket_ids").expect("ticket ids").is_array());
        assert!(value.get("slot").expect("slot").is_object());
        assert!(value.get("queue_lock").expect("queue lock").is_object());
        assert!(
            value
                .get("process_visibility_note")
                .expect("visibility note")
                .is_string()
        );
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
