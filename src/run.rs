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

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::cache::{CacheError, ManagedCache};
use crate::config::{ExecutionPlanEnvelopeV1, NormalizedCheck, RuntimeKind};
use crate::process::{
    CancellationReason, CancellationToken, CleanupStatus, GenerationGuard, ProcessError,
    ProcessRequest, ProcessResult, ProcessTermination, RunIdentity, SupervisorPort,
};
use crate::receipt::{
    CheckEvidence, EvidenceStatus, PlatformEvidence, ProducerEvidence, ReceiptEnvelopeV1,
    ReceiptEnvelopeV2, ReceiptError, ReceiptV1, ReceiptV2, RepositoryEvidence, RunEvidence,
    SourceSnapshotEvidence, SourceSnapshotStrategy, canonical_digest,
};
use crate::runtime::{RuntimeError, RuntimePort, docker_execution_environment, doctor_guard};
use crate::source_snapshot::{SourceSnapshot, SourceSnapshotError};
use crate::workspace::{PreparedWorkspace, WorkspaceError};

const GIT_TIMEOUT: Duration = Duration::from_secs(10);
const GIT_CAPTURE_BYTES: usize = 65_536;
const REDACTION_POLICY_VERSION: &str = "ccp-redaction-v1";
static RECEIPT_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct RunRequest<'a> {
    pub envelope: &'a ExecutionPlanEnvelopeV1,
    pub repository: &'a Path,
    pub cache: &'a ManagedCache,
    pub generation: u64,
    pub source_snapshot: Option<&'a SourceSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunOutcome {
    pub receipt: ReceiptEnvelopeV1,
    pub receipt_v2: Option<ReceiptEnvelopeV2>,
    pub receipt_path: PathBuf,
}

