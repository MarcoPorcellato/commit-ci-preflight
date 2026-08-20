// Copyright 2026 Marco Porcellato
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::durable_fs::{DurableFileSystem, DurableFsError};
use crate::receipt::canonical_digest;

pub const RUN_JOURNAL_SCHEMA_VERSION: &str = "1.0";
const JOURNAL_DIR: &str = "run-journal-v1";
const RUNS_DIR: &str = "runs";
const ROOT_MARKER: &str = ".ccp-run-journal-root-v1.json";
const RUN_MARKER: &str = ".ccp-run-owner-v1.json";
const RESOURCES_DIR: &str = "resources";
const SOURCE_BINDING: &str = "source-snapshot-v1.json";
const QUARANTINE_PREFIX: &str = "quarantined-";
static TOKEN_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootOwnerMarkerV1 {
    schema_version: String,
    owner: String,
    purpose: String,
    root_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunOwnerMarkerV1 {
    schema_version: String,
    owner: String,
    purpose: String,
    root_token: String,
    run_id: String,
    run_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunJournalSourceV1 {
    pub schema_version: String,
    pub commit_sha: String,
    pub manifest_digest: String,
    pub entry_count: u64,
    pub resource_id: String,
}

#[derive(Serialize)]
struct OwnershipTokenSeed<'a> {
    schema_version: &'static str,
    purpose: &'static str,
    parent_token: Option<&'a str>,
    run_id: Option<&'a str>,
    unix_nanos: u128,
    process_id: u32,
    sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunJournalStateV1 {
    Created,
    Admitted,
    Prepared,
    Executing,
    Finalizing,
    CleanupPending,
    Failed,
    Sealed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunFailureKindV1 {
    AdmissionRejected,
    PreparationFailed,
    ExecutionFailed,
    FinalizationFailed,
    CleanupFailed,
    ResourcePressure,
    StaleCommit,
    Invariant,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunJournalEntryV1 {
    pub schema_version: String,
    pub run_id: String,
    pub seq: u64,
    pub state: RunJournalStateV1,
    pub at_utc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<RunFailureKindV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryClassificationV1 {
    Restartable,
    CleanupRequired,
    Terminal,
    Quarantined,
    OperatorRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RecoveryRunStatusV1 {
    pub run_id: String,
    pub state: Option<RunJournalStateV1>,
    pub classification: RecoveryClassificationV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RecoveryStatusV1 {
    pub schema_version: String,
    pub runs: Vec<RecoveryRunStatusV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RecoveryApplyV1 {
    pub schema_version: String,
    pub run_id: String,
    pub outcome: RecoveryClassificationV1,
}

pub fn run_journal_entry_schema_json() -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&schemars::schema_for!(RunJournalEntryV1)).map(|mut schema| {
        schema.push('\n');
        schema
    })
}

#[derive(Debug)]
pub enum RunJournalError {
    Io(io::Error),
    Durable(DurableFsError),
    InvalidRunId,
    InvalidTimestamp,
    InvalidTransition,
    Corrupt,
    OwnershipMismatch,
    NonActionable,
}

impl RunJournalError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::InvalidRunId
            | Self::InvalidTimestamp
            | Self::InvalidTransition
            | Self::OwnershipMismatch => 2,
            Self::NonActionable => 5,
            Self::Io(_) | Self::Durable(_) | Self::Corrupt => 70,
        }
    }
}

impl fmt::Display for RunJournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "run journal I/O failed: {error}"),
            Self::Durable(error) => write!(formatter, "run journal persistence failed: {error}"),
            Self::InvalidRunId => formatter.write_str("invalid run id"),
            Self::InvalidTimestamp => formatter.write_str("invalid journal timestamp"),
            Self::InvalidTransition => formatter.write_str("invalid run journal transition"),
            Self::Corrupt => formatter.write_str("run journal is corrupt or incomplete"),
            Self::OwnershipMismatch => formatter.write_str("run journal ownership mismatch"),
            Self::NonActionable => formatter.write_str("run journal state is not actionable"),
        }
    }
}

impl std::error::Error for RunJournalError {}

