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
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::resource::{MACOS_POLICY_VERSION, ResourceObservationSummary, WatchdogTripReason};

pub const RESOURCE_HISTORY_SCHEMA_VERSION: &str = "1.0";
pub const DEFAULT_RESOURCE_HISTORY_RECORDS: usize = 100;
pub const DEFAULT_RESOURCE_PROFILE: &str = "guard-exec";
pub const RESOURCE_HISTORY_DIR_ENV: &str = "CCP_RESOURCE_HISTORY_DIR";

const PLATFORM_DIRECTORY: &str = "commit-ci-preflight";
const HISTORY_FILE: &str = "resource-history-v1.jsonl";
const MAX_HISTORY_BYTES: u64 = 1024 * 1024;
const MAX_PROFILE_BYTES: usize = 64;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceRunOutcome {
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    ResourcePressure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceTripReasonV1 {
    HardPressure,
    SoftPressure,
    ProbeFailure,
}

impl From<WatchdogTripReason> for ResourceTripReasonV1 {
    fn from(value: WatchdogTripReason) -> Self {
        match value {
            WatchdogTripReason::HardPressure => Self::HardPressure,
            WatchdogTripReason::SoftPressure => Self::SoftPressure,
            WatchdogTripReason::ProbeFailure => Self::ProbeFailure,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceHistoryRecordV1 {
    pub schema_version: String,
    pub policy_version: String,
    pub platform: String,
    pub profile: String,
    pub started_at_unix_seconds: u64,
    pub duration_milliseconds: u64,
    pub outcome: ResourceRunOutcome,
    pub watchdog_trip_reason: Option<ResourceTripReasonV1>,
    pub sample_count: u64,
    pub baseline_available_percent: u8,
    pub minimum_available_percent: u8,
    pub baseline_reclaimable_uncompressed_bytes: u64,
    pub minimum_reclaimable_uncompressed_bytes: u64,
    pub baseline_compressor_occupied_bytes: u64,
    pub maximum_compressor_occupied_bytes: u64,
    pub baseline_swap_used_bytes: u64,
    pub maximum_swap_used_bytes: u64,
    pub total_memory_bytes: u64,
}

impl ResourceHistoryRecordV1 {
    pub fn from_summary(
        profile: &str,
        started_at_unix_seconds: u64,
        duration_milliseconds: u64,
        outcome: ResourceRunOutcome,
        watchdog_trip_reason: Option<WatchdogTripReason>,
        summary: &ResourceObservationSummary,
    ) -> Result<Self, ResourceHistoryError> {
        validate_profile(profile)?;
        Ok(Self {
            schema_version: RESOURCE_HISTORY_SCHEMA_VERSION.to_owned(),
            policy_version: MACOS_POLICY_VERSION.to_owned(),
            platform: "macos".to_owned(),
            profile: profile.to_owned(),
            started_at_unix_seconds,
            duration_milliseconds,
            outcome,
            watchdog_trip_reason: watchdog_trip_reason.map(Into::into),
            sample_count: summary.sample_count,
            baseline_available_percent: summary.baseline.available_percent,
            minimum_available_percent: summary.minimum_available_percent,
            baseline_reclaimable_uncompressed_bytes: summary
                .baseline
                .reclaimable_uncompressed_bytes,
            minimum_reclaimable_uncompressed_bytes: summary.minimum_reclaimable_uncompressed_bytes,
            baseline_compressor_occupied_bytes: summary.baseline.compressor_occupied_bytes,
            maximum_compressor_occupied_bytes: summary.maximum_compressor_occupied_bytes,
            baseline_swap_used_bytes: summary.baseline.swap_used_bytes,
            maximum_swap_used_bytes: summary.maximum_swap_used_bytes,
            total_memory_bytes: summary.baseline.total_memory_bytes,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ResourceHistoryStore {
    root: PathBuf,
}

impl ResourceHistoryStore {
    pub fn platform() -> Result<Self, ResourceHistoryError> {
        let root = std::env::var_os(RESOURCE_HISTORY_DIR_ENV)
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(platform_root)?;
        Self::at(root)
    }

    pub fn at(root: PathBuf) -> Result<Self, ResourceHistoryError> {
        validate_root_candidate(&root)?;
        Ok(Self { root })
    }

    pub fn append(&self, record: &ResourceHistoryRecordV1) -> Result<(), ResourceHistoryError> {
        self.append_with_limit(record, DEFAULT_RESOURCE_HISTORY_RECORDS)
    }

    fn append_with_limit(
        &self,
        record: &ResourceHistoryRecordV1,
        limit: usize,
    ) -> Result<(), ResourceHistoryError> {
        if limit == 0 {
            return Err(ResourceHistoryError::InvalidLimit);
        }
        ensure_directory(&self.root)?;
        let target = self.root.join(HISTORY_FILE);
        reject_symlink_file(&target)?;
        let mut records = read_records(&target)?;
        let keep = limit.saturating_sub(1);
        if records.len() > keep {
            records.drain(..records.len() - keep);
        }
        records.push(record.clone());
        write_records_atomic(&target, &records)
    }
}

pub fn validate_profile(profile: &str) -> Result<(), ResourceHistoryError> {
    let bytes = profile.as_bytes();
    if bytes.is_empty()
        || bytes.len() > MAX_PROFILE_BYTES
        || !bytes[0].is_ascii_alphanumeric()
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ResourceHistoryError::InvalidProfile);
    }
    Ok(())
}

fn platform_root() -> Result<PathBuf, ResourceHistoryError> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| {
                home.join("Library")
                    .join("Application Support")
                    .join(PLATFORM_DIRECTORY)
            })
            .ok_or(ResourceHistoryError::NoPersistentDefault)
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|base| base.join(PLATFORM_DIRECTORY))
            .ok_or(ResourceHistoryError::NoPersistentDefault)
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join(".local").join("state"))
            })
            .map(|base| base.join(PLATFORM_DIRECTORY))
            .ok_or(ResourceHistoryError::NoPersistentDefault)
    }
}