pub trait CompletionBarrier {
    fn finalize(&mut self, checks: &[CheckEvidence]) -> Result<(), RunError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunLifecyclePhase {
    Prepared,
    Executing,
    Finalizing,
    Sealed,
}

pub trait RunLifecycleObserver {
    fn transition(&mut self, phase: RunLifecyclePhase) -> Result<(), RunError>;
}

#[derive(Debug, Default)]
pub struct NoopRunLifecycleObserver;

impl RunLifecycleObserver for NoopRunLifecycleObserver {
    fn transition(&mut self, _phase: RunLifecyclePhase) -> Result<(), RunError> {
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct NoopCompletionBarrier;

impl CompletionBarrier for NoopCompletionBarrier {
    fn finalize(&mut self, _checks: &[CheckEvidence]) -> Result<(), RunError> {
        Ok(())
    }
}

impl RunOutcome {
    pub fn exit_code(&self) -> i32 {
        match self.receipt.receipt.overall_status {
            EvidenceStatus::Pass => 0,
            EvidenceStatus::Fail => 1,
            EvidenceStatus::Pending | EvidenceStatus::NotRun => 5,
        }
    }

    pub fn published_canonical_bytes(&self) -> Result<Vec<u8>, ReceiptError> {
        match &self.receipt_v2 {
            Some(receipt) => receipt.canonical_bytes(),
            None => self.receipt.canonical_bytes(),
        }
    }
}

pub trait Clock: Send + Sync {
    fn now_utc(&self) -> Result<String, RunError>;
}

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_utc(&self) -> Result<String, RunError> {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| RunError::Clock)?
            .as_secs();
        format_unix_utc(seconds)
    }
}

pub fn execute_local_run(
    request: &RunRequest<'_>,
    runtime: &dyn RuntimePort,
    supervisor: &dyn SupervisorPort,
    cancellation: &CancellationToken,
    clock: &dyn Clock,
) -> Result<RunOutcome, RunError> {
    let mut barrier = NoopCompletionBarrier;
    execute_local_run_with_barrier(
        request,
        runtime,
        supervisor,
        cancellation,
        clock,
        &mut barrier,
    )
}

pub fn execute_local_run_with_barrier(
    request: &RunRequest<'_>,
    runtime: &dyn RuntimePort,
    supervisor: &dyn SupervisorPort,
    cancellation: &CancellationToken,
    clock: &dyn Clock,
    barrier: &mut dyn CompletionBarrier,
) -> Result<RunOutcome, RunError> {
    let mut lifecycle = NoopRunLifecycleObserver;
    execute_local_run_with_barrier_and_lifecycle(
        request,
        runtime,
        supervisor,
        cancellation,
        clock,
        barrier,
        &mut lifecycle,
    )
}

pub fn execute_local_run_with_barrier_and_lifecycle(
    request: &RunRequest<'_>,
    runtime: &dyn RuntimePort,
    supervisor: &dyn SupervisorPort,
    cancellation: &CancellationToken,
    clock: &dyn Clock,
    barrier: &mut dyn CompletionBarrier,
    lifecycle: &mut dyn RunLifecycleObserver,
) -> Result<RunOutcome, RunError> {
    let receipt = execute_local_receipt_with_barrier_and_lifecycle(
        request,
        runtime,
        supervisor,
        cancellation,
        clock,
        barrier,
        lifecycle,
    )?;
    let receipt_v2 = request
        .source_snapshot
        .map(|snapshot| {
            ReceiptEnvelopeV2::seal(ReceiptV2 {
                schema_version: crate::receipt::RECEIPT_V2_SCHEMA_VERSION.to_owned(),
                producer: receipt.receipt.producer.clone(),
                repository: receipt.receipt.repository.clone(),
                run: receipt.receipt.run.clone(),
                source_snapshot: SourceSnapshotEvidence {
                    schema_version: crate::receipt::SOURCE_SNAPSHOT_SCHEMA_VERSION.to_owned(),
                    strategy: SourceSnapshotStrategy::GitObject,
                    manifest_digest: snapshot.evidence().manifest_digest.clone(),
                    entry_count: snapshot.evidence().entry_count,
                },
                platform: receipt.receipt.platform.clone(),
                configuration_digest: receipt.receipt.configuration_digest.clone(),
                checks: receipt.receipt.checks.clone(),
                overall_status: receipt.receipt.overall_status,
                incomplete_reason: receipt.receipt.incomplete_reason.clone(),
                redaction_policy_version: receipt.receipt.redaction_policy_version.clone(),
            })
        })
        .transpose()
        .map_err(RunError::Receipt)?;
    let published_bytes = match &receipt_v2 {
        Some(value) => value.canonical_bytes(),
        None => receipt.canonical_bytes(),
    }
    .map_err(RunError::Receipt)?;
    let receipt_path = write_receipt_bytes_atomic(
        request.repository,
        &request.envelope.plan.receipt.output,
        &published_bytes,
    )?;
    Ok(RunOutcome {
        receipt,
        receipt_v2,
        receipt_path,
    })
}

/// Execute one already-validated v1 plan and return sealed evidence without
/// writing a receipt into the source checkout. Matrix orchestration uses this
/// primitive so every runtime observes the same clean exact head before the
/// single outer receipt is published.
pub fn execute_local_receipt_with_barrier_and_lifecycle(
    request: &RunRequest<'_>,
    runtime: &dyn RuntimePort,
    supervisor: &dyn SupervisorPort,
    cancellation: &CancellationToken,
    clock: &dyn Clock,
    barrier: &mut dyn CompletionBarrier,
    lifecycle: &mut dyn RunLifecycleObserver,
) -> Result<ReceiptEnvelopeV1, RunError> {
    if cancellation.reason() == Some(CancellationReason::ResourcePressure) {
        return Err(RunError::ResourcePressure);
    }
    let repository = fs::canonicalize(request.repository).map_err(RunError::Io)?;
    if !repository.is_dir() {
        return Err(RunError::RepositoryNotDirectory);
    }
    let identity = RunIdentity {
        project: request.envelope.plan.project.clone(),
        commit: None,
        config_digest: request.envelope.plan_digest.clone(),
        generation: request.generation.to_string(),
    };
    let generation = GenerationGuard::new(identity.clone());
    let commit = if let Some(snapshot) = request.source_snapshot {
        snapshot.commit_sha().to_owned()
    } else {
        inspect_repository(
            request.envelope,
            &repository,
            supervisor,
            cancellation,
            &generation,
            &identity,
        )
        .map_err(|error| {
            if cancellation.reason() == Some(CancellationReason::ResourcePressure) {
                RunError::ResourcePressure
            } else {
                error
            }
        })?
    };
    let identity = RunIdentity {
        commit: Some(commit.clone()),
        ..identity
    };
    generation
        .replace(identity.clone())
        .map_err(RunError::Process)?;

    let execution_root = request
        .source_snapshot
        .map(SourceSnapshot::root)
        .unwrap_or(repository.as_path());
    let probe = runtime
        .probe(
            request.envelope,
            supervisor,
            execution_root,
            cancellation,
            &doctor_guard(request.envelope),
        )
        .map_err(|error| {
            if cancellation.reason() == Some(CancellationReason::ResourcePressure) {
                RunError::ResourcePressure
            } else {
                RunError::Runtime(error)
            }
        })?;
    if cancellation.reason() == Some(CancellationReason::ResourcePressure) {
        return Err(RunError::ResourcePressure);
    }
    let prepared = if let Some(snapshot) = request.source_snapshot {
        PreparedWorkspace::prepare_snapshot(
            request.envelope,
            execution_root,
            &snapshot.evidence().manifest_digest,
            request.cache,
        )
    } else {
        PreparedWorkspace::prepare(request.envelope, execution_root, request.cache)
    }
    .map_err(RunError::Workspace)?;
    let dry_run = runtime
        .dry_run(request.envelope, &prepared.plan)
        .map_err(RunError::Runtime)?;
    if dry_run.checks.len() != request.envelope.plan.checks.len() {
        return Err(RunError::Invariant("runtime check plan length changed"));
    }
    lifecycle.transition(RunLifecyclePhase::Prepared)?;

    let started_at_utc = clock.now_utc()?;
    let run_id = run_id(
        request.envelope,
        &commit,
        request.generation,
        &started_at_utc,
    )?;
    let environment = docker_execution_environment(&request.envelope.plan.environment_allow);
    let mut statuses = BTreeMap::new();
    let mut checks = Vec::with_capacity(request.envelope.plan.checks.len());
    let mut terminal_reason: Option<&'static str> = None;
    lifecycle.transition(RunLifecyclePhase::Executing)?;

    for (declared, rendered) in request.envelope.plan.checks.iter().zip(&dry_run.checks) {
        let evidence = if cancellation.is_cancelled() {
            let reason = cancellation_reason_text(cancellation);
            terminal_reason = Some(reason);
            not_run(declared, reason)
        } else if let Some(reason) = terminal_reason {
            not_run(declared, reason)
        } else if declared
            .depends_on
            .iter()
            .any(|dependency| statuses.get(dependency) != Some(&EvidenceStatus::Pass))
        {
            not_run(declared, "dependency did not pass in this run")
        } else {
            match runtime.execute_check(
                request.envelope,
                declared,
                rendered,
                execution_root,
                &environment,
                &identity,
                &run_id,
                supervisor,
                cancellation,
                &generation,
            ) {
                Ok(result)
                    if result.termination == ProcessTermination::Cancelled
                        && cancellation.reason() == Some(CancellationReason::ResourcePressure) =>
                {
                    terminal_reason = Some("host resource pressure watchdog tripped");
                    not_run(declared, "host resource pressure watchdog tripped")
                }
                Ok(result) => evidence_from_result(declared, result)?,
                Err(error @ RuntimeError::LifecycleFailure(_))
                | Err(error @ RuntimeError::CleanupUncertain)
                | Err(error @ RuntimeError::Process(ProcessError::CleanupUncertain { .. })) => {
                    return Err(RunError::Runtime(error));
                }
                Err(_) => {
                    let reason =
                        if cancellation.reason() == Some(CancellationReason::ResourcePressure) {
                            "host resource pressure watchdog tripped"
                        } else {
                            "runtime execution became unavailable or uncertain"
                        };
                    terminal_reason = Some(reason);
                    not_run(declared, reason)
                }
            }
        };
        statuses.insert(declared.id.clone(), evidence.status);
        checks.push(evidence);
    }

    if cancellation.reason() == Some(CancellationReason::ResourcePressure)
        && !has_resource_pressure_not_run(&checks)
    {
        return Err(RunError::ResourcePressure);
    }

    let post_run_cancellation = CancellationToken::default();
    let completed_commit = inspect_repository(
        request.envelope,
        &repository,
        supervisor,
        &post_run_cancellation,
        &generation,
        &identity,
    )?;
    if completed_commit != commit {
        return Err(RunError::StaleCommit);
    }
    if let Some(snapshot) = request.source_snapshot {
        snapshot
            .revalidate(supervisor, &post_run_cancellation, &generation, &identity)
            .map_err(RunError::SourceSnapshot)?;
    }
    if cancellation.reason() == Some(CancellationReason::ResourcePressure)
        && !has_resource_pressure_not_run(&checks)
    {
        return Err(RunError::ResourcePressure);
    }

    barrier.finalize(&checks)?;
    lifecycle.transition(RunLifecyclePhase::Finalizing)?;

    let all_checks_passed = checks
        .iter()
        .all(|check| check.status == EvidenceStatus::Pass);
    if all_checks_passed {
        prepared
            .mark_caches_complete(request.cache)
            .map_err(RunError::Workspace)?;
    }
    let finished_at_utc = clock.now_utc()?;
    let overall_status = derive_overall_status(&request.envelope.plan.checks, &statuses)?;
    let incomplete_reason = (overall_status == EvidenceStatus::Pending)
        .then(|| "one or more required checks were not run".to_owned());
    let image_digest = request
        .envelope
        .plan
        .runtime
        .image
        .rsplit_once('@')
        .map(|(_, digest)| digest.to_owned())
        .ok_or(RunError::Invariant("validated image lost its digest"))?;
    let receipt = ReceiptEnvelopeV1::seal(ReceiptV1 {
        schema_version: crate::receipt::RECEIPT_SCHEMA_VERSION.to_owned(),
        producer: ProducerEvidence {
            name: env!("CARGO_PKG_NAME").to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        },
        repository: RepositoryEvidence {
            repository: request.envelope.plan.project.clone(),
            commit_sha: commit,
            dirty: false,
        },
        run: RunEvidence {
            run_id,
            generation: request.generation,
            started_at_utc,
            finished_at_utc,
        },
        platform: PlatformEvidence {
            host_os: std::env::consts::OS.to_owned(),
            host_arch: std::env::consts::ARCH.to_owned(),
            runtime_kind: match probe.runtime {
                RuntimeKind::DockerCompatible => "docker_compatible",
                RuntimeKind::Host => "host",
            }
            .to_owned(),
            runtime_version: probe
                .server_version
                .unwrap_or_else(|| "not-reported".to_owned()),
            image_reference: request.envelope.plan.runtime.image.clone(),
            image_digest,
        },
        configuration_digest: request.envelope.plan_digest.clone(),
        checks,
        overall_status,
        incomplete_reason,
        redaction_policy_version: REDACTION_POLICY_VERSION.to_owned(),
    })
    .map_err(RunError::Receipt)?;
    Ok(receipt)
}

fn inspect_repository(
    envelope: &ExecutionPlanEnvelopeV1,
    repository: &Path,
    supervisor: &dyn SupervisorPort,
    cancellation: &CancellationToken,
    generation: &GenerationGuard,
    identity: &RunIdentity,
) -> Result<String, RunError> {
    let commit = execute_git(
        repository,
        &["rev-parse", "--verify", "HEAD"],
        supervisor,
        cancellation,
        generation,
        identity,
    )?;
    let commit = commit.trim();
    if !matches!(commit.len(), 40 | 64)
        || !commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(RunError::InvalidCommit);
    }
    let exclusion = format!(":(exclude){}", envelope.plan.receipt.output);
    let status = execute_git(
        repository,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--",
            ".",
            &exclusion,
        ],
        supervisor,
        cancellation,
        generation,
        identity,
    )?;
    if !status.is_empty() {
        return Err(RunError::DirtyRepository);
    }
    Ok(commit.to_owned())
}