impl From<io::Error> for RunJournalError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<DurableFsError> for RunJournalError {
    fn from(value: DurableFsError) -> Self {
        Self::Durable(value)
    }
}

#[derive(Debug, Clone)]
pub struct RunJournalStore {
    root: PathBuf,
    root_token: String,
    durable: DurableFileSystem,
}

impl RunJournalStore {
    pub fn initialize(cache_root: &Path) -> Result<Self, RunJournalError> {
        let root = cache_root.join(JOURNAL_DIR);
        let durable = DurableFileSystem::default();
        durable.create_directory(&root)?;
        let marker = root.join(ROOT_MARKER);
        let root_token = if marker.exists() {
            read_root_marker(&marker)?.root_token
        } else {
            ensure_unowned_root_is_empty(&root)?;
            let root_token = new_ownership_token("run-journal-root", None, None)?;
            durable.create_new(&marker, &root_marker_bytes(&root_token)?)?;
            root_token
        };
        durable.create_directory(&root.join(RUNS_DIR))?;
        Ok(Self {
            root,
            root_token,
            durable,
        })
    }

    pub fn open(cache_root: &Path) -> Result<Option<Self>, RunJournalError> {
        let root = cache_root.join(JOURNAL_DIR);
        match fs::symlink_metadata(&root) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(RunJournalError::Io(error)),
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(RunJournalError::OwnershipMismatch);
            }
            Ok(_) => {}
        }
        let root_token = read_root_marker(&root.join(ROOT_MARKER))?.root_token;
        validate_existing_plain_directory(&root.join(RUNS_DIR))?;
        Ok(Some(Self {
            root,
            root_token,
            durable: DurableFileSystem::default(),
        }))
    }

    pub fn create_run(
        &self,
        run_id: &str,
        at_utc: &str,
    ) -> Result<RunJournalEntryV1, RunJournalError> {
        validate_run_id(run_id)?;
        validate_timestamp(at_utc)?;
        let run = self.run_path(run_id);
        self.durable.create_new_directory(&run)?;
        self.durable.create_new(
            &run.join(RUN_MARKER),
            &run_marker_bytes(&self.root_token, run_id)?,
        )?;
        self.append_entry(run_id, 0, RunJournalStateV1::Created, at_utc, None)
    }

    pub fn transition(
        &self,
        run_id: &str,
        state: RunJournalStateV1,
        at_utc: &str,
        failure_kind: Option<RunFailureKindV1>,
    ) -> Result<RunJournalEntryV1, RunJournalError> {
        validate_run_id(run_id)?;
        validate_timestamp(at_utc)?;
        let entries = self.read_entries(run_id)?;
        let previous = entries.last().ok_or(RunJournalError::Corrupt)?;
        if !legal_transition(previous.state, state) {
            return Err(RunJournalError::InvalidTransition);
        }
        if (state == RunJournalStateV1::Failed) != failure_kind.is_some() {
            return Err(RunJournalError::InvalidTransition);
        }
        self.append_entry(run_id, previous.seq + 1, state, at_utc, failure_kind)
    }

    /// Reserve one exact CCP-owned location for ephemeral run resources.
    /// The directory remains inside the owned run tree so `recover apply`
    /// quarantines it together with an interrupted journal.
    pub fn reserve_resource(
        &self,
        run_id: &str,
        resource_id: &str,
    ) -> Result<PathBuf, RunJournalError> {
        validate_run_id(run_id)?;
        validate_resource_id(resource_id)?;
        let run = self.run_path(run_id);
        self.validate_run_marker(&run, run_id)?;
        let resources = run.join(RESOURCES_DIR);
        match self.durable.create_new_directory(&resources) {
            Ok(()) => {}
            Err(DurableFsError::Io(error)) if error.kind() == io::ErrorKind::AlreadyExists => {
                validate_existing_plain_directory(&resources)?;
            }
            Err(error) => return Err(RunJournalError::Durable(error)),
        }
        let target = resources.join(resource_id);
        if fs::symlink_metadata(&target).is_ok() {
            return Err(RunJournalError::OwnershipMismatch);
        }
        Ok(target)
    }

    pub fn bind_source(
        &self,
        run_id: &str,
        commit_sha: &str,
        manifest_digest: &str,
        entry_count: u64,
    ) -> Result<RunJournalSourceV1, RunJournalError> {
        validate_run_id(run_id)?;
        validate_hex_identity(commit_sha)?;
        validate_digest(manifest_digest)?;
        if entry_count == 0 {
            return Err(RunJournalError::Corrupt);
        }
        let run = self.run_path(run_id);
        self.validate_run_marker(&run, run_id)?;
        let binding = RunJournalSourceV1 {
            schema_version: RUN_JOURNAL_SCHEMA_VERSION.to_owned(),
            commit_sha: commit_sha.to_owned(),
            manifest_digest: manifest_digest.to_owned(),
            entry_count,
            resource_id: "source-snapshot-v1".to_owned(),
        };
        let mut bytes = serde_json::to_vec(&binding).map_err(|_| RunJournalError::Corrupt)?;
        bytes.push(b'\n');
        self.durable.create_new(&run.join(SOURCE_BINDING), &bytes)?;
        Ok(binding)
    }

    pub fn status(&self) -> Result<RecoveryStatusV1, RunJournalError> {
        let mut runs = Vec::new();
        let mut entries: Vec<_> =
            fs::read_dir(self.root.join(RUNS_DIR))?.collect::<Result<_, _>>()?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let name = match entry.file_name().into_string() {
                Ok(name) => name,
                Err(_) => {
                    runs.push(operator_required("unknown"));
                    continue;
                }
            };
            if let Some(run_id) = name.strip_prefix(QUARANTINE_PREFIX) {
                let classification = if validate_existing_plain_directory(&entry.path()).is_ok()
                    && validate_run_id(run_id).is_ok()
                    && self.validate_run_marker(&entry.path(), run_id).is_ok()
                {
                    RecoveryClassificationV1::Quarantined
                } else {
                    RecoveryClassificationV1::OperatorRequired
                };
                runs.push(RecoveryRunStatusV1 {
                    run_id: bounded_run_id(run_id),
                    state: None,
                    classification,
                });
                continue;
            }
            if validate_run_id(&name).is_err() {
                runs.push(operator_required("unknown"));
                continue;
            }
            match self.read_entries(&name) {
                Ok(journal) if !journal.is_empty() => {
                    let last = journal.last().expect("non-empty journal");
                    runs.push(RecoveryRunStatusV1 {
                        run_id: name,
                        state: Some(last.state),
                        classification: classify(last.state),
                    });
                }
                Ok(_) | Err(_) => runs.push(operator_required(&name)),
            }
        }
        Ok(RecoveryStatusV1 {
            schema_version: RUN_JOURNAL_SCHEMA_VERSION.to_owned(),
            runs,
        })
    }

    pub fn apply(&self, run_id: &str) -> Result<RecoveryApplyV1, RunJournalError> {
        validate_run_id(run_id)?;
        let source = self.run_path(run_id);
        let quarantine = self
            .root
            .join(RUNS_DIR)
            .join(format!("{QUARANTINE_PREFIX}{run_id}"));
        if quarantine.exists() {
            if source.exists() {
                return Err(RunJournalError::Corrupt);
            }
            self.validate_run_marker(&quarantine, run_id)?;
            return Ok(quarantined_result(run_id));
        }
        let entries = self.read_entries(run_id)?;
        let last = entries.last().ok_or(RunJournalError::Corrupt)?;
        if classify(last.state) == RecoveryClassificationV1::Terminal {
            return Err(RunJournalError::NonActionable);
        }
        let marker = run_marker_bytes(&self.root_token, run_id)?;
        self.durable
            .quarantine_owned_tree(&source, &quarantine, RUN_MARKER, &marker)?;
        Ok(quarantined_result(run_id))
    }

    fn append_entry(
        &self,
        run_id: &str,
        seq: u64,
        state: RunJournalStateV1,
        at_utc: &str,
        failure_kind: Option<RunFailureKindV1>,
    ) -> Result<RunJournalEntryV1, RunJournalError> {
        let entry = RunJournalEntryV1 {
            schema_version: RUN_JOURNAL_SCHEMA_VERSION.to_owned(),
            run_id: run_id.to_owned(),
            seq,
            state,
            at_utc: at_utc.to_owned(),
            failure_kind,
        };
        let mut bytes = serde_json::to_vec(&entry).map_err(|_| RunJournalError::Corrupt)?;
        bytes.push(b'\n');
        let filename = format!("{seq:020}-{}.json", state_name(state));
        self.durable
            .create_new(&self.run_path(run_id).join(filename), &bytes)?;
        Ok(entry)
    }

    fn read_entries(&self, run_id: &str) -> Result<Vec<RunJournalEntryV1>, RunJournalError> {
        let run = self.run_path(run_id);
        validate_existing_plain_directory(&run)?;
        self.validate_run_marker(&run, run_id)?;
        let mut paths: Vec<_> = fs::read_dir(&run)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|entry| {
                entry.file_name() != RUN_MARKER
                    && entry.file_name() != RESOURCES_DIR
                    && entry.file_name() != SOURCE_BINDING
            })
            .map(|entry| entry.path())
            .collect();
        paths.sort();
        let resources = run.join(RESOURCES_DIR);
        if resources.exists() {
            validate_existing_plain_directory(&resources)?;
        }
        let source_binding = run.join(SOURCE_BINDING);
        if source_binding.exists() {
            let binding: RunJournalSourceV1 = read_json_marker(&source_binding)?;
            if binding.schema_version != RUN_JOURNAL_SCHEMA_VERSION
                || validate_hex_identity(&binding.commit_sha).is_err()
                || validate_digest(&binding.manifest_digest).is_err()
                || binding.entry_count == 0
                || binding.resource_id != "source-snapshot-v1"
            {
                return Err(RunJournalError::Corrupt);
            }
        }
        let mut entries: Vec<RunJournalEntryV1> = Vec::with_capacity(paths.len());
        for path in paths {
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(RunJournalError::Corrupt);
            }
            let entry: RunJournalEntryV1 =
                serde_json::from_slice(&fs::read(&path)?).map_err(|_| RunJournalError::Corrupt)?;
            validate_entry(&entry, run_id, entries.len() as u64)?;
            let expected_name = format!("{:020}-{}.json", entry.seq, state_name(entry.state));
            if path.file_name().and_then(|value| value.to_str()) != Some(expected_name.as_str()) {
                return Err(RunJournalError::Corrupt);
            }
            if let Some(previous) = entries.last()
                && !legal_transition(previous.state, entry.state)
            {
                return Err(RunJournalError::Corrupt);
            }
            entries.push(entry);
        }
        Ok(entries)
    }

    fn run_path(&self, run_id: &str) -> PathBuf {
        self.root.join(RUNS_DIR).join(run_id)
    }

    fn validate_run_marker(&self, run: &Path, run_id: &str) -> Result<(), RunJournalError> {
        validate_existing_plain_directory(run)?;
        let marker: RunOwnerMarkerV1 = read_json_marker(&run.join(RUN_MARKER))?;
        if marker.schema_version != RUN_JOURNAL_SCHEMA_VERSION
            || marker.owner != "commit-ci-preflight"
            || marker.purpose != "run-journal-entry"
            || marker.root_token != self.root_token
            || marker.run_id != run_id
            || marker.run_token != derived_run_token(&self.root_token, run_id)?
        {
            return Err(RunJournalError::OwnershipMismatch);
        }
        Ok(())
    }
}