fn validate_root_candidate(path: &Path) -> Result<(), ResourceHistoryError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(ResourceHistoryError::UnsafePath);
    }
    reject_symlink_components(path)
}

fn ensure_directory(path: &Path) -> Result<(), ResourceHistoryError> {
    fs::create_dir_all(path).map_err(ResourceHistoryError::Io)?;
    reject_symlink_components(path)?;
    let metadata = fs::symlink_metadata(path).map_err(ResourceHistoryError::Io)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ResourceHistoryError::UnsafePath);
    }
    Ok(())
}

fn reject_symlink_components(path: &Path) -> Result<(), ResourceHistoryError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ResourceHistoryError::UnsafePath);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(ResourceHistoryError::Io(error)),
        }
    }
    Ok(())
}

fn reject_symlink_file(path: &Path) -> Result<(), ResourceHistoryError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(ResourceHistoryError::UnsafePath)
        }
        Ok(metadata) if metadata.len() > MAX_HISTORY_BYTES => {
            Err(ResourceHistoryError::HistoryTooLarge)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ResourceHistoryError::Io(error)),
    }
}

fn read_records(path: &Path) -> Result<Vec<ResourceHistoryRecordV1>, ResourceHistoryError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(ResourceHistoryError::Io(error)),
    };
    let text = std::str::from_utf8(&bytes).map_err(|_| ResourceHistoryError::InvalidHistory)?;
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).map_err(|_| ResourceHistoryError::InvalidHistory))
        .collect()
}

fn write_records_atomic(
    target: &Path,
    records: &[ResourceHistoryRecordV1],
) -> Result<(), ResourceHistoryError> {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = target.with_file_name(format!(
        ".{HISTORY_FILE}.tmp-{}-{sequence}",
        std::process::id()
    ));
    reject_symlink_file(&temporary)?;
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(ResourceHistoryError::Io)?;
        for record in records {
            serde_json::to_writer(&mut file, record).map_err(ResourceHistoryError::Serialize)?;
            file.write_all(b"\n").map_err(ResourceHistoryError::Io)?;
        }
        file.sync_all().map_err(ResourceHistoryError::Io)?;
        fs::rename(&temporary, target).map_err(ResourceHistoryError::Io)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[derive(Debug)]
pub enum ResourceHistoryError {
    NoPersistentDefault,
    UnsafePath,
    InvalidProfile,
    InvalidLimit,
    InvalidHistory,
    HistoryTooLarge,
    Io(io::Error),
    Serialize(serde_json::Error),
}

impl fmt::Display for ResourceHistoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPersistentDefault => {
                formatter.write_str("no persistent history root is available")
            }
            Self::UnsafePath => formatter.write_str("resource history path is unsafe"),
            Self::InvalidProfile => formatter.write_str("resource profile is invalid"),
            Self::InvalidLimit => formatter.write_str("resource history limit is invalid"),
            Self::InvalidHistory => formatter.write_str("resource history is invalid"),
            Self::HistoryTooLarge => formatter.write_str("resource history exceeds its size limit"),
            Self::Io(_) => formatter.write_str("resource history filesystem operation failed"),
            Self::Serialize(_) => formatter.write_str("resource history serialization failed"),
        }
    }
}