fn execute_git(
    repository: &Path,
    argv: &[&str],
    supervisor: &dyn SupervisorPort,
    cancellation: &CancellationToken,
    generation: &GenerationGuard,
    identity: &RunIdentity,
) -> Result<String, RunError> {
    let request = ProcessRequest {
        identity: identity.clone(),
        program: OsString::from("git"),
        argv: argv.iter().map(OsString::from).collect(),
        current_dir: repository.to_path_buf(),
        environment: git_environment(),
        timeout: GIT_TIMEOUT,
        max_capture_bytes: GIT_CAPTURE_BYTES,
    };
    let result = supervisor
        .execute(&request, cancellation, generation)
        .map_err(|error| {
            if cancellation.reason() == Some(CancellationReason::ResourcePressure) {
                RunError::ResourcePressure
            } else {
                RunError::Process(error)
            }
        })?;
    if cancellation.reason() == Some(CancellationReason::ResourcePressure) {
        return Err(RunError::ResourcePressure);
    }
    if result.termination != ProcessTermination::Completed
        || result.cleanup != CleanupStatus::Verified
        || result.exit.map(|exit| exit.success) != Some(true)
        || result.stdout.truncated
        || result.stderr.truncated
    {
        return Err(RunError::GitInspection);
    }
    String::from_utf8(result.stdout.bytes).map_err(|_| RunError::GitInspection)
}

fn git_environment() -> BTreeMap<OsString, OsString> {
    const ALLOWED: &[&str] = &[
        "PATH",
        "HOME",
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "SYSTEMROOT",
        "XDG_CONFIG_HOME",
    ];
    ALLOWED
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|value| (OsString::from(name), value)))
        .collect()
}

fn evidence_from_result(
    check: &NormalizedCheck,
    result: ProcessResult,
) -> Result<CheckEvidence, RunError> {
    if result.cleanup != CleanupStatus::Verified {
        return Err(RunError::Invariant("process cleanup was not verified"));
    }
    let duration_ms = u64::try_from(result.elapsed_millis).unwrap_or(u64::MAX);
    let (status, timed_out, cancelled) = match result.termination {
        ProcessTermination::TimedOut => (EvidenceStatus::Fail, true, false),
        ProcessTermination::Cancelled => (EvidenceStatus::Fail, false, true),
        ProcessTermination::Completed => (
            if result.exit.map(|exit| exit.success) == Some(true) {
                EvidenceStatus::Pass
            } else {
                EvidenceStatus::Fail
            },
            false,
            false,
        ),
    };
    let output_digest = canonical_digest(&OutputDigestInput {
        stdout: &result.stdout.bytes,
        stderr: &result.stderr.bytes,
        stdout_truncated: result.stdout.truncated,
        stderr_truncated: result.stderr.truncated,
    })
    .map_err(RunError::Receipt)?;
    Ok(CheckEvidence {
        id: check.id.clone(),
        required: check.required,
        argv: check.argv.clone(),
        working_directory: check.working_directory.clone(),
        status,
        exit_code: result.exit.and_then(|exit| exit.code),
        duration_ms,
        timed_out,
        cancelled,
        output_digest: Some(output_digest),
        incomplete_reason: None,
    })
}