fn validate_resource_id(value: &str) -> Result<(), RunJournalError> {
    if !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        Ok(())
    } else {
        Err(RunJournalError::InvalidRunId)
    }
}

fn validate_hex_identity(value: &str) -> Result<(), RunJournalError> {
    if matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(RunJournalError::Corrupt)
    }
}

fn validate_digest(value: &str) -> Result<(), RunJournalError> {
    value
        .strip_prefix("sha256:")
        .filter(|hex| {
            hex.len() == 64
                && hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .map(|_| ())
        .ok_or(RunJournalError::Corrupt)
}

fn validate_existing_plain_directory(path: &Path) -> Result<(), RunJournalError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| RunJournalError::OwnershipMismatch)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RunJournalError::OwnershipMismatch);
    }
    Ok(())
}

fn ensure_unowned_root_is_empty(path: &Path) -> Result<(), RunJournalError> {
    if fs::read_dir(path)?.next().transpose()?.is_some() {
        return Err(RunJournalError::OwnershipMismatch);
    }
    Ok(())
}

fn read_json_marker<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, RunJournalError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| RunJournalError::OwnershipMismatch)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 4096 {
        return Err(RunJournalError::OwnershipMismatch);
    }
    serde_json::from_slice(&fs::read(path)?).map_err(|_| RunJournalError::OwnershipMismatch)
}