impl std::error::Error for ResourceHistoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Serialize(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::{ResourceObservation, ResourceSnapshot};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_root(name: &str) -> PathBuf {
        fs::canonicalize(std::env::temp_dir())
            .expect("canonical temporary root")
            .join(format!(
                "ccp-resource-history-{name}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("clock after epoch")
                    .as_nanos()
            ))
    }

    fn snapshot(compressor: u64) -> ResourceSnapshot {
        ResourceSnapshot {
            available_percent: 60,
            reclaimable_uncompressed_bytes: 12_000,
            compressor_occupied_bytes: compressor,
            total_memory_bytes: 40_000,
            swap_used_bytes: 1_000,
            swap_total_bytes: 4_000,
        }
    }

    fn record(profile: &str, started: u64) -> ResourceHistoryRecordV1 {
        let observation = ResourceObservation::new(snapshot(10_000));
        observation.record(&snapshot(11_000));
        ResourceHistoryRecordV1::from_summary(
            profile,
            started,
            500,
            ResourceRunOutcome::Completed,
            None,
            &observation.summary().expect("summary"),
        )
        .expect("record")
    }

    #[test]
    fn profile_is_bounded_and_machine_safe() {
        for valid in ["guard-exec", "matryca_ready", "A1"] {
            validate_profile(valid).expect("valid profile");
        }
        for invalid in ["", " space", "repo/path", "contains.dot"] {
            assert!(matches!(
                validate_profile(invalid),
                Err(ResourceHistoryError::InvalidProfile)
            ));
        }
        assert!(validate_profile(&"a".repeat(MAX_PROFILE_BYTES + 1)).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn append_rotates_oldest_records_and_is_deterministic_jsonl() {
        let root = fixture_root("rotation");
        let store = ResourceHistoryStore::at(root.clone()).expect("store");
        for started in 1..=4 {
            store
                .append_with_limit(&record("ready", started), 3)
                .expect("append");
        }
        let text = fs::read_to_string(root.join(HISTORY_FILE)).expect("history");
        let records = text
            .lines()
            .map(|line| serde_json::from_str::<ResourceHistoryRecordV1>(line).expect("json"))
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].started_at_unix_seconds, 2);
        assert_eq!(records[2].started_at_unix_seconds, 4);
        assert!(!text.contains("command"));
        assert!(!text.contains("repository"));
        assert!(!text.contains("path"));
        fs::remove_dir_all(root).expect("cleanup owned fixture");
    }

    #[test]
    #[cfg(unix)]
    fn malformed_history_is_preserved_and_never_repaired_implicitly() {
        let root = fixture_root("malformed");
        fs::create_dir_all(&root).expect("root");
        let history = root.join(HISTORY_FILE);
        fs::write(&history, b"{not-json}\n").expect("malformed fixture");
        let store = ResourceHistoryStore::at(root.clone()).expect("store");
        assert!(matches!(
            store.append(&record("ready", 1)),
            Err(ResourceHistoryError::InvalidHistory)
        ));
        assert_eq!(
            fs::read(&history).expect("preserved history"),
            b"{not-json}\n"
        );
        fs::remove_dir_all(root).expect("cleanup root");
    }

    #[cfg(unix)]
    #[test]
    fn history_never_follows_symlinks() {
        use std::os::unix::fs::symlink;

        let root = fixture_root("symlink");
        let outside = fixture_root("outside");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(&outside).expect("outside");
        symlink(outside.join("history.jsonl"), root.join(HISTORY_FILE)).expect("symlink");
        let store = ResourceHistoryStore::at(root.clone()).expect("store");
        assert!(matches!(
            store.append(&record("ready", 1)),
            Err(ResourceHistoryError::UnsafePath)
        ));
        fs::remove_dir_all(root).expect("cleanup root");
        fs::remove_dir_all(outside).expect("cleanup outside");
    }
}