#[derive(Serialize)]
struct OutputDigestInput<'a> {
    stdout: &'a [u8],
    stderr: &'a [u8],
    stdout_truncated: bool,
    stderr_truncated: bool,
}

fn not_run(check: &NormalizedCheck, reason: &'static str) -> CheckEvidence {
    CheckEvidence {
        id: check.id.clone(),
        required: check.required,
        argv: check.argv.clone(),
        working_directory: check.working_directory.clone(),
        status: EvidenceStatus::NotRun,
        exit_code: None,
        duration_ms: 0,
        timed_out: false,
        cancelled: false,
        output_digest: None,
        incomplete_reason: Some(reason.to_owned()),
    }
}

fn cancellation_reason_text(cancellation: &CancellationToken) -> &'static str {
    match cancellation.reason() {
        Some(CancellationReason::ResourcePressure) => "host resource pressure watchdog tripped",
        Some(CancellationReason::User) | None => "run cancelled before check execution",
    }
}

fn has_resource_pressure_not_run(checks: &[CheckEvidence]) -> bool {
    checks.iter().any(|check| {
        check.status == EvidenceStatus::NotRun
            && check.incomplete_reason.as_deref() == Some("host resource pressure watchdog tripped")
    })
}

fn derive_overall_status(
    declared: &[NormalizedCheck],
    statuses: &BTreeMap<String, EvidenceStatus>,
) -> Result<EvidenceStatus, RunError> {
    let required: Vec<_> = declared
        .iter()
        .filter(|check| check.required)
        .map(|check| {
            statuses
                .get(&check.id)
                .copied()
                .ok_or(RunError::Invariant("required check result is missing"))
        })
        .collect::<Result<_, _>>()?;
    if required.contains(&EvidenceStatus::Fail) {
        Ok(EvidenceStatus::Fail)
    } else if required
        .iter()
        .any(|status| matches!(status, EvidenceStatus::Pending | EvidenceStatus::NotRun))
    {
        Ok(EvidenceStatus::Pending)
    } else {
        Ok(EvidenceStatus::Pass)
    }
}

#[derive(Serialize)]
struct RunIdInput<'a> {
    schema_version: &'static str,
    project: &'a str,
    commit: &'a str,
    configuration_digest: &'a str,
    generation: u64,
    started_at_utc: &'a str,
}

fn run_id(
    envelope: &ExecutionPlanEnvelopeV1,
    commit: &str,
    generation: u64,
    started_at_utc: &str,
) -> Result<String, RunError> {
    canonical_digest(&RunIdInput {
        schema_version: "1.0",
        project: &envelope.plan.project,
        commit,
        configuration_digest: &envelope.plan_digest,
        generation,
        started_at_utc,
    })
    .map_err(RunError::Receipt)
}

pub fn write_receipt_atomic(
    repository: &Path,
    relative_output: &str,
    receipt: &ReceiptEnvelopeV1,
) -> Result<PathBuf, RunError> {
    let bytes = receipt.canonical_bytes().map_err(RunError::Receipt)?;
    write_receipt_bytes_atomic(repository, relative_output, &bytes)
}

/// Create a receipt file exactly once from already canonical receipt bytes.
/// This is shared by versioned outer receipt envelopes; it preserves the v1
/// no-overwrite, path-isolation, fsync, and atomic-rename guarantees.
pub fn write_canonical_receipt_bytes_atomic(
    repository: &Path,
    relative_output: &str,
    bytes: &[u8],
) -> Result<PathBuf, RunError> {
    write_receipt_bytes_atomic(repository, relative_output, bytes)
}

fn write_receipt_bytes_atomic(
    repository: &Path,
    relative_output: &str,
    bytes: &[u8],
) -> Result<PathBuf, RunError> {
    let repository = fs::canonicalize(repository).map_err(RunError::Io)?;
    let target = repository.join(relative_output);
    if !target.starts_with(&repository) {
        return Err(RunError::UnsafeReceiptPath);
    }
    let parent = target.parent().ok_or(RunError::UnsafeReceiptPath)?;
    create_directory_chain(&repository, parent)?;
    if let Ok(metadata) = fs::symlink_metadata(&target)
        && (metadata.file_type().is_symlink() || !metadata.is_file())
    {
        return Err(RunError::UnsafeReceiptPath);
    }
    let sequence = RECEIPT_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".receipt-tmp-{}-{sequence}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(RunError::Io)?;
        file.write_all(bytes).map_err(RunError::Io)?;
        file.sync_all().map_err(RunError::Io)?;
        fs::rename(&temporary, &target).map_err(RunError::Io)
    })();
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(target)
}

fn create_directory_chain(root: &Path, target: &Path) -> Result<(), RunError> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| RunError::UnsafeReceiptPath)?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::create_dir(&current) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&current).map_err(RunError::Io)?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(RunError::UnsafeReceiptPath);
                }
            }
            Err(error) => return Err(RunError::Io(error)),
        }
    }
    Ok(())
}

fn format_unix_utc(seconds: u64) -> Result<String, RunError> {
    let days = i64::try_from(seconds / 86_400).map_err(|_| RunError::Clock)?;
    let day_seconds = seconds % 86_400;
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;
    let (year, month, day) = civil_from_days(days);
    if !(1..=9999).contains(&year) {
        return Err(RunError::Clock);
    }
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
    ))
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