fn read_root_marker(path: &Path) -> Result<RootOwnerMarkerV1, RunJournalError> {
    let marker: RootOwnerMarkerV1 = read_json_marker(path)?;
    if marker.schema_version != RUN_JOURNAL_SCHEMA_VERSION
        || marker.owner != "commit-ci-preflight"
        || marker.purpose != "run-journal-root"
        || validate_token(&marker.root_token).is_err()
    {
        return Err(RunJournalError::OwnershipMismatch);
    }
    Ok(marker)
}

fn root_marker_bytes(root_token: &str) -> Result<Vec<u8>, RunJournalError> {
    marker_bytes(&RootOwnerMarkerV1 {
        schema_version: RUN_JOURNAL_SCHEMA_VERSION.to_owned(),
        owner: "commit-ci-preflight".to_owned(),
        purpose: "run-journal-root".to_owned(),
        root_token: root_token.to_owned(),
    })
}

fn run_marker_bytes(root_token: &str, run_id: &str) -> Result<Vec<u8>, RunJournalError> {
    marker_bytes(&RunOwnerMarkerV1 {
        schema_version: RUN_JOURNAL_SCHEMA_VERSION.to_owned(),
        owner: "commit-ci-preflight".to_owned(),
        purpose: "run-journal-entry".to_owned(),
        root_token: root_token.to_owned(),
        run_id: run_id.to_owned(),
        run_token: derived_run_token(root_token, run_id)?,
    })
}

