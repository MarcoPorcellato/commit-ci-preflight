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

use std::ffi::CString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::durable_fs::{DurableFileSystem, DurableFsError};
use crate::process::CancellationToken;

pub const ADMISSION_SCHEMA_VERSION: &str = "1.0";
pub const ADMISSION_STATUS_SCHEMA_VERSION: &str = "2.0";
pub const DEFAULT_QUEUE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
pub const DEFAULT_STATUS_TIMEOUT: Duration = Duration::from_secs(5);
pub const MAX_QUEUE_TICKETS: usize = 1024;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum AdmissionReconciliationModeV1 {
    Preview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdmissionReconciliationCandidateV1 {
    pub ticket_id: String,
    pub classification: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdmissionReconciliationReportV1 {
    pub schema_version: String,
    pub mode: AdmissionReconciliationModeV1,
    pub candidates: Vec<AdmissionReconciliationCandidateV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdmissionReconciliationOutcomeV1 {
    pub ticket_id: String,
    pub classification: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdmissionReconciliationApplyReportV1 {
    pub schema_version: String,
    pub outcomes: Vec<AdmissionReconciliationOutcomeV1>,
}

#[derive(Debug)]
pub enum AdmissionReconciliationError {
    Admission(AdmissionError),
    Blocked(&'static str),
    Partial {
        reason: &'static str,
        report: AdmissionReconciliationApplyReportV1,
    },
}

impl From<AdmissionError> for AdmissionReconciliationError {
    fn from(error: AdmissionError) -> Self {
        Self::Admission(error)
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReconciliationSyncPoint {
    LeaseDirectory,
    TicketsDirectory,
    QuarantineDirectory,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReconciliationTestGatePoint {
    AfterExclusion,
    AfterSelectedDescriptors,
}

#[cfg(test)]
#[derive(Debug)]
struct ReconciliationTestGate {
    reached: std::sync::Barrier,
    release: std::sync::Barrier,
}

#[cfg(test)]
impl ReconciliationTestGate {
    fn new() -> Self {
        Self {
            reached: std::sync::Barrier::new(2),
            release: std::sync::Barrier::new(2),
        }
    }

    fn wait_until_reached(&self) {
        self.reached.wait();
    }

    fn release(&self) {
        self.release.wait();
    }

    fn stop(&self) {
        self.reached.wait();
        self.release.wait();
    }
}

#[cfg(test)]
#[derive(Debug, Default)]
struct ReconciliationTestControls {
    after_exclusion: std::sync::Mutex<Option<std::sync::Arc<ReconciliationTestGate>>>,
    after_selected_descriptors: std::sync::Mutex<Option<std::sync::Arc<ReconciliationTestGate>>>,
    after_lease_removal: std::sync::Mutex<Option<(usize, std::sync::Arc<ReconciliationTestGate>)>>,
    fail_move_on_attempt: std::sync::atomic::AtomicUsize,
    move_attempts: std::sync::atomic::AtomicUsize,
    fail_lease_remove_on_attempt: std::sync::atomic::AtomicUsize,
    lease_remove_attempts: std::sync::atomic::AtomicUsize,
    fail_sync_on: std::sync::Mutex<Option<(ReconciliationSyncPoint, usize)>>,
    quarantine_suffix: std::sync::Mutex<Option<String>>,
    cleanup_runs: std::sync::atomic::AtomicUsize,
}

struct ReconciliationApplyLocks {
    queue: Option<File>,
    slot: Option<File>,
    tickets: Vec<(File, PathBuf, u64, u64)>,
    #[cfg(test)]
    test_controls: std::sync::Arc<ReconciliationTestControls>,
}

impl ReconciliationApplyLocks {
    #[cfg(not(test))]
    fn new(queue: File, slot: File) -> Self {
        Self {
            queue: Some(queue),
            slot: Some(slot),
            tickets: Vec::new(),
        }
    }

    #[cfg(test)]
    fn new(
        queue: File,
        slot: File,
        test_controls: std::sync::Arc<ReconciliationTestControls>,
    ) -> Self {
        Self {
            queue: Some(queue),
            slot: Some(slot),
            tickets: Vec::new(),
            test_controls,
        }
    }

    fn release_all(&mut self) -> Option<&'static str> {
        let mut first_error = None;
        for (file, _, ..) in self.tickets.drain(..) {
            if FileExt::unlock(&file).is_err() && first_error.is_none() {
                first_error = Some("ticket_unlock_failed");
            }
        }
        if let Some(slot) = self.slot.take()
            && FileExt::unlock(&slot).is_err()
            && first_error.is_none()
        {
            first_error = Some("slot_unlock_failed");
        }
        if let Some(mut queue) = self.queue.take()
            && unlock(&mut queue).is_err()
            && first_error.is_none()
        {
            first_error = Some("queue_unlock_failed");
        }
        first_error
    }
}

impl Drop for ReconciliationApplyLocks {
    fn drop(&mut self) {
        let _ = self.release_all();
        #[cfg(test)]
        self.test_controls
            .cleanup_runs
            .fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone)]
pub struct AdmissionCoordinator {
    root: PathBuf,
    #[cfg(test)]
    reconciliation_test_controls: std::sync::Arc<ReconciliationTestControls>,
}

impl AdmissionCoordinator {
    #[cfg(test)]
    fn test_at(root: PathBuf) -> Self {
        Self {
            root,
            reconciliation_test_controls: std::sync::Arc::new(ReconciliationTestControls::default()),
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
            reconciliation_test_controls: std::sync::Arc::new(ReconciliationTestControls::default()),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    #[cfg(test)]
    fn set_reconciliation_gate(
        &self,
        point: ReconciliationTestGatePoint,
        gate: std::sync::Arc<ReconciliationTestGate>,
    ) {
        let target = match point {
            ReconciliationTestGatePoint::AfterExclusion => {
                &self.reconciliation_test_controls.after_exclusion
            }
            ReconciliationTestGatePoint::AfterSelectedDescriptors => {
                &self.reconciliation_test_controls.after_selected_descriptors
            }
        };
        *target.lock().expect("reconciliation gate lock") = Some(gate);
    }

    #[cfg(test)]
    fn set_reconciliation_after_lease_removal_gate(
        &self,
        ticket_ordinal: usize,
        gate: std::sync::Arc<ReconciliationTestGate>,
    ) {
        *self
            .reconciliation_test_controls
            .after_lease_removal
            .lock()
            .expect("reconciliation lease-removal gate lock") = Some((ticket_ordinal, gate));
    }

    #[cfg(test)]
    fn reach_reconciliation_gate(&self, point: ReconciliationTestGatePoint) {
        let target = match point {
            ReconciliationTestGatePoint::AfterExclusion => {
                &self.reconciliation_test_controls.after_exclusion
            }
            ReconciliationTestGatePoint::AfterSelectedDescriptors => {
                &self.reconciliation_test_controls.after_selected_descriptors
            }
        };
        let gate = target.lock().expect("reconciliation gate lock").take();
        if let Some(gate) = gate {
            gate.stop();
        }
    }

    #[cfg(test)]
    fn reach_reconciliation_after_lease_removal_gate(&self, ticket_ordinal: usize) {
        let gate = {
            let mut target = self
                .reconciliation_test_controls
                .after_lease_removal
                .lock()
                .expect("reconciliation lease-removal gate lock");
            if target.as_ref().map(|(ordinal, _)| *ordinal) == Some(ticket_ordinal) {
                target.take().map(|(_, gate)| gate)
            } else {
                None
            }
        };
        if let Some(gate) = gate {
            gate.stop();
        }
    }

    #[cfg(test)]
    fn fail_reconciliation_move_on_attempt(&self, attempt: usize) {
        self.reconciliation_test_controls
            .move_attempts
            .store(0, Ordering::SeqCst);
        self.reconciliation_test_controls
            .fail_move_on_attempt
            .store(attempt, Ordering::SeqCst);
    }

    #[cfg(test)]
    fn fail_reconciliation_lease_remove_on_attempt(&self, attempt: usize) {
        self.reconciliation_test_controls
            .lease_remove_attempts
            .store(0, Ordering::SeqCst);
        self.reconciliation_test_controls
            .fail_lease_remove_on_attempt
            .store(attempt, Ordering::SeqCst);
    }

    #[cfg(test)]
    fn fail_reconciliation_sync_on(&self, point: ReconciliationSyncPoint, ticket_ordinal: usize) {
        *self
            .reconciliation_test_controls
            .fail_sync_on
            .lock()
            .expect("reconciliation sync fault lock") = Some((point, ticket_ordinal));
    }

    #[cfg(test)]
    fn set_reconciliation_quarantine_suffix(&self, suffix: &str) {
        *self
            .reconciliation_test_controls
            .quarantine_suffix
            .lock()
            .expect("reconciliation suffix lock") = Some(suffix.to_owned());
    }

    #[cfg(test)]
    fn reconciliation_cleanup_runs(&self) -> usize {
        self.reconciliation_test_controls
            .cleanup_runs
            .load(Ordering::SeqCst)
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
        if !self.valid_owner_marker_exists()? {
            return Err(AdmissionError::ForeignOwner(self.root.join(OWNER_FILE)));
        }
        let queue_lock = self.lock_status(QUEUE_LOCK, "queue_lock")?;
        let mut queue = self.lock_queue_until(&deadline, cancellation)?;
        if !self.valid_owner_marker_exists()? {
            return Err(AdmissionError::ForeignOwner(self.root.join(OWNER_FILE)));
        }
        self.validate_layout(true)?;
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

    pub fn reconcile_preview_with_timeout(
        &self,
        timeout: Duration,
        cancellation: &CancellationToken,
    ) -> Result<AdmissionReconciliationReportV1, AdmissionReconciliationError> {
        let deadline = AdmissionDeadline::from_timeout(timeout)?;
        deadline.check(cancellation)?;
        if !self.root_exists()? {
            return Ok(AdmissionReconciliationReportV1 {
                schema_version: "1.0".to_owned(),
                mode: AdmissionReconciliationModeV1::Preview,
                candidates: Vec::new(),
            });
        }
        if !self.valid_owner_marker_exists()? {
            return Err(AdmissionReconciliationError::Admission(
                AdmissionError::ForeignOwner(self.root.join(OWNER_FILE)),
            ));
        }
        self.validate_layout(true)?;
        let mut queue = self.lock_queue_until(&deadline, cancellation)?;
        let mut slot_blocked = false;
        let slot = match open_existing_lock_file(&self.root.join(SLOT_LOCK))? {
            Some(file) => match file.try_lock_exclusive() {
                Ok(()) => Some(file),
                Err(source) if source.kind() == io::ErrorKind::WouldBlock => {
                    slot_blocked = true;
                    None
                }
                Err(source) => {
                    return Err(AdmissionReconciliationError::Admission(
                        AdmissionError::Lock {
                            path: self.root.join(SLOT_LOCK),
                            source,
                        },
                    ));
                }
            },
            None => return Err(AdmissionReconciliationError::Blocked("missing_slot_lock")),
        };
        let mut candidates = Vec::new();
        let directory =
            fs::read_dir(self.root.join(TICKETS_DIR)).map_err(|source| AdmissionError::Io {
                path: self.root.join(TICKETS_DIR),
                source,
            })?;
        let mut count = 0;
        for entry in directory {
            count += 1;
            if count > MAX_QUEUE_TICKETS {
                return Err(AdmissionReconciliationError::Admission(
                    AdmissionError::QueueFull,
                ));
            }
            deadline.check(cancellation)?;
            let entry = entry.map_err(AdmissionError::ReadDir)?;
            let path = entry.path();
            validate_regular(&path)?;
            let id = parse_ticket_name(&path)?;
            let marker = read_ticket(&path)?;
            if marker.owner != "commit-ci-preflight"
                || marker.purpose != "host-admission-ticket"
                || marker.schema_version != ADMISSION_SCHEMA_VERSION
                || marker.ticket_id != id
            {
                return Err(AdmissionReconciliationError::Admission(
                    AdmissionError::ForeignTicket(path),
                ));
            }
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .map_err(|source| AdmissionError::Io {
                    path: path.clone(),
                    source,
                })?;
            let classification = match file.try_lock_exclusive() {
                Err(source) if source.kind() == io::ErrorKind::WouldBlock => {
                    "blocked_ticket_locked"
                }
                Err(source) => {
                    return Err(AdmissionReconciliationError::Admission(
                        AdmissionError::Lock {
                            path: path.clone(),
                            source,
                        },
                    ));
                }
                Ok(()) => {
                    let lease = self.read_lease(&id)?;
                    let result = if slot_blocked {
                        "blocked_slot_locked"
                    } else if lease.is_none() {
                        "eligible_absent_lease"
                    } else {
                        match lease.as_ref() {
                            Some(lease) if !lease_is_semantically_valid(lease) => {
                                "blocked_invalid_lease"
                            }
                            Some(lease) if lease_is_expired(lease) => "eligible_expired_lease",
                            Some(_) => "blocked_live_lease",
                            None => unreachable!(),
                        }
                    };
                    FileExt::unlock(&file).map_err(|source| AdmissionError::Lock {
                        path: path.clone(),
                        source,
                    })?;
                    result
                }
            };
            candidates.push(AdmissionReconciliationCandidateV1 {
                ticket_id: id,
                classification: classification.to_owned(),
            });
        }
        if let Some(slot) = slot {
            FileExt::unlock(&slot).map_err(|source| AdmissionError::Lock {
                path: self.root.join(SLOT_LOCK),
                source,
            })?;
        }
        unlock(&mut queue)?;
        candidates.sort_by(|left, right| left.ticket_id.cmp(&right.ticket_id));
        Ok(AdmissionReconciliationReportV1 {
            schema_version: "1.0".to_owned(),
            mode: AdmissionReconciliationModeV1::Preview,
            candidates,
        })
    }

    pub fn reconcile_apply_with_timeout(
        &self,
        selected_ids: &[String],
        timeout: Duration,
        cancellation: &CancellationToken,
    ) -> Result<AdmissionReconciliationApplyReportV1, AdmissionReconciliationError> {
        let deadline = AdmissionDeadline::from_timeout(timeout)?;
        deadline.check(cancellation)?;
        if selected_ids.is_empty() {
            return Err(AdmissionReconciliationError::Blocked("empty_target"));
        }
        if selected_ids.len() > MAX_QUEUE_TICKETS {
            return Err(AdmissionReconciliationError::Blocked("too_many_targets"));
        }
        let mut canonical = selected_ids.to_vec();
        canonical.sort();
        canonical.dedup();
        if canonical.len() != selected_ids.len() {
            return Err(AdmissionReconciliationError::Blocked("duplicate_target"));
        }
        if canonical
            .iter()
            .any(|id| id.len() != 20 || !id.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err(AdmissionReconciliationError::Blocked("malformed_target"));
        }
        if !self.root_exists()? {
            return Err(AdmissionReconciliationError::Blocked("unknown_target"));
        }
        if !self.valid_owner_marker_exists()? {
            return Err(AdmissionReconciliationError::Admission(
                AdmissionError::ForeignOwner(self.root.join(OWNER_FILE)),
            ));
        }
        self.validate_reconciliation_apply_layout()?;
        let mut queue = self.lock_queue_until(&deadline, cancellation)?;
        let slot_path = self.root.join(SLOT_LOCK);
        let slot = match open_existing_lock_file(&slot_path)? {
            Some(file) => match file.try_lock_exclusive() {
                Ok(()) => file,
                Err(source) if source.kind() == io::ErrorKind::WouldBlock => {
                    unlock(&mut queue)?;
                    return Err(AdmissionReconciliationError::Blocked("slot_busy"));
                }
                Err(source) => {
                    unlock(&mut queue)?;
                    return Err(AdmissionReconciliationError::Admission(
                        AdmissionError::Lock {
                            path: slot_path,
                            source,
                        },
                    ));
                }
            },
            None => {
                unlock(&mut queue)?;
                return Err(AdmissionReconciliationError::Blocked("missing_slot_lock"));
            }
        };
        #[cfg(test)]
        let mut locks =
            ReconciliationApplyLocks::new(queue, slot, self.reconciliation_test_controls.clone());
        #[cfg(not(test))]
        let mut locks = ReconciliationApplyLocks::new(queue, slot);
        if !self.valid_owner_marker_exists()? {
            return Err(AdmissionReconciliationError::Blocked("invalid_layout"));
        }
        self.validate_reconciliation_apply_layout()?;
        #[cfg(test)]
        self.reach_reconciliation_gate(ReconciliationTestGatePoint::AfterExclusion);
        self.validate_reconciliation_lease_namespace(&deadline, cancellation)?;
        macro_rules! reject {
            ($reason:expr) => {{
                return Err(AdmissionReconciliationError::Blocked($reason));
            }};
        }
        for id in &canonical {
            deadline.check(cancellation)?;
            let path = self
                .root
                .join(TICKETS_DIR)
                .join(format!("{TICKET_PREFIX}{id}{TICKET_SUFFIX}"));
            let mut file = open_selected_ticket(&path)?;
            match file.try_lock_exclusive() {
                Ok(()) => {}
                Err(source) if source.kind() == io::ErrorKind::WouldBlock => {
                    reject!("held_ticket");
                }
                Err(source) => {
                    return Err(AdmissionReconciliationError::Admission(
                        AdmissionError::Lock { path, source },
                    ));
                }
            }
            let marker = read_ticket_from_descriptor(&mut file, &path)?;
            if marker.owner != "commit-ci-preflight"
                || marker.purpose != "host-admission-ticket"
                || marker.schema_version != ADMISSION_SCHEMA_VERSION
                || marker.ticket_id != *id
            {
                reject!("foreign_or_malformed_ticket");
            }
            if let Some(lease) = self
                .read_lease(id)
                .map_err(AdmissionReconciliationError::Admission)?
            {
                if !lease_is_semantically_valid(&lease) {
                    reject!("invalid_lease");
                }
                if !lease_is_expired(&lease) {
                    reject!("live_or_future_lease");
                }
            }
            let descriptor_metadata = file.metadata().map_err(|source| {
                AdmissionReconciliationError::Admission(AdmissionError::Io {
                    path: path.clone(),
                    source,
                })
            })?;
            #[cfg(unix)]
            let (dev, ino) = (descriptor_metadata.dev(), descriptor_metadata.ino());
            #[cfg(not(unix))]
            let (dev, ino) = (descriptor_metadata.len(), 0);
            locks.tickets.push((file, path, dev, ino));
        }

        #[cfg(test)]
        self.reach_reconciliation_gate(ReconciliationTestGatePoint::AfterSelectedDescriptors);

        for (_file, path, dev, ino) in &locks.tickets {
            let current = match fs::symlink_metadata(path) {
                Ok(current) => current,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    reject!("selected_ticket_changed");
                }
                Err(source) => {
                    return Err(AdmissionReconciliationError::Admission(
                        AdmissionError::Io {
                            path: path.clone(),
                            source,
                        },
                    ));
                }
            };
            #[cfg(unix)]
            let same_object = current.is_file() && current.dev() == *dev && current.ino() == *ino;
            #[cfg(not(unix))]
            let same_object = current.is_file() && current.len() == *dev;
            if !same_object {
                reject!("selected_ticket_changed");
            }
        }

        let mut outcomes = Vec::new();
        macro_rules! partial {
            ($index:expr, $reason:expr, $classification:expr) => {{
                debug_assert_eq!(outcomes.len(), $index);
                outcomes.push(AdmissionReconciliationOutcomeV1 {
                    ticket_id: canonical[$index].clone(),
                    classification: $classification.to_owned(),
                });
                outcomes.extend(canonical.iter().skip($index + 1).map(|id| {
                    AdmissionReconciliationOutcomeV1 {
                        ticket_id: id.clone(),
                        classification: "not_attempted".to_owned(),
                    }
                }));
                return Err(AdmissionReconciliationError::Partial {
                    reason: $reason,
                    report: AdmissionReconciliationApplyReportV1 {
                        schema_version: "1.0".to_owned(),
                        outcomes,
                    },
                });
            }};
        }

        for (index, (id, (_file, path, ..))) in
            canonical.iter().zip(locks.tickets.iter()).enumerate()
        {
            if let Err(error) = deadline.check(cancellation) {
                if outcomes.is_empty() {
                    return Err(AdmissionReconciliationError::Admission(error));
                }
                let reason = match error {
                    AdmissionError::Cancelled => "cancelled",
                    AdmissionError::Timeout => "timeout",
                    _ => "deadline_failed",
                };
                partial!(index, reason, "not_attempted");
            }

            let lease_path = self.lease_path(id);
            let lease_removed = match self.remove_reconciliation_lease(&lease_path) {
                Ok(()) => true,
                Err(error) if error.kind() == io::ErrorKind::NotFound => false,
                Err(source) => {
                    if outcomes.is_empty() {
                        return Err(AdmissionReconciliationError::Admission(
                            AdmissionError::Io {
                                path: lease_path,
                                source,
                            },
                        ));
                    }
                    partial!(index, "lease_remove_failed", "partial_lease_remove_failed");
                }
            };
            #[cfg(test)]
            if lease_removed {
                self.reach_reconciliation_after_lease_removal_gate(index + 1);
            }
            if lease_removed
                && self
                    .sync_reconciliation_directory(
                        &self.root.join(LEASES_DIR),
                        ReconciliationSyncPoint::LeaseDirectory,
                        index + 1,
                    )
                    .is_err()
            {
                partial!(index, "lease_sync_failed", "partial_lease_sync_failed");
            }
            if let Err(error) = deadline.check(cancellation) {
                if lease_removed || !outcomes.is_empty() {
                    let reason = match error {
                        AdmissionError::Cancelled => "cancelled",
                        AdmissionError::Timeout => "timeout",
                        _ => "deadline_failed",
                    };
                    partial!(index, reason, "partial_after_lease_removal");
                }
                return Err(AdmissionReconciliationError::Admission(error));
            }

            match self.quarantine_file_no_replace(path, index + 1) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if !lease_removed && outcomes.is_empty() {
                        return Err(AdmissionReconciliationError::Blocked(
                            "quarantine_collision",
                        ));
                    }
                    partial!(
                        index,
                        "quarantine_collision",
                        "partial_quarantine_collision"
                    );
                }
                Err(source) => {
                    if !lease_removed && outcomes.is_empty() {
                        return Err(AdmissionReconciliationError::Admission(
                            AdmissionError::Io {
                                path: path.clone(),
                                source,
                            },
                        ));
                    }
                    partial!(index, "move_failed", "partial_move_failed");
                }
            }
            if let Err(error) = deadline.check(cancellation) {
                let reason = match error {
                    AdmissionError::Cancelled => "cancelled",
                    AdmissionError::Timeout => "timeout",
                    _ => "deadline_failed",
                };
                partial!(index, reason, "partial_ticket_moved");
            }
            if self
                .sync_reconciliation_directory(
                    &self.root.join(TICKETS_DIR),
                    ReconciliationSyncPoint::TicketsDirectory,
                    index + 1,
                )
                .is_err()
            {
                partial!(index, "ticket_sync_failed", "partial_ticket_sync_failed");
            }
            if self
                .sync_reconciliation_directory(
                    &self.root.join(QUARANTINE_DIR),
                    ReconciliationSyncPoint::QuarantineDirectory,
                    index + 1,
                )
                .is_err()
            {
                partial!(
                    index,
                    "quarantine_sync_failed",
                    "partial_quarantine_sync_failed"
                );
            }
            outcomes.push(AdmissionReconciliationOutcomeV1 {
                ticket_id: id.clone(),
                classification: "quarantined".to_owned(),
            });
        }
        if let Some(reason) = locks.release_all() {
            return Err(AdmissionReconciliationError::Partial {
                reason,
                report: AdmissionReconciliationApplyReportV1 {
                    schema_version: "1.0".to_owned(),
                    outcomes,
                },
            });
        }
        Ok(AdmissionReconciliationApplyReportV1 {
            schema_version: "1.0".to_owned(),
            outcomes,
        })
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
        let initialized = self.valid_owner_marker_exists()?;
        if !initialized {
            self.validate_layout(false)?;
        }
        let mut queue = self.open_queue(!initialized)?;
        lock_exclusive_until(&queue, &self.root.join(QUEUE_LOCK), deadline, cancellation)?;
        if initialized {
            if !self.valid_owner_marker_exists()? {
                return Err(AdmissionError::ForeignOwner(self.root.join(OWNER_FILE)));
            }
            self.validate_layout(true)?;
        } else {
            self.validate_layout(false)?;
        }
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

    fn valid_owner_marker_exists(&self) -> Result<bool, AdmissionError> {
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
                Ok(true)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(AdmissionError::Io { path, source }),
        }
    }

    fn ensure_owner_marker(&self) -> Result<(), AdmissionError> {
        let path = self.root.join(OWNER_FILE);
        match self.valid_owner_marker_exists()? {
            true => Ok(()),
            false => {
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

    /// Reconciliation must never use `rename`, which replaces an existing
    /// destination on Unix.  Use the kernel no-replace primitive; unsupported
    /// hosts fail closed rather than falling back to an overwrite-capable move.
    fn quarantine_file_no_replace(&self, path: &Path, ticket_ordinal: usize) -> io::Result<()> {
        #[cfg(test)]
        {
            let attempt = self
                .reconciliation_test_controls
                .move_attempts
                .fetch_add(1, Ordering::SeqCst)
                + 1;
            if self
                .reconciliation_test_controls
                .fail_move_on_attempt
                .compare_exchange(attempt, 0, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return Err(io::Error::other("injected move failure"));
            }
        }
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "ticket has no name"))?;
        #[cfg(test)]
        let suffix = self
            .reconciliation_test_controls
            .quarantine_suffix
            .lock()
            .expect("reconciliation suffix lock")
            .clone()
            .unwrap_or_else(|| {
                QUARANTINE_SEQUENCE
                    .fetch_add(1, Ordering::Relaxed)
                    .to_string()
            });
        #[cfg(not(test))]
        let suffix = QUARANTINE_SEQUENCE
            .fetch_add(1, Ordering::Relaxed)
            .to_string();
        let destination =
            self.root
                .join(QUARANTINE_DIR)
                .join(format!("{}.{}", name.to_string_lossy(), suffix));
        let _ = ticket_ordinal;
        atomic_rename_no_replace(path, &destination)
    }

    fn remove_reconciliation_lease(&self, path: &Path) -> io::Result<()> {
        #[cfg(test)]
        {
            let attempt = self
                .reconciliation_test_controls
                .lease_remove_attempts
                .fetch_add(1, Ordering::SeqCst)
                + 1;
            if self
                .reconciliation_test_controls
                .fail_lease_remove_on_attempt
                .compare_exchange(attempt, 0, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return Err(io::Error::other("injected lease removal failure"));
            }
        }
        fs::remove_file(path)
    }

    fn sync_reconciliation_directory(
        &self,
        path: &Path,
        point: ReconciliationSyncPoint,
        ticket_ordinal: usize,
    ) -> io::Result<()> {
        #[cfg(test)]
        {
            let mut failure = self
                .reconciliation_test_controls
                .fail_sync_on
                .lock()
                .expect("reconciliation sync fault lock");
            if failure.as_ref() == Some(&(point, ticket_ordinal)) {
                failure.take();
                return Err(io::Error::other("injected directory sync failure"));
            }
        }
        let _ = (point, ticket_ordinal);
        File::open(path)?.sync_all()
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

    fn validate_reconciliation_apply_layout(&self) -> Result<(), AdmissionReconciliationError> {
        match self.validate_layout(true) {
            Ok(()) => {}
            Err(AdmissionError::UnsafeLayout(_) | AdmissionError::ForeignOwner(_)) => {
                return Err(AdmissionReconciliationError::Blocked("invalid_layout"));
            }
            Err(error) => return Err(AdmissionReconciliationError::Admission(error)),
        }
        for name in [OWNER_FILE, QUEUE_LOCK, SLOT_LOCK] {
            let path = self.root.join(name);
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Err(AdmissionReconciliationError::Blocked("invalid_layout"));
                }
                Err(source) => {
                    return Err(AdmissionReconciliationError::Admission(
                        AdmissionError::Io { path, source },
                    ));
                }
            };
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(AdmissionReconciliationError::Blocked("invalid_layout"));
            }
        }
        for name in [TICKETS_DIR, LEASES_DIR, QUARANTINE_DIR] {
            let path = self.root.join(name);
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Err(AdmissionReconciliationError::Blocked("invalid_layout"));
                }
                Err(source) => {
                    return Err(AdmissionReconciliationError::Admission(
                        AdmissionError::Io { path, source },
                    ));
                }
            };
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(AdmissionReconciliationError::Blocked("invalid_layout"));
            }
        }
        Ok(())
    }

    fn validate_reconciliation_lease_namespace(
        &self,
        deadline: &AdmissionDeadline,
        cancellation: &CancellationToken,
    ) -> Result<(), AdmissionReconciliationError> {
        let directory = self.root.join(LEASES_DIR);
        let entries = fs::read_dir(&directory).map_err(|source| {
            AdmissionReconciliationError::Admission(AdmissionError::Io {
                path: directory,
                source,
            })
        })?;
        let mut count = 0;
        for entry in entries {
            deadline.check(cancellation)?;
            count += 1;
            if count > MAX_QUEUE_TICKETS {
                return Err(AdmissionReconciliationError::Blocked("too_many_leases"));
            }
            let entry = entry.map_err(|source| {
                AdmissionReconciliationError::Admission(AdmissionError::ReadDir(source))
            })?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|source| {
                AdmissionReconciliationError::Admission(AdmissionError::Io {
                    path: path.clone(),
                    source,
                })
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(AdmissionReconciliationError::Blocked(
                    "foreign_or_malformed_lease",
                ));
            }
            let Some(id) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix(LEASE_PREFIX))
                .and_then(|name| name.strip_suffix(LEASE_SUFFIX))
                .filter(|id| id.len() == 20 && id.bytes().all(|byte| byte.is_ascii_digit()))
            else {
                return Err(AdmissionReconciliationError::Blocked(
                    "foreign_or_malformed_lease",
                ));
            };
            let lease = match read_lease(&path) {
                Ok(lease) => lease,
                Err(AdmissionError::MalformedLease(_)) => {
                    return Err(AdmissionReconciliationError::Blocked(
                        "foreign_or_malformed_lease",
                    ));
                }
                Err(error) => return Err(AdmissionReconciliationError::Admission(error)),
            };
            if lease.owner != "commit-ci-preflight"
                || lease.purpose != "host-admission-lease"
                || lease.schema_version != ADMISSION_SCHEMA_VERSION
                || lease.owner_run_id != id
                || !lease_is_semantically_valid(&lease)
            {
                return Err(AdmissionReconciliationError::Blocked(
                    "foreign_or_malformed_lease",
                ));
            }
            let ticket_path = self
                .root
                .join(TICKETS_DIR)
                .join(format!("{TICKET_PREFIX}{id}{TICKET_SUFFIX}"));
            let ticket_metadata = match fs::symlink_metadata(&ticket_path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Err(AdmissionReconciliationError::Blocked("lease_only_residue"));
                }
                Err(source) => {
                    return Err(AdmissionReconciliationError::Admission(
                        AdmissionError::Io {
                            path: ticket_path,
                            source,
                        },
                    ));
                }
            };
            if ticket_metadata.file_type().is_symlink() || !ticket_metadata.is_file() {
                return Err(AdmissionReconciliationError::Blocked(
                    "unsafe_selected_ticket",
                ));
            }
            let marker = match read_ticket(&ticket_path) {
                Ok(marker) => marker,
                Err(AdmissionError::MalformedTicket(_)) => {
                    return Err(AdmissionReconciliationError::Blocked(
                        "foreign_or_malformed_ticket",
                    ));
                }
                Err(error) => return Err(AdmissionReconciliationError::Admission(error)),
            };
            if marker.owner != "commit-ci-preflight"
                || marker.purpose != "host-admission-ticket"
                || marker.schema_version != ADMISSION_SCHEMA_VERSION
                || marker.ticket_id != id
            {
                return Err(AdmissionReconciliationError::Blocked(
                    "foreign_or_malformed_ticket",
                ));
            }
            if !lease_is_expired(&lease) {
                return Err(AdmissionReconciliationError::Blocked(
                    "slot_lease_contradiction",
                ));
            }
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

#[cfg(target_os = "linux")]
fn atomic_rename_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    // Linux renameat2(RENAME_NOREPLACE), available since Linux 3.15.
    let source = CString::new(source.as_os_str().as_encoded_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid source path"))?;
    let destination = CString::new(destination.as_os_str().as_encoded_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid destination path"))?;
    let result = unsafe {
        nix::libc::syscall(
            nix::libc::SYS_renameat2,
            nix::libc::AT_FDCWD,
            source.as_ptr(),
            nix::libc::AT_FDCWD,
            destination.as_ptr(),
            nix::libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
fn atomic_rename_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    // macOS renameatx_np(RENAME_EXCL) is the no-replace variant.
    let source = CString::new(source.as_os_str().as_encoded_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid source path"))?;
    let destination = CString::new(destination.as_os_str().as_encoded_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid destination path"))?;
    let result = unsafe {
        nix::libc::renameatx_np(
            nix::libc::AT_FDCWD,
            source.as_ptr(),
            nix::libc::AT_FDCWD,
            destination.as_ptr(),
            nix::libc::RENAME_EXCL,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn atomic_rename_no_replace(_source: &Path, _destination: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic no-replace rename unavailable on this host",
    ))
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

fn open_selected_ticket(path: &Path) -> Result<File, AdmissionReconciliationError> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        const O_NOFOLLOW: i32 = if cfg!(target_os = "macos") {
            0x100
        } else {
            0x20000
        };
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(O_NOFOLLOW)
            .open(path)
            .map_err(|source| {
                if source.kind() == io::ErrorKind::NotFound {
                    return AdmissionReconciliationError::Blocked("unknown_target");
                }
                let symlink = fs::symlink_metadata(path)
                    .map(|metadata| metadata.file_type().is_symlink())
                    .unwrap_or(false);
                if source.kind() == io::ErrorKind::TooManyLinks || symlink {
                    AdmissionReconciliationError::Blocked("unsafe_selected_ticket")
                } else {
                    AdmissionReconciliationError::Admission(AdmissionError::Io {
                        path: path.to_path_buf(),
                        source,
                    })
                }
            })?;
        if !file
            .metadata()
            .map_err(|source| {
                AdmissionReconciliationError::Admission(AdmissionError::Io {
                    path: path.to_path_buf(),
                    source,
                })
            })?
            .is_file()
        {
            return Err(AdmissionReconciliationError::Blocked(
                "unsafe_selected_ticket",
            ));
        }
        Ok(file)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        Err(AdmissionReconciliationError::Blocked(
            "unsupported_selected_ticket_open",
        ))
    }
}

fn read_ticket_from_descriptor(
    file: &mut File,
    path: &Path,
) -> Result<TicketMarker, AdmissionReconciliationError> {
    file.seek(std::io::SeekFrom::Start(0))
        .and_then(|_| {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).map(|_| bytes)
        })
        .map_err(|source| {
            AdmissionReconciliationError::Admission(AdmissionError::Io {
                path: path.to_path_buf(),
                source,
            })
        })
        .and_then(|bytes| {
            serde_json::from_slice(&bytes).map_err(|_| {
                AdmissionReconciliationError::Admission(AdmissionError::MalformedTicket(
                    path.to_path_buf(),
                ))
            })
        })
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

fn lease_is_semantically_valid(lease: &LeaseMarker) -> bool {
    matches!(lease.state.as_str(), "queued" | "active")
        && lease.heartbeat_at_unix_seconds >= lease.acquired_at_unix_seconds
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
    use std::sync::Arc;
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

    #[test]
    fn reconciliation_preview_is_read_only_and_classifies_an_absent_lease() {
        let coordinator = coordinator("reconcile-preview");
        coordinator.initialize().expect("initialize");
        let id = "00000000000000000001";
        let ticket_path = coordinator
            .root()
            .join(TICKETS_DIR)
            .join(format!("{TICKET_PREFIX}{id}{TICKET_SUFFIX}"));
        let marker = TicketMarker {
            owner: "commit-ci-preflight".to_owned(),
            purpose: "host-admission-ticket".to_owned(),
            schema_version: ADMISSION_SCHEMA_VERSION.to_owned(),
            ticket_id: id.to_owned(),
        };
        fs::write(&ticket_path, serde_json::to_vec(&marker).expect("marker")).expect("ticket");
        fs::write(coordinator.root().join(SLOT_LOCK), []).expect("slot lock");
        let before = tree_bytes(coordinator.root());
        let report = coordinator
            .reconcile_preview_with_timeout(Duration::from_secs(1), &CancellationToken::default())
            .expect("preview");
        assert_eq!(report.mode, AdmissionReconciliationModeV1::Preview);
        assert_eq!(
            report.candidates,
            vec![AdmissionReconciliationCandidateV1 {
                ticket_id: id.to_owned(),
                classification: "eligible_absent_lease".to_owned(),
            }]
        );
        assert_eq!(before, tree_bytes(coordinator.root()));
    }

    fn tree_bytes(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        fn visit(path: &Path, root: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
            for entry in fs::read_dir(path).expect("read tree") {
                let entry = entry.expect("entry");
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path).expect("metadata");
                if metadata.is_dir() {
                    visit(&path, root, out);
                } else if metadata.is_file() {
                    out.push((
                        path.strip_prefix(root).expect("relative").to_path_buf(),
                        fs::read(&path).expect("bytes"),
                    ));
                }
            }
        }
        let mut out = Vec::new();
        visit(root, root, &mut out);
        out.sort();
        out
    }

    fn fixture_ticket(
        coordinator: &AdmissionCoordinator,
        id: &str,
        marker: TicketMarker,
    ) -> PathBuf {
        fs::write(coordinator.root().join(SLOT_LOCK), []).expect("slot lock");
        let path = coordinator
            .root()
            .join(TICKETS_DIR)
            .join(format!("{TICKET_PREFIX}{id}{TICKET_SUFFIX}"));
        fs::write(&path, serde_json::to_vec(&marker).expect("marker")).expect("ticket");
        path
    }

    fn valid_marker(id: &str) -> TicketMarker {
        TicketMarker {
            owner: "commit-ci-preflight".to_owned(),
            purpose: "host-admission-ticket".to_owned(),
            schema_version: ADMISSION_SCHEMA_VERSION.to_owned(),
            ticket_id: id.to_owned(),
        }
    }

    fn write_lease_fixture(
        coordinator: &AdmissionCoordinator,
        id: &str,
        state: &str,
        acquired: u64,
        heartbeat: u64,
    ) {
        let lease = LeaseMarker {
            owner: "commit-ci-preflight".to_owned(),
            purpose: "host-admission-lease".to_owned(),
            schema_version: ADMISSION_SCHEMA_VERSION.to_owned(),
            owner_run_id: id.to_owned(),
            acquired_at_unix_seconds: acquired,
            heartbeat_at_unix_seconds: heartbeat,
            state: state.to_owned(),
        };
        fs::write(
            coordinator.lease_path(id),
            serde_json::to_vec(&lease).expect("lease"),
        )
        .expect("lease");
    }

    #[test]
    fn reconciliation_preview_preserves_held_ticket_with_future_lease() {
        let c = coordinator("reconcile-held-future");
        c.initialize().expect("initialize");
        let id = "00000000000000000002";
        let path = fixture_ticket(&c, id, valid_marker(id));
        write_lease_fixture(&c, id, "active", 4_000_000_000, 4_000_000_001);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .expect("open");
        file.lock_exclusive().expect("lock");
        let before = tree_bytes(c.root());
        let report = c
            .reconcile_preview_with_timeout(Duration::from_secs(1), &CancellationToken::default())
            .expect("preview");
        assert_eq!(report.candidates[0].classification, "blocked_ticket_locked");
        assert_eq!(before, tree_bytes(c.root()));
    }

    #[test]
    fn reconciliation_preview_absent_root_preserves_parent() {
        let parent = test_root("reconcile-absent-parent");
        let _ = fs::remove_dir_all(&parent);
        fs::create_dir_all(&parent).expect("parent");
        let root = parent.join("root");
        let metadata = fs::symlink_metadata(&parent).expect("parent metadata");
        let before = (metadata.len(), metadata.modified().expect("mtime"));
        let c = AdmissionCoordinator::test_at(root.clone());
        let report = c
            .reconcile_preview_with_timeout(Duration::from_secs(1), &CancellationToken::default())
            .expect("preview");
        assert!(report.candidates.is_empty());
        assert!(!root.exists());
        let metadata = fs::symlink_metadata(&parent).expect("parent metadata");
        assert_eq!(
            before,
            (metadata.len(), metadata.modified().expect("mtime"))
        );
    }

    #[test]
    fn reconciliation_preview_rejects_symlink_ticket_without_mutation() {
        let c = coordinator("reconcile-symlink");
        c.initialize().expect("initialize");
        let id = "00000000000000000003";
        let path = c
            .root()
            .join(TICKETS_DIR)
            .join(format!("{TICKET_PREFIX}{id}{TICKET_SUFFIX}"));
        fixture_ticket(
            &c,
            "00000000000000000004",
            valid_marker("00000000000000000004"),
        );
        fs::remove_file(
            c.root()
                .join(TICKETS_DIR)
                .join("ticket-00000000000000000004.json"),
        )
        .expect("remove");
        std::os::unix::fs::symlink(c.root().join(OWNER_FILE), &path).expect("symlink");
        let before = tree_bytes(c.root());
        let target_before = fs::read_link(&path).expect("link target");
        let metadata_before = fs::symlink_metadata(&path).expect("link metadata");
        let result =
            c.reconcile_preview_with_timeout(Duration::from_secs(1), &CancellationToken::default());
        assert!(matches!(
            result,
            Err(AdmissionReconciliationError::Admission(
                AdmissionError::UnsafeLayout(_)
            ))
        ));
        assert_eq!(before, tree_bytes(c.root()));
        assert_eq!(target_before, fs::read_link(&path).expect("link target"));
        assert_eq!(
            metadata_before.file_type(),
            fs::symlink_metadata(&path)
                .expect("link metadata")
                .file_type()
        );
    }

    #[test]
    fn reconciliation_preview_rejects_foreign_json_without_mutation() {
        let c = coordinator("reconcile-foreign");
        c.initialize().expect("initialize");
        let id = "00000000000000000005";
        let mut marker = valid_marker(id);
        marker.owner = "foreign".to_owned();
        fixture_ticket(&c, id, marker);
        let before = tree_bytes(c.root());
        let result =
            c.reconcile_preview_with_timeout(Duration::from_secs(1), &CancellationToken::default());
        assert!(matches!(
            result,
            Err(AdmissionReconciliationError::Admission(
                AdmissionError::ForeignTicket(_)
            ))
        ));
        assert_eq!(before, tree_bytes(c.root()));
    }

    #[test]
    fn reconciliation_preview_missing_slot_lock_is_blocked_without_creation() {
        let c = coordinator("reconcile-no-slot");
        c.initialize().expect("initialize");
        let id = "00000000000000000006";
        let path = c
            .root()
            .join(TICKETS_DIR)
            .join(format!("{TICKET_PREFIX}{id}{TICKET_SUFFIX}"));
        fs::write(
            &path,
            serde_json::to_vec(&valid_marker(id)).expect("marker"),
        )
        .expect("ticket");
        let before = tree_bytes(c.root());
        let result =
            c.reconcile_preview_with_timeout(Duration::from_secs(1), &CancellationToken::default());
        assert!(matches!(
            result,
            Err(AdmissionReconciliationError::Blocked("missing_slot_lock"))
        ));
        assert_eq!(before, tree_bytes(c.root()));
        assert!(!c.root().join(SLOT_LOCK).exists());
    }

    #[test]
    fn reconciliation_preview_blocks_invalid_lease_before_expiration() {
        let c = coordinator("reconcile-invalid-lease");
        c.initialize().expect("initialize");
        let id = "00000000000000000007";
        fixture_ticket(&c, id, valid_marker(id));
        write_lease_fixture(&c, id, "unknown", 4_000_000_000, 1);
        let before = tree_bytes(c.root());
        let report = c
            .reconcile_preview_with_timeout(Duration::from_secs(1), &CancellationToken::default())
            .expect("preview");
        assert_eq!(report.candidates[0].classification, "blocked_invalid_lease");
        assert_eq!(before, tree_bytes(c.root()));
    }

    #[test]
    fn reconciliation_preview_blocks_timestamp_inconsistent_active_lease() {
        let c = coordinator("reconcile-inconsistent-lease");
        c.initialize().expect("initialize");
        let id = "00000000000000000008";
        fixture_ticket(&c, id, valid_marker(id));
        write_lease_fixture(&c, id, "active", 4_000_000_001, 4_000_000_000);
        let before = tree_bytes(c.root());
        let report = c
            .reconcile_preview_with_timeout(Duration::from_secs(1), &CancellationToken::default())
            .expect("preview");
        assert_eq!(report.candidates[0].classification, "blocked_invalid_lease");
        assert_eq!(before, tree_bytes(c.root()));
    }

    #[test]
    fn reconciliation_apply_quarantines_only_the_selected_expired_ticket() {
        let c = coordinator("reconcile-target");
        c.initialize().expect("initialize");
        let selected = "00000000000000000009";
        let untouched = "00000000000000000010";
        fixture_ticket(&c, selected, valid_marker(selected));
        fixture_ticket(&c, untouched, valid_marker(untouched));
        fs::write(c.root().join(NEXT_TICKET), b"1\n").expect("counter");
        write_lease_fixture(&c, selected, "active", 1, 1);
        write_lease_fixture(&c, untouched, "active", 1, 1);
        let counter = fs::read(c.root().join(NEXT_TICKET)).expect("counter");
        let report = c
            .reconcile_apply_with_timeout(
                &[selected.to_owned()],
                Duration::from_secs(1),
                &CancellationToken::default(),
            )
            .expect("apply");
        assert_eq!(report.outcomes, vec![outcome(selected, "quarantined")]);
        assert!(
            !c.root()
                .join(TICKETS_DIR)
                .join(format!("{TICKET_PREFIX}{selected}{TICKET_SUFFIX}"))
                .exists()
        );
        assert!(
            c.root()
                .join(TICKETS_DIR)
                .join(format!("{TICKET_PREFIX}{untouched}{TICKET_SUFFIX}"))
                .exists()
        );
        assert_eq!(
            counter,
            fs::read(c.root().join(NEXT_TICKET)).expect("counter")
        );
    }

    #[test]
    fn reconciliation_apply_quarantines_selected_ticket_with_expired_queued_lease() {
        let c = coordinator("reconcile-expired-queued-apply-review");
        c.initialize().expect("initialize");
        let id = "00000000000000000071";
        let path = fixture_ticket(&c, id, valid_marker(id));
        write_lease_fixture(&c, id, "queued", 1, 1);

        let preview = c
            .reconcile_preview_with_timeout(Duration::from_secs(1), &CancellationToken::default())
            .expect("preview");
        assert_eq!(
            preview.candidates,
            vec![AdmissionReconciliationCandidateV1 {
                ticket_id: id.to_owned(),
                classification: "eligible_expired_lease".to_owned(),
            }]
        );

        let report = c
            .reconcile_apply_with_timeout(
                &[id.to_owned()],
                Duration::from_secs(1),
                &CancellationToken::default(),
            )
            .expect("apply accepts same expired queued lease as preview");

        assert_eq!(report.outcomes, vec![outcome(id, "quarantined")]);
        assert!(!path.exists());
        assert!(!c.lease_path(id).exists());
    }

    #[test]
    fn reconciliation_apply_rejects_incomplete_required_layout_without_mutation() {
        for (ordinal, missing) in [
            QUEUE_LOCK,
            SLOT_LOCK,
            TICKETS_DIR,
            LEASES_DIR,
            QUARANTINE_DIR,
        ]
        .into_iter()
        .enumerate()
        {
            let c = coordinator(&format!("reconcile-missing-layout-review-{ordinal}"));
            c.initialize().expect("initialize");
            let id = format!("{:020}", 72 + ordinal);
            fixture_ticket(&c, &id, valid_marker(&id));
            let path = c.root().join(missing);
            if path.is_dir() {
                fs::remove_dir_all(&path).expect("remove required directory fixture");
            } else {
                fs::remove_file(&path).expect("remove required file fixture");
            }
            let before = tree_bytes(c.root());

            let result = c.reconcile_apply_with_timeout(
                &[id],
                Duration::from_secs(1),
                &CancellationToken::default(),
            );

            assert!(matches!(
                result,
                Err(AdmissionReconciliationError::Blocked("invalid_layout"))
            ));
            assert_eq!(before, tree_bytes(c.root()));
            assert!(!path.exists());
        }
    }

    #[test]
    fn reconciliation_no_replace_collision_preserves_destination_bytes() {
        let c = coordinator("reconcile-collision-round2");
        c.initialize().expect("initialize");
        let source = c.root().join(TICKETS_DIR).join("source");
        let destination = c.root().join(QUARANTINE_DIR).join("fixed");
        fs::write(&source, b"source").expect("source");
        fs::write(&destination, b"preserve").expect("destination");
        let error = atomic_rename_no_replace(&source, &destination).expect_err("collision");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(destination).expect("bytes"), b"preserve");
        assert_eq!(fs::read(source).expect("source bytes"), b"source");
    }

    #[test]
    fn reconciliation_preview_then_changed_bytes_blocks_without_mutation() {
        let c = coordinator("reconcile-changed-round2");
        c.initialize().expect("initialize");
        let id = "00000000000000000021";
        let path = fixture_ticket(&c, id, valid_marker(id));
        let before_preview = tree_bytes(c.root());
        c.reconcile_preview_with_timeout(Duration::from_secs(1), &CancellationToken::default())
            .expect("preview");
        assert_eq!(before_preview, tree_bytes(c.root()));
        fs::write(&path, b"changed").expect("change");
        let retained_after_change = tree_bytes(c.root());
        assert_ne!(before_preview, retained_after_change);
        let result = c.reconcile_apply_with_timeout(
            &[id.to_owned()],
            Duration::from_secs(1),
            &CancellationToken::default(),
        );
        assert!(result.is_err());
        assert!(path.exists());
        assert_eq!(fs::read(c.lease_path(id)).is_err(), true);
        assert_eq!(retained_after_change, tree_bytes(c.root()));
    }

    fn outcome(ticket_id: &str, classification: &str) -> AdmissionReconciliationOutcomeV1 {
        AdmissionReconciliationOutcomeV1 {
            ticket_id: ticket_id.to_owned(),
            classification: classification.to_owned(),
        }
    }

    fn expired_ticket(coordinator: &AdmissionCoordinator, id: &str) -> PathBuf {
        let path = fixture_ticket(coordinator, id, valid_marker(id));
        write_lease_fixture(coordinator, id, "active", 1, 1);
        path
    }

    struct ReconciliationProtectedState {
        files: Vec<(PathBuf, Vec<u8>)>,
    }

    impl ReconciliationProtectedState {
        fn capture(coordinator: &AdmissionCoordinator, unselected_id: &str) -> Self {
            fs::write(coordinator.root().join(NEXT_TICKET), b"77\n").expect("counter");
            let unselected_ticket = expired_ticket(coordinator, unselected_id);
            let journal = coordinator.root().with_extension("journal-sentinel");
            let cache = coordinator.root().with_extension("cache-sentinel");
            fs::write(&journal, b"journal unchanged").expect("journal sentinel");
            fs::write(&cache, b"cache unchanged").expect("cache sentinel");
            let files = [
                coordinator.root().join(NEXT_TICKET),
                journal,
                cache,
                unselected_ticket,
                coordinator.lease_path(unselected_id),
            ]
            .into_iter()
            .map(|path| {
                let bytes = fs::read(&path).expect("protected bytes");
                (path, bytes)
            })
            .collect();
            Self { files }
        }

        fn assert_unchanged(&self) {
            for (path, expected) in &self.files {
                assert_eq!(&fs::read(path).expect("retained protected bytes"), expected);
            }
        }
    }

    fn assert_reconciliation_locks_reusable(
        coordinator: &AdmissionCoordinator,
        ticket_paths: &[&Path],
    ) {
        for path in [
            coordinator.root().join(QUEUE_LOCK),
            coordinator.root().join(SLOT_LOCK),
        ] {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .expect("open reusable coordinator lock");
            file.try_lock_exclusive()
                .expect("coordinator lock reusable");
            FileExt::unlock(&file).expect("unlock reusable coordinator lock");
        }
        for path in ticket_paths {
            if path.exists() {
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(path)
                    .expect("open retained ticket");
                file.try_lock_exclusive().expect("ticket lock reusable");
                FileExt::unlock(&file).expect("unlock retained ticket");
            }
        }
    }

    #[test]
    fn reconciliation_injected_lease_remove_failure_before_mutation_preserves_everything() {
        let c = coordinator("reconcile-lease-remove-pre-mutation-round5");
        c.initialize().expect("initialize");
        let selected = "00000000000000000043";
        let unselected = "00000000000000000044";
        let selected_path = expired_ticket(&c, selected);
        let protected = ReconciliationProtectedState::capture(&c, unselected);
        let before = tree_bytes(c.root());
        c.fail_reconciliation_lease_remove_on_attempt(1);

        let result = c.reconcile_apply_with_timeout(
            &[selected.to_owned()],
            Duration::from_secs(1),
            &CancellationToken::default(),
        );

        assert!(matches!(
            result,
            Err(AdmissionReconciliationError::Admission(
                AdmissionError::Io { .. }
            ))
        ));
        assert_eq!(before, tree_bytes(c.root()));
        protected.assert_unchanged();
        assert_reconciliation_locks_reusable(&c, &[&selected_path]);
    }

    #[test]
    fn reconciliation_injected_lease_remove_failure_after_success_is_canonical_partial() {
        let c = coordinator("reconcile-lease-remove-partial-round5");
        c.initialize().expect("initialize");
        let first = "00000000000000000045";
        let second = "00000000000000000046";
        let third = "00000000000000000047";
        let unselected = "00000000000000000048";
        expired_ticket(&c, first);
        let second_path = expired_ticket(&c, second);
        let third_path = expired_ticket(&c, third);
        let second_before = fs::read(&second_path).expect("second ticket");
        let second_lease_before = fs::read(c.lease_path(second)).expect("second lease");
        let third_before = fs::read(&third_path).expect("third ticket");
        let third_lease_before = fs::read(c.lease_path(third)).expect("third lease");
        let protected = ReconciliationProtectedState::capture(&c, unselected);
        c.fail_reconciliation_lease_remove_on_attempt(2);

        let result = c.reconcile_apply_with_timeout(
            &[third.to_owned(), first.to_owned(), second.to_owned()],
            Duration::from_secs(1),
            &CancellationToken::default(),
        );

        match result {
            Err(AdmissionReconciliationError::Partial { reason, report }) => {
                assert_eq!(reason, "lease_remove_failed");
                assert_eq!(
                    report.outcomes,
                    vec![
                        outcome(first, "quarantined"),
                        outcome(second, "partial_lease_remove_failed"),
                        outcome(third, "not_attempted"),
                    ]
                );
            }
            other => panic!("unexpected result: {other:?}"),
        }
        assert_eq!(
            fs::read(&second_path).expect("second retained"),
            second_before
        );
        assert_eq!(
            fs::read(c.lease_path(second)).expect("second lease retained"),
            second_lease_before
        );
        assert_eq!(fs::read(&third_path).expect("third retained"), third_before);
        assert_eq!(
            fs::read(c.lease_path(third)).expect("third lease retained"),
            third_lease_before
        );
        protected.assert_unchanged();
        assert_reconciliation_locks_reusable(&c, &[&second_path, &third_path]);
    }

    fn assert_two_target_partial(
        result: Result<AdmissionReconciliationApplyReportV1, AdmissionReconciliationError>,
        reason_expected: &str,
        first_id: &str,
        first_classification: &str,
        second_id: &str,
    ) {
        match result {
            Err(AdmissionReconciliationError::Partial { reason, report }) => {
                assert_eq!(reason, reason_expected);
                assert_eq!(
                    report.outcomes,
                    vec![
                        outcome(first_id, first_classification),
                        outcome(second_id, "not_attempted"),
                    ]
                );
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn reconciliation_injected_lease_directory_sync_failure_is_canonical_partial() {
        let c = coordinator("reconcile-lease-sync-round5");
        c.initialize().expect("initialize");
        let first = "00000000000000000049";
        let second = "00000000000000000050";
        let unselected = "00000000000000000051";
        let first_path = expired_ticket(&c, first);
        let second_path = expired_ticket(&c, second);
        let second_before = fs::read(&second_path).expect("second ticket");
        let second_lease_before = fs::read(c.lease_path(second)).expect("second lease");
        let protected = ReconciliationProtectedState::capture(&c, unselected);
        c.fail_reconciliation_sync_on(ReconciliationSyncPoint::LeaseDirectory, 1);

        let result = c.reconcile_apply_with_timeout(
            &[second.to_owned(), first.to_owned()],
            Duration::from_secs(1),
            &CancellationToken::default(),
        );

        assert_two_target_partial(
            result,
            "lease_sync_failed",
            first,
            "partial_lease_sync_failed",
            second,
        );
        assert!(first_path.exists());
        assert!(!c.lease_path(first).exists());
        assert_eq!(
            fs::read(&second_path).expect("second retained"),
            second_before
        );
        assert_eq!(
            fs::read(c.lease_path(second)).expect("second lease retained"),
            second_lease_before
        );
        protected.assert_unchanged();
        assert_reconciliation_locks_reusable(&c, &[&first_path, &second_path]);
    }

    #[test]
    fn reconciliation_injected_tickets_directory_sync_failure_is_canonical_partial() {
        let c = coordinator("reconcile-ticket-sync-round5");
        c.initialize().expect("initialize");
        let first = "00000000000000000052";
        let second = "00000000000000000053";
        let unselected = "00000000000000000054";
        let first_path = expired_ticket(&c, first);
        let second_path = expired_ticket(&c, second);
        let second_before = fs::read(&second_path).expect("second ticket");
        let second_lease_before = fs::read(c.lease_path(second)).expect("second lease");
        let protected = ReconciliationProtectedState::capture(&c, unselected);
        c.set_reconciliation_quarantine_suffix("ticket-sync-failure");
        c.fail_reconciliation_sync_on(ReconciliationSyncPoint::TicketsDirectory, 1);

        let result = c.reconcile_apply_with_timeout(
            &[first.to_owned(), second.to_owned()],
            Duration::from_secs(1),
            &CancellationToken::default(),
        );

        assert_two_target_partial(
            result,
            "ticket_sync_failed",
            first,
            "partial_ticket_sync_failed",
            second,
        );
        assert!(!first_path.exists());
        assert!(!c.lease_path(first).exists());
        assert_eq!(
            fs::read(&second_path).expect("second retained"),
            second_before
        );
        assert_eq!(
            fs::read(c.lease_path(second)).expect("second lease retained"),
            second_lease_before
        );
        protected.assert_unchanged();
        assert_reconciliation_locks_reusable(&c, &[&second_path]);
    }

    #[test]
    fn reconciliation_injected_quarantine_directory_sync_failure_is_canonical_partial() {
        let c = coordinator("reconcile-quarantine-sync-round5");
        c.initialize().expect("initialize");
        let first = "00000000000000000055";
        let second = "00000000000000000056";
        let unselected = "00000000000000000057";
        let first_path = expired_ticket(&c, first);
        let second_path = expired_ticket(&c, second);
        let second_before = fs::read(&second_path).expect("second ticket");
        let second_lease_before = fs::read(c.lease_path(second)).expect("second lease");
        let protected = ReconciliationProtectedState::capture(&c, unselected);
        c.set_reconciliation_quarantine_suffix("quarantine-sync-failure");
        c.fail_reconciliation_sync_on(ReconciliationSyncPoint::QuarantineDirectory, 1);

        let result = c.reconcile_apply_with_timeout(
            &[second.to_owned(), first.to_owned()],
            Duration::from_secs(1),
            &CancellationToken::default(),
        );

        assert_two_target_partial(
            result,
            "quarantine_sync_failed",
            first,
            "partial_quarantine_sync_failed",
            second,
        );
        assert!(!first_path.exists());
        assert!(!c.lease_path(first).exists());
        assert_eq!(
            fs::read(&second_path).expect("second retained"),
            second_before
        );
        assert_eq!(
            fs::read(c.lease_path(second)).expect("second lease retained"),
            second_lease_before
        );
        protected.assert_unchanged();
        assert_reconciliation_locks_reusable(&c, &[&second_path]);
    }

    #[test]
    fn reconciliation_cancellation_after_lease_removal_is_canonical_partial() {
        let c = coordinator("reconcile-cancel-after-mutation-round5");
        c.initialize().expect("initialize");
        let first = "00000000000000000058";
        let second = "00000000000000000059";
        let unselected = "00000000000000000060";
        let first_path = expired_ticket(&c, first);
        let second_path = expired_ticket(&c, second);
        let second_before = fs::read(&second_path).expect("second ticket");
        let second_lease_before = fs::read(c.lease_path(second)).expect("second lease");
        let protected = ReconciliationProtectedState::capture(&c, unselected);
        let gate = Arc::new(ReconciliationTestGate::new());
        c.set_reconciliation_after_lease_removal_gate(1, gate.clone());
        assert_eq!(c.reconciliation_cleanup_runs(), 0);
        let cancellation = CancellationToken::default();
        let worker_c = c.clone();
        let worker_cancel = cancellation.clone();
        let worker = thread::spawn(move || {
            worker_c.reconcile_apply_with_timeout(
                &[first.to_owned(), second.to_owned()],
                Duration::from_secs(1),
                &worker_cancel,
            )
        });
        gate.wait_until_reached();
        cancellation.cancel();
        gate.release();

        assert_two_target_partial(
            worker.join().expect("reconciler"),
            "cancelled",
            first,
            "partial_after_lease_removal",
            second,
        );
        assert!(first_path.exists());
        assert!(!c.lease_path(first).exists());
        assert_eq!(
            fs::read(&second_path).expect("second retained"),
            second_before
        );
        assert_eq!(
            fs::read(c.lease_path(second)).expect("second lease retained"),
            second_lease_before
        );
        protected.assert_unchanged();
        assert_reconciliation_locks_reusable(&c, &[&first_path, &second_path]);
        assert_eq!(c.reconciliation_cleanup_runs(), 1);
    }

    #[test]
    fn reconciliation_timeout_after_lease_removal_is_canonical_partial() {
        let c = coordinator("reconcile-timeout-after-mutation-round5");
        c.initialize().expect("initialize");
        let first = "00000000000000000061";
        let second = "00000000000000000062";
        let unselected = "00000000000000000063";
        let first_path = expired_ticket(&c, first);
        let second_path = expired_ticket(&c, second);
        let second_before = fs::read(&second_path).expect("second ticket");
        let second_lease_before = fs::read(c.lease_path(second)).expect("second lease");
        let protected = ReconciliationProtectedState::capture(&c, unselected);
        let gate = Arc::new(ReconciliationTestGate::new());
        c.set_reconciliation_after_lease_removal_gate(1, gate.clone());
        let worker_c = c.clone();
        let worker = thread::spawn(move || {
            worker_c.reconcile_apply_with_timeout(
                &[first.to_owned(), second.to_owned()],
                Duration::from_millis(80),
                &CancellationToken::default(),
            )
        });
        gate.wait_until_reached();
        thread::sleep(Duration::from_millis(120));
        gate.release();

        assert_two_target_partial(
            worker.join().expect("reconciler"),
            "timeout",
            first,
            "partial_after_lease_removal",
            second,
        );
        assert!(first_path.exists());
        assert!(!c.lease_path(first).exists());
        assert_eq!(
            fs::read(&second_path).expect("second retained"),
            second_before
        );
        assert_eq!(
            fs::read(c.lease_path(second)).expect("second lease retained"),
            second_lease_before
        );
        protected.assert_unchanged();
        assert_reconciliation_locks_reusable(&c, &[&first_path, &second_path]);
    }

    #[test]
    fn reconciliation_apply_rejects_more_than_maximum_selected_ids() {
        let c = coordinator("reconcile-target-bound-round4");
        let selected: Vec<String> = (0..=MAX_QUEUE_TICKETS)
            .map(|value| format!("{value:020}"))
            .collect();

        let result = c.reconcile_apply_with_timeout(
            &selected,
            Duration::from_secs(1),
            &CancellationToken::default(),
        );

        assert!(matches!(
            result,
            Err(AdmissionReconciliationError::Blocked("too_many_targets"))
        ));
        assert!(!c.root().exists());
    }

    #[test]
    fn reconciliation_exclusion_blocks_acquire_then_releases_reusable_locks() {
        let c = coordinator("reconcile-acquire-exclusion-round4");
        c.initialize().expect("initialize");
        let id = "00000000000000000031";
        expired_ticket(&c, id);
        let gate = Arc::new(ReconciliationTestGate::new());
        c.set_reconciliation_gate(ReconciliationTestGatePoint::AfterExclusion, gate.clone());

        let reconcile_coordinator = c.clone();
        let reconcile_id = id.to_owned();
        let reconciler = thread::spawn(move || {
            reconcile_coordinator.reconcile_apply_with_timeout(
                &[reconcile_id],
                Duration::from_secs(2),
                &CancellationToken::default(),
            )
        });
        gate.wait_until_reached();

        let queue = open_existing_lock_file(&c.root().join(QUEUE_LOCK))
            .expect("open queue")
            .expect("queue exists");
        assert!(matches!(
            queue.try_lock_exclusive(),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock
        ));

        let (started_tx, started_rx) = mpsc::sync_channel(0);
        let (completed_tx, completed_rx) = mpsc::sync_channel(0);
        let acquire_coordinator = c.clone();
        let acquirer = thread::spawn(move || {
            started_tx.send(()).expect("started receiver");
            let result = acquire_coordinator
                .acquire(Duration::from_secs(2), &CancellationToken::default())
                .and_then(AdmissionGuard::release);
            completed_tx.send(result).expect("completed receiver");
        });
        started_rx.recv().expect("acquire started");
        assert!(matches!(
            completed_rx.recv_timeout(Duration::from_millis(80)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));

        gate.release();
        assert_eq!(
            reconciler
                .join()
                .expect("reconciler")
                .expect("apply")
                .outcomes,
            vec![outcome(id, "quarantined")]
        );
        completed_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("acquire completes after reconciliation")
            .expect("acquire succeeds");
        acquirer.join().expect("acquirer");

        c.acquire(Duration::from_secs(1), &CancellationToken::default())
            .expect("locks reusable")
            .release()
            .expect("release reusable lock");
    }

    #[test]
    fn reconciliation_active_guard_busy_slot_blocks_immediately_and_preserves_reuse() {
        let c = coordinator("reconcile-active-guard-round5");
        let guard = c
            .acquire(Duration::from_secs(1), &CancellationToken::default())
            .expect("active guard");
        let active_id = guard.ticket_id().to_owned();
        let unselected = "00000000000000000066";
        let protected = ReconciliationProtectedState::capture(&c, unselected);
        let before = tree_bytes(c.root());
        let started = Instant::now();

        let result = c.reconcile_apply_with_timeout(
            &[active_id],
            Duration::from_secs(2),
            &CancellationToken::default(),
        );

        assert!(
            started.elapsed() < Duration::from_millis(250),
            "busy slot must not be waited on while queue lock is held"
        );
        assert!(matches!(
            result,
            Err(AdmissionReconciliationError::Blocked("slot_busy"))
        ));
        assert_eq!(before, tree_bytes(c.root()));
        protected.assert_unchanged();
        guard.release().expect("active guard release");
        let unselected_path = c
            .root()
            .join(TICKETS_DIR)
            .join(format!("{TICKET_PREFIX}{unselected}{TICKET_SUFFIX}"));
        assert_reconciliation_locks_reusable(&c, &[&unselected_path]);
    }

    #[test]
    fn reconciliation_rechecks_every_selected_identity_before_first_mutation() {
        let c = coordinator("reconcile-all-identities-round4");
        c.initialize().expect("initialize");
        let first = "00000000000000000032";
        let second = "00000000000000000033";
        let unselected = "00000000000000000067";
        let first_path = expired_ticket(&c, first);
        let second_path = expired_ticket(&c, second);
        let first_lease = fs::read(c.lease_path(first)).expect("first lease");
        let second_lease = fs::read(c.lease_path(second)).expect("second lease");
        let protected = ReconciliationProtectedState::capture(&c, unselected);
        let gate = Arc::new(ReconciliationTestGate::new());
        c.set_reconciliation_gate(
            ReconciliationTestGatePoint::AfterSelectedDescriptors,
            gate.clone(),
        );

        let reconcile_coordinator = c.clone();
        let reconciler = thread::spawn(move || {
            reconcile_coordinator.reconcile_apply_with_timeout(
                &[second.to_owned(), first.to_owned()],
                Duration::from_secs(2),
                &CancellationToken::default(),
            )
        });
        gate.wait_until_reached();
        fs::remove_file(&second_path).expect("replace selected pathname");
        fs::write(
            &second_path,
            serde_json::to_vec(&valid_marker(second)).expect("replacement marker"),
        )
        .expect("replacement ticket");
        gate.release();

        assert!(matches!(
            reconciler.join().expect("reconciler"),
            Err(AdmissionReconciliationError::Blocked(
                "selected_ticket_changed"
            ))
        ));
        assert!(first_path.exists());
        assert!(second_path.exists());
        assert_eq!(
            fs::read(c.lease_path(first)).expect("first lease"),
            first_lease
        );
        assert_eq!(
            fs::read(c.lease_path(second)).expect("second lease"),
            second_lease
        );
        assert_eq!(
            fs::read_dir(c.root().join(QUARANTINE_DIR))
                .expect("quarantine")
                .count(),
            0
        );
        protected.assert_unchanged();
        assert_reconciliation_locks_reusable(&c, &[&first_path, &second_path]);

        let retry = c
            .reconcile_apply_with_timeout(
                &[second.to_owned(), first.to_owned()],
                Duration::from_secs(1),
                &CancellationToken::default(),
            )
            .expect("locks reusable after identity rejection");
        assert_eq!(
            retry.outcomes,
            vec![
                outcome(first, "quarantined"),
                outcome(second, "quarantined")
            ]
        );
        protected.assert_unchanged();
    }

    #[test]
    fn reconciliation_apply_collision_stops_with_canonical_partial_report() {
        let c = coordinator("reconcile-collision-sequence-round4");
        c.initialize().expect("initialize");
        let first = "00000000000000000034";
        let second = "00000000000000000035";
        let third = "00000000000000000036";
        let unselected = "00000000000000000064";
        expired_ticket(&c, first);
        let second_path = expired_ticket(&c, second);
        let third_path = expired_ticket(&c, third);
        c.set_reconciliation_quarantine_suffix("collision");
        let collision = c.root().join(QUARANTINE_DIR).join(format!(
            "{}.collision",
            second_path
                .file_name()
                .expect("ticket name")
                .to_string_lossy()
        ));
        fs::write(&collision, b"preserve collision evidence").expect("collision fixture");
        let third_lease = fs::read(c.lease_path(third)).expect("third lease");
        let protected = ReconciliationProtectedState::capture(&c, unselected);

        let result = c.reconcile_apply_with_timeout(
            &[third.to_owned(), first.to_owned(), second.to_owned()],
            Duration::from_secs(1),
            &CancellationToken::default(),
        );

        match result {
            Err(AdmissionReconciliationError::Partial { reason, report }) => {
                assert_eq!(reason, "quarantine_collision");
                assert_eq!(
                    report.outcomes,
                    vec![
                        outcome(first, "quarantined"),
                        outcome(second, "partial_quarantine_collision"),
                        outcome(third, "not_attempted"),
                    ]
                );
            }
            other => panic!("unexpected result: {other:?}"),
        }
        assert_eq!(
            fs::read(&collision).expect("collision evidence"),
            b"preserve collision evidence"
        );
        assert!(second_path.exists());
        assert!(!c.lease_path(second).exists());
        assert!(third_path.exists());
        assert_eq!(
            fs::read(c.lease_path(third)).expect("third lease"),
            third_lease
        );
        protected.assert_unchanged();
        assert_reconciliation_locks_reusable(&c, &[&second_path, &third_path]);
    }

    #[test]
    fn reconciliation_injected_move_failure_stops_with_residual_evidence() {
        let c = coordinator("reconcile-move-failure-sequence-round4");
        c.initialize().expect("initialize");
        let first = "00000000000000000037";
        let second = "00000000000000000038";
        let third = "00000000000000000039";
        let unselected = "00000000000000000065";
        expired_ticket(&c, first);
        let second_path = expired_ticket(&c, second);
        let third_path = expired_ticket(&c, third);
        let third_lease = fs::read(c.lease_path(third)).expect("third lease");
        let protected = ReconciliationProtectedState::capture(&c, unselected);
        c.fail_reconciliation_move_on_attempt(2);

        let result = c.reconcile_apply_with_timeout(
            &[third.to_owned(), second.to_owned(), first.to_owned()],
            Duration::from_secs(1),
            &CancellationToken::default(),
        );

        match result {
            Err(AdmissionReconciliationError::Partial { reason, report }) => {
                assert_eq!(reason, "move_failed");
                assert_eq!(
                    report.outcomes,
                    vec![
                        outcome(first, "quarantined"),
                        outcome(second, "partial_move_failed"),
                        outcome(third, "not_attempted"),
                    ]
                );
            }
            other => panic!("unexpected result: {other:?}"),
        }
        assert!(second_path.exists());
        assert!(!c.lease_path(second).exists());
        assert!(third_path.exists());
        assert_eq!(
            fs::read(c.lease_path(third)).expect("third lease"),
            third_lease
        );
        protected.assert_unchanged();
        assert_reconciliation_locks_reusable(&c, &[&second_path, &third_path]);

        let retry = c
            .reconcile_apply_with_timeout(
                &[second.to_owned(), third.to_owned()],
                Duration::from_secs(1),
                &CancellationToken::default(),
            )
            .expect("locks reusable after move failure");
        assert_eq!(
            retry.outcomes,
            vec![
                outcome(second, "quarantined"),
                outcome(third, "quarantined")
            ]
        );
        protected.assert_unchanged();
    }

    #[test]
    fn reconciliation_sync_failure_reports_moved_residual_and_stops() {
        let c = coordinator("reconcile-sync-failure-sequence-round4");
        c.initialize().expect("initialize");
        let first = "00000000000000000040";
        let second = "00000000000000000041";
        let third = "00000000000000000042";
        let unselected = "00000000000000000068";
        expired_ticket(&c, first);
        let second_path = expired_ticket(&c, second);
        let third_path = expired_ticket(&c, third);
        let third_lease = fs::read(c.lease_path(third)).expect("third lease");
        let protected = ReconciliationProtectedState::capture(&c, unselected);
        c.set_reconciliation_quarantine_suffix("sync-failure");
        c.fail_reconciliation_sync_on(ReconciliationSyncPoint::QuarantineDirectory, 2);

        let result = c.reconcile_apply_with_timeout(
            &[third.to_owned(), first.to_owned(), second.to_owned()],
            Duration::from_secs(1),
            &CancellationToken::default(),
        );

        match result {
            Err(AdmissionReconciliationError::Partial { reason, report }) => {
                assert_eq!(reason, "quarantine_sync_failed");
                assert_eq!(
                    report.outcomes,
                    vec![
                        outcome(first, "quarantined"),
                        outcome(second, "partial_quarantine_sync_failed"),
                        outcome(third, "not_attempted"),
                    ]
                );
            }
            other => panic!("unexpected result: {other:?}"),
        }
        assert!(!second_path.exists());
        assert!(!c.lease_path(second).exists());
        assert!(
            c.root()
                .join(QUARANTINE_DIR)
                .join(format!(
                    "{}.sync-failure",
                    second_path
                        .file_name()
                        .expect("ticket name")
                        .to_string_lossy()
                ))
                .exists()
        );
        assert!(third_path.exists());
        assert_eq!(
            fs::read(c.lease_path(third)).expect("third lease"),
            third_lease
        );
        protected.assert_unchanged();
        assert_reconciliation_locks_reusable(&c, &[&third_path]);

        assert_eq!(
            c.reconcile_apply_with_timeout(
                &[third.to_owned()],
                Duration::from_secs(1),
                &CancellationToken::default(),
            )
            .expect("locks reusable after sync failure")
            .outcomes,
            vec![outcome(third, "quarantined")]
        );
        protected.assert_unchanged();
    }

    #[test]
    fn reconciliation_apply_rejects_held_ticket_without_mutation() {
        let c = coordinator("reconcile-held-apply");
        c.initialize().expect("initialize");
        let id = "00000000000000000011";
        let path = fixture_ticket(&c, id, valid_marker(id));
        fs::write(c.root().join(NEXT_TICKET), b"1\n").expect("counter");
        write_lease_fixture(&c, id, "active", 1, 1);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .expect("open");
        file.lock_exclusive().expect("lock");
        let before = tree_bytes(c.root());
        let result = c.reconcile_apply_with_timeout(
            &[id.to_owned()],
            Duration::from_millis(50),
            &CancellationToken::default(),
        );
        assert!(matches!(
            result,
            Err(AdmissionReconciliationError::Blocked("held_ticket"))
        ));
        assert_eq!(before, tree_bytes(c.root()));
    }

    #[test]
    fn reconciliation_apply_rejects_duplicate_and_malformed_targets_without_mutation() {
        let c = coordinator("reconcile-invalid-target");
        c.initialize().expect("initialize");
        let id = "00000000000000000012";
        fixture_ticket(&c, id, valid_marker(id));
        fs::write(c.root().join(NEXT_TICKET), b"1\n").expect("counter");
        write_lease_fixture(&c, id, "active", 1, 1);
        let before = tree_bytes(c.root());
        for ids in [vec![id.to_owned(), id.to_owned()], vec!["bad".to_owned()]] {
            let result = c.reconcile_apply_with_timeout(
                &ids,
                Duration::from_secs(1),
                &CancellationToken::default(),
            );
            assert!(matches!(
                result,
                Err(AdmissionReconciliationError::Blocked(_))
            ));
            assert_eq!(before, tree_bytes(c.root()));
        }
    }

    #[test]
    fn reconciliation_apply_quarantines_selected_ticket_with_absent_lease() {
        let c = coordinator("reconcile-absent-lease-apply");
        c.initialize().expect("initialize");
        let id = "00000000000000000013";
        fixture_ticket(&c, id, valid_marker(id));
        fs::write(c.root().join(NEXT_TICKET), b"1\n").expect("counter");
        let result = c.reconcile_apply_with_timeout(
            &[id.to_owned()],
            Duration::from_secs(1),
            &CancellationToken::default(),
        );
        assert_eq!(
            result.expect("apply").outcomes,
            vec![outcome(id, "quarantined")]
        );
        assert!(
            !c.root()
                .join(TICKETS_DIR)
                .join(format!("{TICKET_PREFIX}{id}{TICKET_SUFFIX}"))
                .exists()
        );
    }

    #[test]
    fn reconciliation_apply_returns_bounded_unknown_target_and_preserves_state() {
        let c = coordinator("reconcile-lock-release");
        c.initialize().expect("initialize");
        let first = "00000000000000000014";
        fixture_ticket(&c, first, valid_marker(first));
        fs::write(c.root().join(NEXT_TICKET), b"1\n").expect("counter");
        write_lease_fixture(&c, first, "active", 1, 1);
        let missing = "00000000000000000015".to_owned();
        let before = tree_bytes(c.root());
        assert_eq!(c.reconciliation_cleanup_runs(), 0);
        let result = c.reconcile_apply_with_timeout(
            &[first.to_owned(), missing],
            Duration::from_secs(1),
            &CancellationToken::default(),
        );
        assert!(matches!(
            result,
            Err(AdmissionReconciliationError::Blocked("unknown_target"))
        ));
        assert_eq!(before, tree_bytes(c.root()));
        assert_eq!(c.reconciliation_cleanup_runs(), 1);
        let follow_up = c
            .reconcile_apply_with_timeout(
                &[first.to_owned()],
                Duration::from_secs(1),
                &CancellationToken::default(),
            )
            .expect("follow-up apply");
        assert_eq!(follow_up.outcomes, vec![outcome(first, "quarantined")]);
    }

    #[test]
    fn reconciliation_apply_blocks_lease_only_residue_and_preserves_state() {
        let c = coordinator("reconcile-lease-only-review");
        c.initialize().expect("initialize");
        let selected = "00000000000000000077";
        let lease_only = "00000000000000000078";
        expired_ticket(&c, selected);
        write_lease_fixture(&c, lease_only, "active", 1, 1);
        let before = tree_bytes(c.root());

        let result = c.reconcile_apply_with_timeout(
            &[selected.to_owned()],
            Duration::from_secs(1),
            &CancellationToken::default(),
        );

        assert!(matches!(
            result,
            Err(AdmissionReconciliationError::Blocked("lease_only_residue"))
        ));
        assert_eq!(before, tree_bytes(c.root()));
    }

    #[test]
    fn reconciliation_apply_blocks_unselected_live_or_future_lease_and_preserves_state() {
        let now = unix_seconds().expect("clock");
        for (ordinal, acquired, heartbeat) in [(0, now, now), (1, 4_000_000_000, 4_000_000_001)] {
            let c = coordinator(&format!("reconcile-unselected-live-review-{ordinal}"));
            c.initialize().expect("initialize");
            let selected = format!("{:020}", 79 + ordinal * 2);
            let unselected = format!("{:020}", 80 + ordinal * 2);
            expired_ticket(&c, &selected);
            fixture_ticket(&c, &unselected, valid_marker(&unselected));
            write_lease_fixture(&c, &unselected, "active", acquired, heartbeat);
            let before = tree_bytes(c.root());

            let result = c.reconcile_apply_with_timeout(
                &[selected],
                Duration::from_secs(1),
                &CancellationToken::default(),
            );

            assert!(matches!(
                result,
                Err(AdmissionReconciliationError::Blocked(
                    "slot_lease_contradiction"
                ))
            ));
            assert_eq!(before, tree_bytes(c.root()));
        }
    }

    #[test]
    fn reconciliation_apply_blocks_foreign_or_malformed_lease_and_preserves_state() {
        for ordinal in 0..3 {
            let c = coordinator(&format!("reconcile-invalid-global-lease-review-{ordinal}"));
            c.initialize().expect("initialize");
            let selected = format!("{:020}", 83 + ordinal * 2);
            let unselected = format!("{:020}", 84 + ordinal * 2);
            expired_ticket(&c, &selected);
            fixture_ticket(&c, &unselected, valid_marker(&unselected));
            let bytes = if ordinal == 0 {
                let lease = LeaseMarker {
                    owner: "foreign".to_owned(),
                    purpose: "host-admission-lease".to_owned(),
                    schema_version: ADMISSION_SCHEMA_VERSION.to_owned(),
                    owner_run_id: unselected.clone(),
                    acquired_at_unix_seconds: 1,
                    heartbeat_at_unix_seconds: 1,
                    state: "active".to_owned(),
                };
                serde_json::to_vec(&lease).expect("foreign lease")
            } else {
                b"{".to_vec()
            };
            let lease_path = if ordinal == 2 {
                c.root().join(LEASES_DIR).join("rogue-lease-name")
            } else {
                c.lease_path(&unselected)
            };
            fs::write(lease_path, bytes).expect("invalid lease fixture");
            let before = tree_bytes(c.root());

            let result = c.reconcile_apply_with_timeout(
                &[selected],
                Duration::from_secs(1),
                &CancellationToken::default(),
            );

            assert!(matches!(
                result,
                Err(AdmissionReconciliationError::Blocked(
                    "foreign_or_malformed_lease"
                ))
            ));
            assert_eq!(before, tree_bytes(c.root()));
        }
    }

    #[test]
    fn reconciliation_identity_io_error_uses_cleanup_owner_and_leaves_locks_reusable() {
        let c = coordinator("reconcile-identity-io-cleanup-round6");
        c.initialize().expect("initialize");
        let first = "00000000000000000069";
        let second = "00000000000000000070";
        expired_ticket(&c, first);
        expired_ticket(&c, second);
        let gate = Arc::new(ReconciliationTestGate::new());
        c.set_reconciliation_gate(
            ReconciliationTestGatePoint::AfterSelectedDescriptors,
            gate.clone(),
        );
        assert_eq!(c.reconciliation_cleanup_runs(), 0);

        let worker_c = c.clone();
        let worker = thread::spawn(move || {
            worker_c.reconcile_apply_with_timeout(
                &[second.to_owned(), first.to_owned()],
                Duration::from_secs(2),
                &CancellationToken::default(),
            )
        });
        gate.wait_until_reached();
        let tickets = c.root().join(TICKETS_DIR);
        let retained_tickets = c.root().join("tickets-retained-round6");
        fs::rename(&tickets, &retained_tickets).expect("retain tickets directory");
        fs::write(&tickets, b"force ENOTDIR during identity recheck")
            .expect("replace tickets directory with file");
        gate.release();

        assert!(matches!(
            worker.join().expect("reconciler"),
            Err(AdmissionReconciliationError::Admission(
                AdmissionError::Io { .. }
            ))
        ));
        assert_eq!(c.reconciliation_cleanup_runs(), 1);
        fs::remove_file(&tickets).expect("remove identity I/O fixture");
        fs::rename(&retained_tickets, &tickets).expect("restore tickets directory");

        let report = c
            .reconcile_apply_with_timeout(
                &[second.to_owned(), first.to_owned()],
                Duration::from_secs(1),
                &CancellationToken::default(),
            )
            .expect("locks reusable after identity I/O error");
        assert_eq!(
            report.outcomes,
            vec![
                outcome(first, "quarantined"),
                outcome(second, "quarantined")
            ]
        );
    }

    #[test]
    fn reconciliation_apply_rejects_selected_symlink_without_mutation() {
        let c = coordinator("reconcile-selected-symlink");
        c.initialize().expect("initialize");
        let id = "00000000000000000016";
        let path = fixture_ticket(&c, id, valid_marker(id));
        fs::write(c.root().join(NEXT_TICKET), b"1\n").expect("counter");
        write_lease_fixture(&c, id, "active", 1, 1);
        let lease_before = fs::read(c.lease_path(id)).expect("lease");
        fs::remove_file(&path).expect("remove ticket");
        std::os::unix::fs::symlink(c.root().join(OWNER_FILE), &path).expect("symlink");
        let result = c.reconcile_apply_with_timeout(
            &[id.to_owned()],
            Duration::from_secs(1),
            &CancellationToken::default(),
        );
        assert!(matches!(
            result,
            Err(AdmissionReconciliationError::Blocked(
                "unsafe_selected_ticket"
            ))
        ));
        assert!(path.is_symlink());
        assert_eq!(lease_before, fs::read(c.lease_path(id)).expect("lease"));
    }

    fn wait_for_ticket_count(root: &Path, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let count = fs::read_dir(root.join(TICKETS_DIR))
                .expect("read tickets")
                .count();
            if count == expected {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for tickets");
            thread::sleep(Duration::from_millis(10));
        }
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
    fn initialized_root_waits_for_queue_owned_durable_temporary_before_validation() {
        let coordinator = coordinator("durable-layout-race");
        coordinator.initialize().expect("initialize coordinator");

        let mut queue = coordinator.open_queue(false).expect("open queue lock");
        queue.lock_exclusive().expect("hold queue lock");
        let temporary = coordinator
            .root()
            .join(format!(".ccp-durable-tmp-{}-test", std::process::id()));
        fs::write(&temporary, b"owned staging\n").expect("temporary durable file");

        assert!(
            matches!(
                coordinator
                    .status_with_timeout(Duration::from_millis(60), &CancellationToken::default(),),
                Err(AdmissionError::Timeout)
            ),
            "status must wait for the queue-owned durable write"
        );
        fs::remove_file(&temporary).expect("remove temporary durable file");
        unlock(&mut queue).expect("release queue lock");
        assert!(!coordinator.status().expect("status").active);

        queue.lock_exclusive().expect("hold queue lock again");
        fs::write(&temporary, b"owned staging\n").expect("temporary durable file");
        assert!(
            matches!(
                coordinator.acquire(Duration::from_millis(60), &CancellationToken::default(),),
                Err(AdmissionError::Timeout)
            ),
            "acquire must wait for the queue-owned durable write"
        );
        fs::remove_file(&temporary).expect("remove temporary durable file");
        unlock(&mut queue).expect("release queue lock");
        let guard = coordinator
            .acquire(Duration::from_secs(2), &CancellationToken::default())
            .expect("acquire");
        drop(guard);
        fs::remove_dir_all(coordinator.root()).expect("remove test coordinator");
    }

    #[test]
    fn unlocked_durable_temporary_remains_fail_closed() {
        let coordinator = coordinator("unlocked-durable-temporary");
        coordinator.initialize().expect("initialize coordinator");
        let temporary = coordinator
            .root()
            .join(format!(".ccp-durable-tmp-{}-foreign", std::process::id()));
        fs::write(&temporary, b"untrusted staging\n").expect("foreign temporary file");

        assert!(matches!(
            coordinator.status(),
            Err(AdmissionError::UnsafeLayout(path)) if path == temporary
        ));
        assert!(matches!(
            coordinator.acquire(Duration::from_secs(1), &CancellationToken::default()),
            Err(AdmissionError::UnsafeLayout(path)) if path == temporary
        ));
        assert!(
            temporary.exists(),
            "admission must not remove unknown state"
        );
        fs::remove_dir_all(coordinator.root()).expect("remove test coordinator");
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
        wait_for_ticket_count(coordinator.root(), 2);
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