#[derive(Debug)]
pub enum RunError {
    RepositoryNotDirectory,
    DirtyRepository,
    InvalidCommit,
    StaleCommit,
    ResourcePressure,
    GitInspection,
    UnsafeReceiptPath,
    Clock,
    Invariant(&'static str),
    Cache(CacheError),
    Workspace(WorkspaceError),
    SourceSnapshot(SourceSnapshotError),
    Runtime(RuntimeError),
    Process(ProcessError),
    Receipt(ReceiptError),
    Io(io::Error),
}

impl RunError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::RepositoryNotDirectory
            | Self::DirtyRepository
            | Self::InvalidCommit
            | Self::UnsafeReceiptPath => 2,
            Self::StaleCommit => 5,
            Self::ResourcePressure => 6,
            Self::Runtime(error) => error.exit_code(),
            Self::Process(ProcessError::StaleGeneration) => 5,
            Self::Process(_) | Self::GitInspection => 4,
            Self::Cache(_)
            | Self::Workspace(_)
            | Self::SourceSnapshot(_)
            | Self::Clock
            | Self::Invariant(_)
            | Self::Receipt(_)
            | Self::Io(_) => 70,
        }
    }
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RepositoryNotDirectory => formatter.write_str("repository is not a directory"),
            Self::DirtyRepository => formatter.write_str("repository has uncommitted changes"),
            Self::InvalidCommit => formatter.write_str("Git returned an invalid commit identifier"),
            Self::StaleCommit => {
                formatter.write_str("repository commit changed while checks were running")
            }
            Self::ResourcePressure => {
                formatter.write_str("host resource pressure watchdog tripped")
            }
            Self::GitInspection => formatter.write_str("Git repository inspection failed"),
            Self::UnsafeReceiptPath => formatter.write_str("receipt output path is unsafe"),
            Self::Clock => formatter.write_str("UTC clock could not be represented"),
            Self::Invariant(message) => write!(formatter, "run invariant failed: {message}"),
            Self::Cache(error) => write!(formatter, "{error}"),
            Self::Workspace(error) => write!(formatter, "{error}"),
            Self::SourceSnapshot(error) => write!(formatter, "{error}"),
            Self::Runtime(error) => write!(formatter, "{error}"),
            Self::Process(error) => write!(formatter, "{error}"),
            Self::Receipt(error) => write!(formatter, "{error}"),
            Self::Io(_) => formatter.write_str("run filesystem operation failed"),
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Cache(error) => Some(error),
            Self::Workspace(error) => Some(error),
            Self::SourceSnapshot(error) => Some(error),
            Self::Runtime(error) => Some(error),
            Self::Process(error) => Some(error),
            Self::Receipt(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::Mutex;

    use crate::cache::{CacheRootOptions, PlatformFamily, ResolvedCacheRoot};
    use crate::config::ConfigV1;
    use crate::process::{CapturedStream, ExitOutcome};
    use crate::runtime::DockerCompatibleRuntime;

    #[test]
    fn utc_formatter_covers_epoch_leap_day_and_current_range() {
        assert_eq!(format_unix_utc(0).expect("epoch"), "1970-01-01T00:00:00Z");
        assert_eq!(
            format_unix_utc(951_782_400).expect("leap day"),
            "2000-02-29T00:00:00Z"
        );
        assert_eq!(
            format_unix_utc(1_785_908_096).expect("2026"),
            "2026-08-05T05:34:56Z"
        );
    }

    #[derive(Clone, Copy)]
    enum ExecutionMode {
        Pass,
        FailFirst,
        TimeoutFirst,
        RuntimeUnavailable,
        Dirty,
        InvalidCommit,
        StaleCommit,
        CancelDuringFirstCheck,
        CancelDuringGitInspection,
        CancelDuringRuntimeProbe,
    }

    struct FakeSupervisor {
        mode: ExecutionMode,
        docker_runs: AtomicU64,
        containers: Mutex<BTreeMap<String, BTreeMap<String, String>>>,
        git_revisions: AtomicU64,
    }

    impl FakeSupervisor {
        fn new(mode: ExecutionMode) -> Self {
            Self {
                mode,
                docker_runs: AtomicU64::new(0),
                containers: Mutex::new(BTreeMap::new()),
                git_revisions: AtomicU64::new(0),
            }
        }
    }

    impl SupervisorPort for FakeSupervisor {
        fn execute(
            &self,
            request: &ProcessRequest,
            cancellation: &CancellationToken,
            generation: &GenerationGuard,
        ) -> Result<ProcessResult, ProcessError> {
            generation.ensure_current(&request.identity)?;
            let program = request.program.to_string_lossy();
            if program == "git" {
                if cancellation.is_cancelled() {
                    return Err(ProcessError::Invariant(
                        "post-run Git audit reused the cancelled run token",
                    ));
                }
                let git_command = request.argv.first().and_then(|argument| argument.to_str());
                let is_status = git_command == Some("status");
                if matches!(self.mode, ExecutionMode::CancelDuringGitInspection)
                    && self.git_revisions.fetch_add(1, Ordering::SeqCst) == 0
                {
                    cancellation.cancel_resource_pressure();
                }
                let stdout = if git_command == Some("ls-tree") {
                    format!("100644 blob {}\tREADME.md\0", "c".repeat(40)).into_bytes()
                } else if git_command == Some("cat-file") {
                    b"snapshot source\n".to_vec()
                } else if git_command == Some("hash-object") {
                    format!("{}\n", "c".repeat(40)).into_bytes()
                } else if is_status {
                    if matches!(self.mode, ExecutionMode::Dirty) {
                        b" M source.rs\n".to_vec()
                    } else {
                        Vec::new()
                    }
                } else if matches!(self.mode, ExecutionMode::InvalidCommit) {
                    b"not-a-commit\n".to_vec()
                } else if matches!(self.mode, ExecutionMode::StaleCommit)
                    && self.git_revisions.fetch_add(1, Ordering::SeqCst) > 0
                {
                    format!("{}\n", "b".repeat(40)).into_bytes()
                } else {
                    format!("{}\n", "a".repeat(40)).into_bytes()
                };
                return Ok(process_result(
                    request,
                    stdout,
                    true,
                    ProcessTermination::Completed,
                ));
            }
            let is_probe = request
                .argv
                .first()
                .is_some_and(|argument| argument == "info");
            if is_probe {
                if matches!(self.mode, ExecutionMode::RuntimeUnavailable) {
                    return Err(ProcessError::Spawn(io::Error::new(
                        io::ErrorKind::NotFound,
                        "fixture runtime absent",
                    )));
                }
                if matches!(self.mode, ExecutionMode::CancelDuringRuntimeProbe) {
                    cancellation.cancel_resource_pressure();
                }
                return Ok(process_result(
                    request,
                    br#"{"ServerVersion":"29.4.0","OperatingSystem":"OrbStack","OSType":"linux","Name":"private"}"#.to_vec(),
                    true,
                    ProcessTermination::Completed,
                ));
            }
            let command = request.argv.first().and_then(|argument| argument.to_str());
            match command {
                Some("create") => {
                    let mut name = None;
                    let mut labels = BTreeMap::new();
                    let mut arguments = request.argv.iter();
                    while let Some(argument) = arguments.next() {
                        match argument.to_str() {
                            Some("--name") => {
                                name = arguments.next().and_then(|value| value.to_str())
                            }
                            Some("--label") => {
                                if let Some((key, value)) = arguments
                                    .next()
                                    .and_then(|value| value.to_str())
                                    .and_then(|value| value.split_once('='))
                                {
                                    labels.insert(key.to_owned(), value.to_owned());
                                }
                            }
                            _ => {}
                        }
                    }
                    self.containers
                        .lock()
                        .expect("containers")
                        .insert(name.expect("container name").to_owned(), labels);
                    Ok(process_result(
                        request,
                        format!("{}\n", "a".repeat(64)).into_bytes(),
                        true,
                        ProcessTermination::Completed,
                    ))
                }
                Some("start" | "stop" | "kill") => Ok(process_result(
                    request,
                    Vec::new(),
                    true,
                    ProcessTermination::Completed,
                )),
                Some("rm") => {
                    let name = request
                        .argv
                        .last()
                        .and_then(|argument| argument.to_str())
                        .expect("container name");
                    self.containers.lock().expect("containers").remove(name);
                    Ok(process_result(
                        request,
                        Vec::new(),
                        true,
                        ProcessTermination::Completed,
                    ))
                }
                Some("inspect") => {
                    let name = request
                        .argv
                        .last()
                        .and_then(|argument| argument.to_str())
                        .expect("container name");
                    let labels = self
                        .containers
                        .lock()
                        .expect("containers")
                        .get(name)
                        .cloned();
                    if let Some(labels) = labels {
                        let inspect = serde_json::json!({
                            "Name": format!("/{name}"),
                            "Config": {"Labels": labels},
                            "State": {"Status": "running"},
                        });
                        Ok(process_result(
                            request,
                            serde_json::to_vec(&inspect).expect("inspect JSON"),
                            true,
                            ProcessTermination::Completed,
                        ))
                    } else {
                        Ok(process_result(
                            request,
                            b"Error: No such container\n".to_vec(),
                            false,
                            ProcessTermination::Completed,
                        ))
                    }
                }
                Some("attach") => {
                    let index = self.docker_runs.fetch_add(1, Ordering::SeqCst);
                    if matches!(self.mode, ExecutionMode::CancelDuringFirstCheck) && index == 0 {
                        cancellation.cancel_resource_pressure();
                        return Ok(process_result(
                            request,
                            b"resource-pressure".to_vec(),
                            false,
                            ProcessTermination::Cancelled,
                        ));
                    }
                    let termination =
                        if matches!(self.mode, ExecutionMode::TimeoutFirst) && index == 0 {
                            ProcessTermination::TimedOut
                        } else {
                            ProcessTermination::Completed
                        };
                    Ok(process_result(
                        request,
                        format!("fixture-output-{index}").into_bytes(),
                        true,
                        termination,
                    ))
                }
                Some("wait") => {
                    let index = self.docker_runs.load(Ordering::SeqCst).saturating_sub(1);
                    let exit_code = if matches!(self.mode, ExecutionMode::FailFirst) && index == 0 {
                        9
                    } else {
                        0
                    };
                    Ok(process_result(
                        request,
                        format!("{exit_code}\n").into_bytes(),
                        true,
                        ProcessTermination::Completed,
                    ))
                }
                _ => Ok(process_result(
                    request,
                    b"fixture-runtime-command\n".to_vec(),
                    true,
                    ProcessTermination::Completed,
                )),
            }
        }
    }

    fn process_result(
        request: &ProcessRequest,
        stdout: Vec<u8>,
        success: bool,
        termination: ProcessTermination,
    ) -> ProcessResult {
        ProcessResult {
            identity: request.identity.clone(),
            termination,
            cleanup: CleanupStatus::Verified,
            exit: Some(ExitOutcome {
                success,
                code: Some(if success { 0 } else { 9 }),
            }),
            stdout: CapturedStream {
                bytes: stdout,
                truncated: false,
            },
            stderr: CapturedStream {
                bytes: Vec::new(),
                truncated: false,
            },
            elapsed_millis: 25,
        }
    }

    struct FixedClock {
        values: Mutex<VecDeque<String>>,
    }

    impl FixedClock {
        fn new() -> Self {
            Self {
                values: Mutex::new(VecDeque::from([
                    "2026-08-09T03:00:00Z".to_owned(),
                    "2026-08-09T03:00:01Z".to_owned(),
                ])),
            }
        }
    }

    impl Clock for FixedClock {
        fn now_utc(&self) -> Result<String, RunError> {
            self.values
                .lock()
                .map_err(|_| RunError::Clock)?
                .pop_front()
                .ok_or(RunError::Clock)
        }
    }

    struct CountingBarrier {
        calls: usize,
        deny: bool,
    }

    #[derive(Default)]
    struct RecordingLifecycle {
        phases: Vec<RunLifecyclePhase>,
    }

    impl RunLifecycleObserver for RecordingLifecycle {
        fn transition(&mut self, phase: RunLifecyclePhase) -> Result<(), RunError> {
            self.phases.push(phase);
            Ok(())
        }
    }

    impl CompletionBarrier for CountingBarrier {
        fn finalize(&mut self, _checks: &[CheckEvidence]) -> Result<(), RunError> {
            self.calls += 1;
            if self.deny {
                Err(RunError::ResourcePressure)
            } else {
                Ok(())
            }
        }
    }

    struct RunFixture {
        root: PathBuf,
        repository: PathBuf,
        cache: ManagedCache,
        envelope: ExecutionPlanEnvelopeV1,
    }

    impl RunFixture {
        fn new(label: &str) -> Self {
            let root = std::env::var_os("CCP_TEST_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    std::env::current_dir()
                        .expect("current directory")
                        .parent()
                        .expect("repository parent")
                        .to_path_buf()
                })
                .join(format!(".ccp-run-test-{}-{label}", std::process::id()));
            if root.exists() {
                fs::remove_dir_all(&root).expect("clean fixture root");
            }
            let repository = root.join("repository");
            fs::create_dir_all(repository.join(".cache/cargo")).expect("cache mount target");
            fs::create_dir_all(repository.join("results")).expect("artifact parent");
            fs::File::create(repository.join("results/first.json")).expect("artifact mount target");
            let resolved = ResolvedCacheRoot::resolve(
                &repository,
                &CacheRootOptions {
                    explicit: Some(root.join("persistent-cache")),
                    environment: None,
                    home: None,
                    xdg_cache_home: None,
                    local_app_data: None,
                    platform: PlatformFamily::Unix,
                },
            )
            .expect("cache root");
            let cache = ManagedCache::initialize(resolved).expect("cache");
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
id = "first"
required = true
argv = ["fixture", "first"]
working_directory = "."
timeout_seconds = 60
artifacts = ["results/first.json"]

[[checks]]
id = "second"
required = true
argv = ["fixture", "second"]
working_directory = "."
timeout_seconds = 60
depends_on = ["first"]
"#,
            )
            .expect("config")
            .into_plan()
            .expect("plan");
            Self {
                root,
                repository,
                cache,
                envelope,
            }
        }

        fn execute(&self, mode: ExecutionMode) -> Result<RunOutcome, RunError> {
            self.execute_with_cancellation(mode, &CancellationToken::default())
        }

        fn execute_with_cancellation(
            &self,
            mode: ExecutionMode,
            cancellation: &CancellationToken,
        ) -> Result<RunOutcome, RunError> {
            execute_local_run(
                &RunRequest {
                    envelope: &self.envelope,
                    repository: &self.repository,
                    cache: &self.cache,
                    generation: 7,
                    source_snapshot: None,
                },
                &DockerCompatibleRuntime,
                &FakeSupervisor::new(mode),
                cancellation,
                &FixedClock::new(),
            )
        }

        fn execute_with_barrier(
            &self,
            mode: ExecutionMode,
            barrier: &mut dyn CompletionBarrier,
        ) -> Result<RunOutcome, RunError> {
            execute_local_run_with_barrier(
                &RunRequest {
                    envelope: &self.envelope,
                    repository: &self.repository,
                    cache: &self.cache,
                    generation: 7,
                    source_snapshot: None,
                },
                &DockerCompatibleRuntime,
                &FakeSupervisor::new(mode),
                &CancellationToken::default(),
                &FixedClock::new(),
                barrier,
            )
        }
    }

    impl Drop for RunFixture {
        fn drop(&mut self) {
            if self.root.exists() {
                fs::remove_dir_all(&self.root).expect("remove exact fixture root");
            }
        }
    }

    #[test]
    fn successful_run_seals_and_atomically_writes_receipt() {
        let fixture = RunFixture::new("pass");
        let outcome = fixture.execute(ExecutionMode::Pass).expect("run");

        assert_eq!(outcome.exit_code(), 0);
        assert_eq!(outcome.receipt.receipt.overall_status, EvidenceStatus::Pass);
        assert!(outcome.receipt_path.is_file());
        let decoded: ReceiptEnvelopeV1 =
            serde_json::from_slice(&fs::read(&outcome.receipt_path).expect("receipt bytes"))
                .expect("receipt JSON");
        decoded.verify().expect("sealed receipt");
        assert_eq!(decoded, outcome.receipt);
        assert!(
            fixture
                .cache
                .inventory()
                .expect("inventory")
                .entries
                .iter()
                .all(|entry| entry.status == crate::cache::CacheEntryStatus::Complete)
        );
    }

    #[test]
    fn snapshot_run_publishes_v2_receipt_bound_to_manifest() {
        let fixture = RunFixture::new("snapshot-v2");
        let supervisor = FakeSupervisor::new(ExecutionMode::Pass);
        let commit = "a".repeat(40);
        let identity = RunIdentity {
            project: fixture.envelope.plan.project.clone(),
            commit: Some(commit.clone()),
            config_digest: fixture.envelope.plan_digest.clone(),
            generation: "7".to_owned(),
        };
        let generation = GenerationGuard::new(identity.clone());
        let resource_root = fixture.root.join("source-snapshot");
        let mut snapshot = SourceSnapshot::materialize(
            &fixture.repository,
            &commit,
            &resource_root,
            &supervisor,
            &CancellationToken::default(),
            &generation,
            &identity,
        )
        .expect("source snapshot");
        snapshot
            .prepare_mount_overlay(&fixture.envelope)
            .expect("mount overlay");

        let outcome = execute_local_run(
            &RunRequest {
                envelope: &fixture.envelope,
                repository: &fixture.repository,
                cache: &fixture.cache,
                generation: 7,
                source_snapshot: Some(&snapshot),
            },
            &DockerCompatibleRuntime,
            &supervisor,
            &CancellationToken::default(),
            &FixedClock::new(),
        )
        .expect("snapshot run");

        let decoded: ReceiptEnvelopeV2 =
            serde_json::from_slice(&fs::read(&outcome.receipt_path).expect("receipt bytes"))
                .expect("v2 receipt JSON");
        decoded.verify().expect("sealed v2 receipt");
        assert_eq!(decoded.receipt.repository.commit_sha, commit);
        assert_eq!(
            decoded.receipt.source_snapshot.manifest_digest,
            snapshot.evidence().manifest_digest
        );
        assert_eq!(
            decoded.receipt.source_snapshot.entry_count,
            snapshot.evidence().entry_count
        );
        assert_eq!(outcome.receipt_v2, Some(decoded));
    }

    #[test]
    fn successful_run_reports_ordered_prepared_executing_and_finalizing_phases() {
        let fixture = RunFixture::new("lifecycle");
        let mut barrier = CountingBarrier {
            calls: 0,
            deny: false,
        };
        let mut lifecycle = RecordingLifecycle::default();
        execute_local_run_with_barrier_and_lifecycle(
            &RunRequest {
                envelope: &fixture.envelope,
                repository: &fixture.repository,
                cache: &fixture.cache,
                generation: 7,
                source_snapshot: None,
            },
            &DockerCompatibleRuntime,
            &FakeSupervisor::new(ExecutionMode::Pass),
            &CancellationToken::default(),
            &FixedClock::new(),
            &mut barrier,
            &mut lifecycle,
        )
        .expect("run");
        assert_eq!(
            lifecycle.phases,
            vec![
                RunLifecyclePhase::Prepared,
                RunLifecyclePhase::Executing,
                RunLifecyclePhase::Finalizing,
            ]
        );
    }

    #[test]
    fn completion_barrier_trip_prevents_receipt_and_cache_completion() {
        let fixture = RunFixture::new("barrier-trip");
        let mut barrier = CountingBarrier {
            calls: 0,
            deny: true,
        };
        let error = fixture
            .execute_with_barrier(ExecutionMode::Pass, &mut barrier)
            .expect_err("barrier trip");

        assert!(matches!(error, RunError::ResourcePressure));
        assert_eq!(barrier.calls, 1);
        assert!(!fixture.repository.join(".ccp/receipt.json").exists());
        assert!(
            fixture
                .cache
                .inventory()
                .expect("inventory")
                .entries
                .iter()
                .all(|entry| entry.status == crate::cache::CacheEntryStatus::Incomplete)
        );
    }

    #[test]
    fn completion_barrier_allows_only_pending_resource_evidence() {
        let fixture = RunFixture::new("barrier-pending");
        let mut barrier = CountingBarrier {
            calls: 0,
            deny: false,
        };
        let outcome = fixture
            .execute_with_barrier(ExecutionMode::CancelDuringFirstCheck, &mut barrier)
            .expect("pending receipt");

        assert_eq!(barrier.calls, 1);
        assert_eq!(
            outcome.receipt.receipt.overall_status,
            EvidenceStatus::Pending
        );
        assert!(outcome.receipt.receipt.checks.iter().all(|check| {
            check.status == EvidenceStatus::NotRun
                && check.incomplete_reason.as_deref()
                    == Some("host resource pressure watchdog tripped")
        }));
    }

    #[test]
    fn completion_barrier_no_trip_preserves_pass() {
        let fixture = RunFixture::new("barrier-pass");
        let mut barrier = CountingBarrier {
            calls: 0,
            deny: false,
        };
        let outcome = fixture
            .execute_with_barrier(ExecutionMode::Pass, &mut barrier)
            .expect("pass receipt");

        assert_eq!(barrier.calls, 1);
        assert_eq!(outcome.receipt.receipt.overall_status, EvidenceStatus::Pass);
    }

    #[test]
    fn failed_dependency_is_not_run_and_cache_stays_incomplete() {
        let fixture = RunFixture::new("failure");
        let outcome = fixture.execute(ExecutionMode::FailFirst).expect("run");

        assert_eq!(outcome.exit_code(), 1);
        assert_eq!(
            outcome.receipt.receipt.checks[0].status,
            EvidenceStatus::Fail
        );
        assert_eq!(
            outcome.receipt.receipt.checks[1].status,
            EvidenceStatus::NotRun
        );
        assert_eq!(outcome.receipt.receipt.overall_status, EvidenceStatus::Fail);
        assert!(
            fixture
                .cache
                .inventory()
                .expect("inventory")
                .entries
                .iter()
                .all(|entry| entry.status == crate::cache::CacheEntryStatus::Incomplete)
        );
    }

    #[test]
    fn timeout_is_a_failure_and_never_a_pass() {
        let fixture = RunFixture::new("timeout");
        let outcome = fixture.execute(ExecutionMode::TimeoutFirst).expect("run");
        assert_eq!(outcome.receipt.receipt.overall_status, EvidenceStatus::Fail);
        assert!(outcome.receipt.receipt.checks[0].timed_out);
        assert_ne!(outcome.exit_code(), 0);
    }

    #[test]
    fn resource_pressure_during_first_check_writes_not_run_evidence() {
        let fixture = RunFixture::new("resource-pressure-check");
        let outcome = fixture
            .execute(ExecutionMode::CancelDuringFirstCheck)
            .expect("resource-pressure receipt");

        assert_eq!(outcome.exit_code(), 5);
        assert_eq!(
            outcome.receipt.receipt.overall_status,
            EvidenceStatus::Pending
        );
        assert!(outcome.receipt.receipt.checks.iter().all(|check| {
            check.status == EvidenceStatus::NotRun
                && !check.cancelled
                && check.incomplete_reason.as_deref()
                    == Some("host resource pressure watchdog tripped")
        }));
    }

    #[test]
    fn resource_pressure_at_entry_returns_without_a_receipt() {
        let fixture = RunFixture::new("resource-pressure-entry");
        let cancellation = CancellationToken::default();
        cancellation.cancel_resource_pressure();
        let error = fixture
            .execute_with_cancellation(ExecutionMode::Pass, &cancellation)
            .expect_err("entry pressure must stop before Git");

        assert!(matches!(error, RunError::ResourcePressure));
        assert!(!fixture.repository.join(".ccp/receipt.json").exists());
    }

    #[test]
    fn resource_pressure_during_git_inspection_maps_to_resource_error() {
        let fixture = RunFixture::new("resource-pressure-git");
        let error = fixture
            .execute(ExecutionMode::CancelDuringGitInspection)
            .expect_err("Git pressure must fail closed");

        assert!(matches!(error, RunError::ResourcePressure));
        assert!(!fixture.repository.join(".ccp/receipt.json").exists());
    }

    #[test]
    fn resource_pressure_during_runtime_probe_maps_to_resource_error() {
        let fixture = RunFixture::new("resource-pressure-runtime");
        let error = fixture
            .execute(ExecutionMode::CancelDuringRuntimeProbe)
            .expect_err("runtime pressure must fail closed");

        assert!(matches!(error, RunError::ResourcePressure));
        assert!(!fixture.repository.join(".ccp/receipt.json").exists());
    }

    #[test]
    fn dirty_invalid_commit_and_runtime_loss_fail_before_receipt() {
        for (label, mode) in [
            ("dirty", ExecutionMode::Dirty),
            ("invalid", ExecutionMode::InvalidCommit),
            ("runtime", ExecutionMode::RuntimeUnavailable),
            ("stale-commit", ExecutionMode::StaleCommit),
        ] {
            let fixture = RunFixture::new(label);
            assert!(fixture.execute(mode).is_err());
            assert!(!fixture.repository.join(".ccp/receipt.json").exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn receipt_writer_rejects_symlinked_parent() {
        use std::os::unix::fs::symlink;

        let fixture = RunFixture::new("receipt-symlink");
        let outside = fixture.root.join("outside");
        fs::create_dir_all(&outside).expect("outside");
        symlink(&outside, fixture.repository.join(".ccp")).expect("symlink");
        let receipt = fixture
            .execute(ExecutionMode::Pass)
            .expect_err("symlink must fail");
        assert!(matches!(receipt, RunError::UnsafeReceiptPath));
        assert!(
            fs::read_dir(outside)
                .expect("outside listing")
                .next()
                .is_none()
        );
    }
}