fn marker_bytes(value: &impl Serialize) -> Result<Vec<u8>, RunJournalError> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| RunJournalError::Corrupt)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn new_ownership_token(
    purpose: &'static str,
    parent_token: Option<&str>,
    run_id: Option<&str>,
) -> Result<String, RunJournalError> {
    let unix_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| RunJournalError::Corrupt)?
        .as_nanos();
    token_digest(&OwnershipTokenSeed {
        schema_version: RUN_JOURNAL_SCHEMA_VERSION,
        purpose,
        parent_token,
        run_id,
        unix_nanos,
        process_id: std::process::id(),
        sequence: TOKEN_SEQUENCE.fetch_add(1, Ordering::Relaxed),
    })
}

fn derived_run_token(root_token: &str, run_id: &str) -> Result<String, RunJournalError> {
    #[derive(Serialize)]
    struct RunTokenSeed<'a> {
        schema_version: &'static str,
        purpose: &'static str,
        root_token: &'a str,
        run_id: &'a str,
    }
    token_digest(&RunTokenSeed {
        schema_version: RUN_JOURNAL_SCHEMA_VERSION,
        purpose: "run-journal-entry",
        root_token,
        run_id,
    })
}

fn token_digest(value: &impl Serialize) -> Result<String, RunJournalError> {
    canonical_digest(value)
        .map_err(|_| RunJournalError::Corrupt)?
        .strip_prefix("sha256:")
        .map(str::to_owned)
        .ok_or(RunJournalError::Corrupt)
}

fn validate_token(value: &str) -> Result<(), RunJournalError> {
    validate_run_id(value)
}

fn quarantined_result(run_id: &str) -> RecoveryApplyV1 {
    RecoveryApplyV1 {
        schema_version: RUN_JOURNAL_SCHEMA_VERSION.to_owned(),
        run_id: run_id.to_owned(),
        outcome: RecoveryClassificationV1::Quarantined,
    }
}

fn validate_entry(
    entry: &RunJournalEntryV1,
    run_id: &str,
    seq: u64,
) -> Result<(), RunJournalError> {
    if entry.schema_version != RUN_JOURNAL_SCHEMA_VERSION
        || entry.run_id != run_id
        || entry.seq != seq
        || validate_timestamp(&entry.at_utc).is_err()
        || (entry.state == RunJournalStateV1::Failed) != entry.failure_kind.is_some()
        || (seq == 0 && entry.state != RunJournalStateV1::Created)
    {
        return Err(RunJournalError::Corrupt);
    }
    Ok(())
}

fn validate_run_id(value: &str) -> Result<(), RunJournalError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(RunJournalError::InvalidRunId)
    }
}

fn validate_timestamp(value: &str) -> Result<(), RunJournalError> {
    let bytes = value.as_bytes();
    if bytes.len() == 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z'
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19) || byte.is_ascii_digit()
        })
    {
        let number = |start: usize, end: usize| {
            value[start..end]
                .parse::<u32>()
                .map_err(|_| RunJournalError::InvalidTimestamp)
        };
        let year = number(0, 4)?;
        let month = number(5, 7)?;
        let day = number(8, 10)?;
        let hour = number(11, 13)?;
        let minute = number(14, 16)?;
        let second = number(17, 19)?;
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let max_day = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return Err(RunJournalError::InvalidTimestamp),
        };
        if year != 0 && (1..=max_day).contains(&day) && hour < 24 && minute < 60 && second < 60 {
            return Ok(());
        }
    }
    Err(RunJournalError::InvalidTimestamp)
}

fn legal_transition(previous: RunJournalStateV1, next: RunJournalStateV1) -> bool {
    use RunJournalStateV1 as State;
    matches!(
        (previous, next),
        (State::Created, State::Admitted | State::Failed)
            | (
                State::Admitted,
                State::Prepared | State::Failed | State::CleanupPending
            )
            | (
                State::Prepared,
                State::Executing | State::Failed | State::CleanupPending
            )
            | (
                State::Executing,
                State::Finalizing | State::Failed | State::CleanupPending
            )
            | (
                State::Finalizing,
                State::Sealed | State::Failed | State::CleanupPending
            )
            | (State::CleanupPending, State::Failed)
    )
}

fn classify(state: RunJournalStateV1) -> RecoveryClassificationV1 {
    use RunJournalStateV1 as State;
    match state {
        State::Created | State::Admitted | State::Prepared => RecoveryClassificationV1::Restartable,
        State::Executing | State::Finalizing | State::CleanupPending => {
            RecoveryClassificationV1::CleanupRequired
        }
        State::Failed | State::Sealed => RecoveryClassificationV1::Terminal,
    }
}

fn bounded_run_id(value: &str) -> String {
    if validate_run_id(value).is_ok() {
        value.to_owned()
    } else {
        "unknown".to_owned()
    }
}

fn operator_required(run_id: &str) -> RecoveryRunStatusV1 {
    RecoveryRunStatusV1 {
        run_id: bounded_run_id(run_id),
        state: None,
        classification: RecoveryClassificationV1::OperatorRequired,
    }
}

fn state_name(state: RunJournalStateV1) -> &'static str {
    use RunJournalStateV1 as State;
    match state {
        State::Created => "created",
        State::Admitted => "admitted",
        State::Prepared => "prepared",
        State::Executing => "executing",
        State::Finalizing => "finalizing",
        State::CleanupPending => "cleanup-pending",
        State::Failed => "failed",
        State::Sealed => "sealed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const RUN_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const RUN_ID_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const AT: &str = "2026-08-14T12:00:00Z";

    fn cache_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ccp-journal-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("cache root");
        root
    }

    #[test]
    fn journal_replay_is_deterministic_and_status_is_read_only() {
        let root = cache_root("replay");
        let store = RunJournalStore::initialize(&root).expect("store");
        store.create_run(RUN_ID, AT).expect("created");
        store
            .transition(RUN_ID, RunJournalStateV1::Admitted, AT, None)
            .expect("admitted");
        let before = directory_fingerprint(&root);
        let first = store.status().expect("status");
        let second = store.status().expect("status");
        assert_eq!(first, second);
        assert_eq!(before, directory_fingerprint(&root));
        assert_eq!(
            first.runs[0].classification,
            RecoveryClassificationV1::Restartable
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn source_binding_is_private_strict_and_recovery_owned() {
        let root = cache_root("source-binding");
        let store = RunJournalStore::initialize(&root).expect("store");
        store.create_run(RUN_ID, AT).expect("run");
        let resource = store
            .reserve_resource(RUN_ID, "source-snapshot-v1")
            .expect("resource");
        let binding = store
            .bind_source(
                RUN_ID,
                "1111111111111111111111111111111111111111",
                &format!("sha256:{}", "2".repeat(64)),
                7,
            )
            .expect("binding");

        assert_eq!(binding.resource_id, "source-snapshot-v1");
        assert!(
            !resource.exists(),
            "reservation is path-free and non-mutating"
        );
        let serialized = serde_json::to_string(&binding).expect("JSON");
        assert!(!serialized.contains(root.to_string_lossy().as_ref()));
        assert_eq!(store.status().expect("status").runs.len(), 1);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn source_binding_rejects_invalid_duplicate_and_tampered_identity() {
        let root = cache_root("source-binding-invalid");
        let store = RunJournalStore::initialize(&root).expect("store");
        store.create_run(RUN_ID, AT).expect("run");
        let digest = format!("sha256:{}", "2".repeat(64));

        assert!(matches!(
            store.bind_source(RUN_ID, "not-a-commit", &digest, 1),
            Err(RunJournalError::Corrupt)
        ));
        assert!(matches!(
            store.bind_source(
                RUN_ID,
                "1111111111111111111111111111111111111111",
                "sha256:invalid",
                1,
            ),
            Err(RunJournalError::Corrupt)
        ));
        assert!(matches!(
            store.bind_source(
                RUN_ID,
                "1111111111111111111111111111111111111111",
                &digest,
                0,
            ),
            Err(RunJournalError::Corrupt)
        ));
        store
            .bind_source(
                RUN_ID,
                "1111111111111111111111111111111111111111",
                &digest,
                1,
            )
            .expect("valid binding");
        assert!(matches!(
            store.bind_source(
                RUN_ID,
                "1111111111111111111111111111111111111111",
                &digest,
                1,
            ),
            Err(RunJournalError::Durable(_))
        ));

        fs::write(
            store.run_path(RUN_ID).join(SOURCE_BINDING),
            br#"{"schema_version":"1.0","unexpected":true}"#,
        )
        .expect("tamper binding");
        let status = store.status().expect("status");
        assert_eq!(
            status.runs[0].classification,
            RecoveryClassificationV1::OperatorRequired
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn transition_machine_rejects_skips_and_post_terminal_writes() {
        let root = cache_root("transitions");
        let store = RunJournalStore::initialize(&root).expect("store");
        store.create_run(RUN_ID, AT).expect("created");
        assert!(matches!(
            store.transition(RUN_ID, RunJournalStateV1::Executing, AT, None),
            Err(RunJournalError::InvalidTransition)
        ));
        store
            .transition(
                RUN_ID,
                RunJournalStateV1::Failed,
                AT,
                Some(RunFailureKindV1::AdmissionRejected),
            )
            .expect("failed");
        assert!(matches!(
            store.transition(RUN_ID, RunJournalStateV1::Admitted, AT, None),
            Err(RunJournalError::InvalidTransition)
        ));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn initialization_never_adopts_a_foreign_nonempty_root() {
        let root = cache_root("foreign-root");
        let journal = root.join(JOURNAL_DIR);
        fs::create_dir(&journal).expect("journal root");
        fs::write(journal.join("foreign.txt"), b"foreign\n").expect("foreign state");
        assert!(matches!(
            RunJournalStore::initialize(&root),
            Err(RunJournalError::OwnershipMismatch)
        ));
        assert!(!journal.join(ROOT_MARKER).exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn impossible_timestamps_are_rejected() {
        for value in [
            "2026-13-01T00:00:00Z",
            "2026-02-30T00:00:00Z",
            "2026-01-01T24:00:00Z",
            "0000-01-01T00:00:00Z",
        ] {
            assert!(matches!(
                validate_timestamp(value),
                Err(RunJournalError::InvalidTimestamp)
            ));
        }
        assert!(validate_timestamp("2024-02-29T23:59:59Z").is_ok());
    }

    #[test]
    fn apply_quarantines_only_the_exact_owned_run() {
        let root = cache_root("apply");
        let store = RunJournalStore::initialize(&root).expect("store");
        store.create_run(RUN_ID, AT).expect("created");
        let result = store.apply(RUN_ID).expect("apply");
        assert_eq!(result.outcome, RecoveryClassificationV1::Quarantined);
        assert_eq!(
            store.status().expect("status").runs[0].classification,
            RecoveryClassificationV1::Quarantined
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn run_markers_are_bound_to_root_and_run_identity() {
        let root = cache_root("marker-binding");
        let store = RunJournalStore::initialize(&root).expect("store");
        store.create_run(RUN_ID, AT).expect("first");
        store.create_run(RUN_ID_B, AT).expect("second");
        let first_marker = fs::read(store.run_path(RUN_ID).join(RUN_MARKER)).expect("marker");
        fs::write(store.run_path(RUN_ID_B).join(RUN_MARKER), first_marker).expect("replace marker");
        let status = store.status().expect("status");
        assert_eq!(status.runs.len(), 2);
        assert_eq!(
            status.runs[1].classification,
            RecoveryClassificationV1::OperatorRequired
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn quarantine_retry_is_idempotent_after_post_rename_sync_failure() {
        let root = cache_root("quarantine-retry");
        let mut store = RunJournalStore::initialize(&root).expect("store");
        store.create_run(RUN_ID, AT).expect("run");
        store.durable = DurableFileSystem::failing_at(2);
        assert!(store.apply(RUN_ID).is_err());
        store.durable = DurableFileSystem::default();
        let result = store.apply(RUN_ID).expect("idempotent retry");
        assert_eq!(result.outcome, RecoveryClassificationV1::Quarantined);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn journal_payload_is_path_and_secret_free() {
        let entry = RunJournalEntryV1 {
            schema_version: RUN_JOURNAL_SCHEMA_VERSION.to_owned(),
            run_id: RUN_ID.to_owned(),
            seq: 0,
            state: RunJournalStateV1::Created,
            at_utc: AT.to_owned(),
            failure_kind: None,
        };
        let json = serde_json::to_string(&entry).expect("json");
        for forbidden in ["/Users/", "C:\\\\", "..", "HOME", "TOKEN", "SECRET"] {
            assert!(!json.contains(forbidden));
        }
    }

    #[test]
    fn journal_schema_is_versioned_and_strict() {
        let schema = run_journal_entry_schema_json().expect("schema");
        assert!(schema.contains("RunJournalEntryV1"));
        assert!(schema.contains("schema_version"));
        assert!(schema.contains("additionalProperties"));
    }

    #[test]
    fn injected_storage_failures_never_classify_partial_state_as_terminal() {
        for fail_at in 1..=6 {
            let root = cache_root("storage-full");
            let mut store = RunJournalStore::initialize(&root).expect("store");
            store.durable = DurableFileSystem::failing_at(fail_at);
            assert!(store.create_run(RUN_ID, AT).is_err());
            let status = RunJournalStore::open(&root)
                .expect("open")
                .expect("journal")
                .status()
                .expect("status");
            assert!(
                status
                    .runs
                    .iter()
                    .all(|run| run.classification != RecoveryClassificationV1::Terminal)
            );
            fs::remove_dir_all(root).expect("cleanup");
        }
    }

    fn directory_fingerprint(root: &Path) -> Vec<(PathBuf, u64)> {
        fn walk(root: &Path, path: &Path, output: &mut Vec<(PathBuf, u64)>) {
            let mut entries: Vec<_> = fs::read_dir(path)
                .expect("read dir")
                .collect::<Result<_, _>>()
                .expect("entries");
            entries.sort_by_key(std::fs::DirEntry::file_name);
            for entry in entries {
                let path = entry.path();
                let metadata = entry.metadata().expect("metadata");
                output.push((
                    path.strip_prefix(root).expect("relative").to_owned(),
                    metadata.len(),
                ));
                if metadata.is_dir() {
                    walk(root, &path, output);
                }
            }
        }
        let mut output = Vec::new();
        walk(root, root, &mut output);
        output
    }
}
