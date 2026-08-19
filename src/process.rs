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
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(windows)]
use process_wrap::std::JobObject;
#[cfg(unix)]
use process_wrap::std::ProcessGroup;
use process_wrap::std::{ChildWrapper, CommandWrap};
use serde::Serialize;
use sha2::{Digest, Sha256};

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const DEFAULT_GRACE_PERIOD: Duration = Duration::from_millis(500);
const GROUP_EXIT_POLL: Duration = Duration::from_millis(5);
const MAX_CAPTURE_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunIdentity {
    pub project: String,
    pub commit: Option<String>,
    pub config_digest: String,
    pub generation: String,
}

#[derive(Debug, Clone)]
pub struct GenerationGuard {
    active: Arc<Mutex<RunIdentity>>,
}

impl GenerationGuard {
    pub fn new(identity: RunIdentity) -> Self {
        Self {
            active: Arc::new(Mutex::new(identity)),
        }
    }

    pub fn replace(&self, identity: RunIdentity) -> Result<(), ProcessError> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| ProcessError::Invariant("generation guard lock is poisoned"))?;
        *active = identity;
        Ok(())
    }

    pub fn ensure_current(&self, candidate: &RunIdentity) -> Result<(), ProcessError> {
        let active = self
            .active
            .lock()
            .map_err(|_| ProcessError::Invariant("generation guard lock is poisoned"))?;
        if *active == *candidate {
            Ok(())
        } else {
            Err(ProcessError::StaleGeneration)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancellationReason {
    User = 1,
    ResourcePressure = 2,
}

#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    reason: Arc<AtomicU8>,
}

impl CancellationToken {
    pub fn cancel(&self) {
        self.set_reason(CancellationReason::User);
    }

    pub fn cancel_resource_pressure(&self) {
        self.set_reason(CancellationReason::ResourcePressure);
    }

    pub fn is_cancelled(&self) -> bool {
        self.reason.load(Ordering::Acquire) != 0
    }

    pub fn reason(&self) -> Option<CancellationReason> {
        match self.reason.load(Ordering::Acquire) {
            1 => Some(CancellationReason::User),
            2 => Some(CancellationReason::ResourcePressure),
            _ => None,
        }
    }

    fn set_reason(&self, reason: CancellationReason) {
        let _ = self
            .reason
            .compare_exchange(0, reason as u8, Ordering::AcqRel, Ordering::Acquire);
    }
}

#[derive(Debug, Clone)]
pub struct ProcessRequest {
    pub identity: RunIdentity,
    pub program: OsString,
    pub argv: Vec<OsString>,
    pub current_dir: PathBuf,
    pub environment: BTreeMap<OsString, OsString>,
    pub timeout: Duration,
    pub max_capture_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Capture,
    Tee,
}

impl ProcessRequest {
    pub fn validate(&self) -> Result<(), ProcessError> {
        if self.program.is_empty() {
            return Err(ProcessError::InvalidRequest("program must not be empty"));
        }
        if self.timeout.is_zero() {
            return Err(ProcessError::InvalidRequest(
                "timeout must be greater than zero",
            ));
        }
        if self.max_capture_bytes == 0 || self.max_capture_bytes > MAX_CAPTURE_BYTES {
            return Err(ProcessError::InvalidRequest(
                "max_capture_bytes must be between 1 and 1048576",
            ));
        }
        if !self.current_dir.is_absolute() {
            return Err(ProcessError::InvalidRequest(
                "current_dir must be an already-resolved absolute path",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessTermination {
    Completed,
    Cancelled,
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupStatus {
    NotRequired,
    Verified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExitOutcome {
    pub success: bool,
    pub code: Option<i32>,
}

impl From<ExitStatus> for ExitOutcome {
    fn from(status: ExitStatus) -> Self {
        Self {
            success: status.success(),
            code: status.code(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedStream {
    pub bytes: Vec<u8>,
    pub byte_count: u64,
    pub full_digest: String,
    pub truncated: bool,
}

impl CapturedStream {
    pub fn from_captured(bytes: Vec<u8>, truncated: bool) -> Self {
        let byte_count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let full_digest = sha256_digest(&bytes);
        Self {
            bytes,
            byte_count,
            full_digest,
            truncated,
        }
    }

    fn empty() -> Self {
        Self::from_captured(Vec::new(), false)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessResult {
    pub identity: RunIdentity,
    pub termination: ProcessTermination,
    pub cleanup: CleanupStatus,
    pub exit: Option<ExitOutcome>,
    pub stdout: CapturedStream,
    pub stderr: CapturedStream,
    pub elapsed_millis: u128,
}

pub trait SupervisorPort: Send + Sync {
    fn execute(
        &self,
        request: &ProcessRequest,
        cancellation: &CancellationToken,
        generation: &GenerationGuard,
    ) -> Result<ProcessResult, ProcessError>;
}

pub trait ProcessSpawner: Send + Sync {
    fn spawn(&self, request: &ProcessRequest) -> Result<Box<dyn ManagedProcess>, ProcessError>;

    fn spawn_with_output(
        &self,
        request: &ProcessRequest,
        output_mode: OutputMode,
    ) -> Result<Box<dyn ManagedProcess>, ProcessError> {
        if output_mode == OutputMode::Capture {
            self.spawn(request)
        } else {
            Err(ProcessError::UnsupportedOutputMode)
        }
    }
}

pub trait ManagedProcess: Send {
    fn try_wait(&mut self) -> io::Result<Option<ExitOutcome>>;
    fn request_graceful_stop(&mut self) -> io::Result<GracefulStop>;
    fn force_stop_and_wait(&mut self, deadline: Duration) -> io::Result<Option<ExitOutcome>>;
    fn seal_descendants(&mut self, deadline: Duration) -> io::Result<()>;
    fn collect_output(
        self: Box<Self>,
        deadline: Duration,
    ) -> io::Result<(CapturedStream, CapturedStream)>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GracefulStop {
    Requested,
    AlreadyStopped,
    Unsupported,
}

#[derive(Debug, Default)]
pub struct StdProcessSpawner;

impl ProcessSpawner for StdProcessSpawner {
    fn spawn(&self, request: &ProcessRequest) -> Result<Box<dyn ManagedProcess>, ProcessError> {
        self.spawn_with_output(request, OutputMode::Capture)
    }

    fn spawn_with_output(
        &self,
        request: &ProcessRequest,
        output_mode: OutputMode,
    ) -> Result<Box<dyn ManagedProcess>, ProcessError> {
        let mut command = CommandWrap::with_new(&request.program, |command| {
            command
                .args(&request.argv)
                .current_dir(&request.current_dir)
                .env_clear()
                .envs(&request.environment)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
        });
        #[cfg(unix)]
        command.wrap(ProcessGroup::leader());
        #[cfg(windows)]
        command.wrap(JobObject);

        let mut child = command.spawn().map_err(ProcessError::Spawn)?;
        let pid = child.id();
        let stdout = child
            .stdout()
            .take()
            .ok_or(ProcessError::Invariant("stdout pipe was not created"))?;
        let stderr = child
            .stderr()
            .take()
            .ok_or(ProcessError::Invariant("stderr pipe was not created"))?;
        let limit = request.max_capture_bytes;
        let stdout_writer = (output_mode == OutputMode::Tee)
            .then(|| Box::new(io::stdout()) as Box<dyn Write + Send>);
        let stderr_writer = (output_mode == OutputMode::Tee)
            .then(|| Box::new(io::stderr()) as Box<dyn Write + Send>);

        Ok(Box::new(StdManagedProcess {
            child,
            pid,
            stdout: Some(spawn_reader(stdout, limit, stdout_writer)),
            stderr: Some(spawn_reader(stderr, limit, stderr_writer)),
            last_exit: None,
        }))
    }
}

pub struct ProcessSupervisor<S = StdProcessSpawner> {
    spawner: S,
    poll_interval: Duration,
    grace_period: Duration,
}

#[derive(Debug, Clone, Copy)]
struct ProcessDeadline {
    started: Instant,
    total: Duration,
    execution_budget: Duration,
    cleanup_budget: Duration,
}

impl ProcessDeadline {
    fn new(started: Instant, total: Duration, cleanup_cap: Duration) -> Self {
        let cleanup_budget = cleanup_cap.min(total / 2);
        Self {
            started,
            total,
            execution_budget: total.saturating_sub(cleanup_budget),
            cleanup_budget,
        }
    }

    fn execution_expired(self) -> bool {
        self.started.elapsed() >= self.execution_budget
    }

    fn cleanup_remaining(self) -> Duration {
        self.cleanup_budget
            .min(self.total.saturating_sub(self.started.elapsed()))
    }
}

impl ProcessSupervisor<StdProcessSpawner> {
    pub fn standard() -> Self {
        Self::new(StdProcessSpawner)
    }
}

impl<S> ProcessSupervisor<S> {
    pub fn new(spawner: S) -> Self {
        Self {
            spawner,
            poll_interval: DEFAULT_POLL_INTERVAL,
            grace_period: DEFAULT_GRACE_PERIOD,
        }
    }

    #[cfg(test)]
    fn with_timing(mut self, poll_interval: Duration, grace_period: Duration) -> Self {
        self.poll_interval = poll_interval;
        self.grace_period = grace_period;
        self
    }
}

impl<S: ProcessSpawner> SupervisorPort for ProcessSupervisor<S> {
    fn execute(
        &self,
        request: &ProcessRequest,
        cancellation: &CancellationToken,
        generation: &GenerationGuard,
    ) -> Result<ProcessResult, ProcessError> {
        self.execute_with_output(request, cancellation, generation, OutputMode::Capture)
    }
}

impl<S: ProcessSpawner> ProcessSupervisor<S> {
    pub fn execute_with_output(
        &self,
        request: &ProcessRequest,
        cancellation: &CancellationToken,
        generation: &GenerationGuard,
        output_mode: OutputMode,
    ) -> Result<ProcessResult, ProcessError> {
        request.validate()?;
        generation.ensure_current(&request.identity)?;
        let started = Instant::now();
        let deadline = ProcessDeadline::new(started, request.timeout, self.grace_period);

        if cancellation.is_cancelled() {
            return Ok(empty_result(
                request,
                ProcessTermination::Cancelled,
                CleanupStatus::NotRequired,
                started,
            ));
        }

        let mut child = self.spawner.spawn_with_output(request, output_mode)?;
        loop {
            if generation.ensure_current(&request.identity).is_err() {
                self.stop_and_collect(
                    child,
                    request,
                    ProcessTermination::Cancelled,
                    started,
                    deadline,
                )?;
                return Err(ProcessError::StaleGeneration);
            }
            if cancellation.is_cancelled() {
                return self.stop_and_collect(
                    child,
                    request,
                    ProcessTermination::Cancelled,
                    started,
                    deadline,
                );
            }
            if deadline.execution_expired() {
                return self.stop_and_collect(
                    child,
                    request,
                    ProcessTermination::TimedOut,
                    started,
                    deadline,
                );
            }
            let observed = match child.try_wait() {
                Ok(observed) => observed,
                Err(error) => {
                    let cleanup = self.stop_and_collect(
                        child,
                        request,
                        ProcessTermination::Cancelled,
                        started,
                        deadline,
                    );
                    return match cleanup {
                        Ok(_) => Err(ProcessError::Monitor(error)),
                        Err(cleanup_error) => Err(cleanup_error),
                    };
                }
            };
            if let Some(exit) = observed {
                child
                    .seal_descendants(deadline.cleanup_remaining())
                    .map_err(|source| ProcessError::CleanupUncertain {
                        stage: "completed descendant seal",
                        source,
                    })?;
                let (stdout, stderr) = child
                    .collect_output(deadline.cleanup_remaining())
                    .map_err(ProcessError::Output)?;
                generation.ensure_current(&request.identity)?;
                return Ok(ProcessResult {
                    identity: request.identity.clone(),
                    termination: ProcessTermination::Completed,
                    cleanup: CleanupStatus::Verified,
                    exit: Some(exit),
                    stdout,
                    stderr,
                    elapsed_millis: started.elapsed().as_millis(),
                });
            }
            thread::sleep(self.poll_interval);
        }
    }

    fn stop_and_collect(
        &self,
        mut child: Box<dyn ManagedProcess>,
        request: &ProcessRequest,
        termination: ProcessTermination,
        started: Instant,
        deadline: ProcessDeadline,
    ) -> Result<ProcessResult, ProcessError> {
        let graceful = child.request_graceful_stop();
        if matches!(graceful, Ok(GracefulStop::Requested)) {
            let grace_started = Instant::now();
            while grace_started.elapsed() < deadline.cleanup_remaining() {
                let observed =
                    child
                        .try_wait()
                        .map_err(|source| ProcessError::CleanupUncertain {
                            stage: "graceful stop monitoring",
                            source,
                        })?;
                if let Some(exit) = observed {
                    child
                        .seal_descendants(deadline.cleanup_remaining())
                        .map_err(|source| ProcessError::CleanupUncertain {
                            stage: "graceful descendant seal",
                            source,
                        })?;
                    let (stdout, stderr) = child
                        .collect_output(deadline.cleanup_remaining())
                        .map_err(ProcessError::Output)?;
                    return Ok(ProcessResult {
                        identity: request.identity.clone(),
                        termination,
                        cleanup: CleanupStatus::Verified,
                        exit: Some(exit),
                        stdout,
                        stderr,
                        elapsed_millis: started.elapsed().as_millis(),
                    });
                }
                thread::sleep(self.poll_interval);
            }
        }

        let exit = match child.force_stop_and_wait(deadline.cleanup_remaining()) {
            Ok(exit) => exit,
            Err(source) => {
                child
                    .seal_descendants(deadline.cleanup_remaining())
                    .map_err(|seal_source| ProcessError::CleanupUncertain {
                        stage: "forced descendant seal after force-stop failure",
                        source: seal_source,
                    })?;
                return Err(ProcessError::CleanupUncertain {
                    stage: "force stop",
                    source,
                });
            }
        };
        child
            .seal_descendants(deadline.cleanup_remaining())
            .map_err(|source| ProcessError::CleanupUncertain {
                stage: "forced descendant seal",
                source,
            })?;
        let (stdout, stderr) = child
            .collect_output(deadline.cleanup_remaining())
            .map_err(ProcessError::Output)?;
        Ok(ProcessResult {
            identity: request.identity.clone(),
            termination,
            cleanup: CleanupStatus::Verified,
            exit,
            stdout,
            stderr,
            elapsed_millis: started.elapsed().as_millis(),
        })
    }
}

fn empty_result(
    request: &ProcessRequest,
    termination: ProcessTermination,
    cleanup: CleanupStatus,
    started: Instant,
) -> ProcessResult {
    ProcessResult {
        identity: request.identity.clone(),
        termination,
        cleanup,
        exit: None,
        stdout: CapturedStream::empty(),
        stderr: CapturedStream::empty(),
        elapsed_millis: started.elapsed().as_millis(),
    }
}

struct StdManagedProcess {
    child: Box<dyn ChildWrapper>,
    pid: u32,
    stdout: Option<ReaderHandle>,
    stderr: Option<ReaderHandle>,
    last_exit: Option<ExitOutcome>,
}

impl ManagedProcess for StdManagedProcess {
    fn try_wait(&mut self) -> io::Result<Option<ExitOutcome>> {
        if let Some(exit) = self.last_exit {
            return Ok(Some(exit));
        }
        let exit = self.child.try_wait()?.map(ExitOutcome::from);
        if let Some(exit) = exit {
            self.last_exit = Some(exit);
        }
        Ok(exit)
    }

    fn request_graceful_stop(&mut self) -> io::Result<GracefulStop> {
        #[cfg(unix)]
        {
            use nix::errno::Errno;
            use nix::sys::signal::{Signal, killpg};
            use nix::unistd::Pid;

            match killpg(Pid::from_raw(pid_as_i32(self.pid)?), Signal::SIGTERM) {
                Ok(()) => Ok(GracefulStop::Requested),
                Err(Errno::ESRCH) => Ok(GracefulStop::AlreadyStopped),
                Err(error) => Err(io::Error::from(error)),
            }
        }
        #[cfg(windows)]
        {
            Ok(GracefulStop::Unsupported)
        }
    }

    fn force_stop_and_wait(&mut self, deadline: Duration) -> io::Result<Option<ExitOutcome>> {
        match self.child.start_kill() {
            Ok(()) => {}
            Err(_) if self.try_wait()?.is_some() => {}
            Err(error) => return Err(error),
        }
        let started = Instant::now();
        loop {
            if let Some(exit) = self.try_wait()? {
                return Ok(Some(exit));
            }
            if started.elapsed() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "child did not exit within the force-stop deadline",
                ));
            }
            thread::sleep(GROUP_EXIT_POLL);
        }
    }

    fn seal_descendants(&mut self, deadline: Duration) -> io::Result<()> {
        #[cfg(unix)]
        {
            seal_unix_process_group(self.pid, deadline)
        }
        #[cfg(windows)]
        {
            match self.child.start_kill() {
                Ok(()) => {}
                Err(error) if self.try_wait()?.is_some() => {}
                Err(error) => return Err(error),
            }
            let exit = ExitOutcome::from(self.child.wait()?);
            self.last_exit = Some(exit);
            Ok(())
        }
    }

    fn collect_output(
        mut self: Box<Self>,
        deadline: Duration,
    ) -> io::Result<(CapturedStream, CapturedStream)> {
        let started = Instant::now();
        let stdout = self
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("stdout reader is missing"))?;
        let stderr = self
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("stderr reader is missing"))?;
        let stdout = match stdout.join_within(deadline) {
            Ok(stream) => stream,
            Err(error) => {
                stderr.cancel();
                return Err(error);
            }
        };
        let remaining = deadline.saturating_sub(started.elapsed());
        let stderr = stderr.join_within(remaining)?;
        Ok((stdout, stderr))
    }
}

struct ReaderHandle {
    result: Receiver<io::Result<CapturedStream>>,
    cancel_flag: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl ReaderHandle {
    fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::Release);
    }

    fn join_within(mut self, deadline: Duration) -> io::Result<CapturedStream> {
        let result = match self.result.recv_timeout(deadline) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                self.cancel();
                self.join.take();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "output reader did not finish within the cleanup deadline",
                ));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(io::Error::other(
                    "output reader thread terminated unexpectedly",
                ));
            }
        }?;
        self.join
            .take()
            .ok_or_else(|| io::Error::other("output reader join handle is missing"))?
            .join()
            .map_err(|_| io::Error::other("output reader thread panicked"))?;
        Ok(result)
    }
}

fn spawn_reader<R: Read + Send + 'static>(
    mut reader: R,
    limit: usize,
    mut writer: Option<Box<dyn Write + Send>>,
) -> ReaderHandle {
    let (sender, result) = mpsc::sync_channel(1);
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let thread_cancel_flag = Arc::clone(&cancel_flag);
    let join = thread::spawn(move || {
        let result = read_stream(&mut reader, limit, &mut writer, &thread_cancel_flag);
        let _ = sender.send(result);
    });
    ReaderHandle {
        result,
        cancel_flag,
        join: Some(join),
    }
}

fn read_stream<R: Read>(
    reader: &mut R,
    limit: usize,
    writer: &mut Option<Box<dyn Write + Send>>,
    cancel_flag: &AtomicBool,
) -> io::Result<CapturedStream> {
    let mut captured = Vec::with_capacity(limit.min(8192));
    let mut byte_count = 0_u64;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    let mut truncated = false;
    let mut write_error = None;
    loop {
        if cancel_flag.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "output reader cancellation requested",
            ));
        }
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        byte_count = byte_count.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        hasher.update(&buffer[..read]);
        if let Some(writer) = writer.as_mut() {
            if let Err(error) = writer
                .write_all(&buffer[..read])
                .and_then(|_| writer.flush())
            {
                if write_error.is_none() {
                    write_error = Some(io::Error::new(error.kind(), error.to_string()));
                }
            }
        }
        let remaining = limit.saturating_sub(captured.len());
        let kept = remaining.min(read);
        captured.extend_from_slice(&buffer[..kept]);
        truncated |= kept < read;
    }
    if let Some(error) = write_error {
        return Err(error);
    }
    Ok(CapturedStream {
        bytes: captured,
        byte_count,
        full_digest: digest_from_hasher(hasher),
        truncated,
    })
}

fn sha256_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    digest_from_hasher(hasher)
}

fn digest_from_hasher(hasher: Sha256) -> String {
    let hex: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}

#[cfg(unix)]
fn seal_unix_process_group(pid: u32, deadline: Duration) -> io::Result<()> {
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;

    let group = Pid::from_raw(pid_as_i32(pid)?);
    match killpg(group, Signal::SIGKILL) {
        Ok(()) | Err(Errno::ESRCH) => {}
        Err(error) => return Err(io::Error::from(error)),
    }

    let started = Instant::now();
    loop {
        match killpg(group, None) {
            Err(Errno::ESRCH) => return Ok(()),
            Ok(()) | Err(Errno::EPERM) if started.elapsed() < deadline => {
                thread::sleep(GROUP_EXIT_POLL);
            }
            Ok(()) | Err(Errno::EPERM) => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "process group still exists after force stop",
                ));
            }
            Err(error) => return Err(io::Error::from(error)),
        }
    }
}

#[cfg(unix)]
fn pid_as_i32(pid: u32) -> io::Result<i32> {
    i32::try_from(pid).map_err(io::Error::other)
}

#[derive(Debug)]
pub enum ProcessError {
    InvalidRequest(&'static str),
    Spawn(io::Error),
    Monitor(io::Error),
    Output(io::Error),
    UnsupportedOutputMode,
    CleanupUncertain {
        stage: &'static str,
        source: io::Error,
    },
    StaleGeneration,
    Invariant(&'static str),
}

impl fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(message) => {
                write!(formatter, "invalid process request: {message}")
            }
            Self::Spawn(_) => formatter.write_str("process could not be started"),
            Self::Monitor(_) => formatter.write_str("process state could not be monitored"),
            Self::Output(_) => formatter.write_str("process output could not be collected"),
            Self::UnsupportedOutputMode => {
                formatter.write_str("requested process output mode is unavailable")
            }
            Self::CleanupUncertain { stage, .. } => {
                write!(formatter, "process cleanup is uncertain at stage: {stage}")
            }
            Self::StaleGeneration => formatter.write_str("stale process generation was rejected"),
            Self::Invariant(message) => write!(formatter, "internal process invariant: {message}"),
        }
    }
}

impl std::error::Error for ProcessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(source)
            | Self::Monitor(source)
            | Self::Output(source)
            | Self::CleanupUncertain { source, .. } => Some(source),
            Self::InvalidRequest(_)
            | Self::UnsupportedOutputMode
            | Self::StaleGeneration
            | Self::Invariant(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn identity(generation: &str) -> RunIdentity {
        RunIdentity {
            project: "owner/repository".to_owned(),
            commit: Some("a".repeat(40)),
            config_digest: format!("sha256:{}", "b".repeat(64)),
            generation: generation.to_owned(),
        }
    }

    fn request() -> ProcessRequest {
        ProcessRequest {
            identity: identity("generation-1"),
            program: OsString::from("fixture"),
            argv: Vec::new(),
            current_dir: std::env::current_dir().expect("test current directory"),
            environment: BTreeMap::new(),
            timeout: Duration::from_millis(20),
            max_capture_bytes: 1024,
        }
    }

    #[derive(Debug, Default)]
    struct FakeState {
        polls: usize,
        graceful: usize,
        forced: usize,
        sealed: usize,
        spawned: usize,
    }

    struct FakeSpawner {
        state: Arc<Mutex<FakeState>>,
        behavior: FakeBehavior,
        output_modes: Arc<StdMutex<Vec<OutputMode>>>,
    }

    #[derive(Clone)]
    enum FakeBehavior {
        Complete,
        Cancel(CancellationToken),
        NeverExit,
        Stale(GenerationGuard),
        MonitorFailure,
        CleanupFailure,
    }

    impl ProcessSpawner for FakeSpawner {
        fn spawn(
            &self,
            _request: &ProcessRequest,
        ) -> Result<Box<dyn ManagedProcess>, ProcessError> {
            self.state.lock().expect("state").spawned += 1;
            Ok(Box::new(FakeProcess {
                state: Arc::clone(&self.state),
                behavior: self.behavior.clone(),
            }))
        }

        fn spawn_with_output(
            &self,
            request: &ProcessRequest,
            output_mode: OutputMode,
        ) -> Result<Box<dyn ManagedProcess>, ProcessError> {
            self.output_modes
                .lock()
                .expect("output modes")
                .push(output_mode);
            self.spawn(request)
        }
    }

    struct FakeProcess {
        state: Arc<Mutex<FakeState>>,
        behavior: FakeBehavior,
    }

    #[derive(Clone, Default)]
    struct BufferWriter(Arc<Mutex<Vec<u8>>>);

    impl BufferWriter {
        fn bytes(&self) -> Vec<u8> {
            self.0.lock().expect("buffer").clone()
        }
    }

    impl io::Write for BufferWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("buffer").extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl ManagedProcess for FakeProcess {
        fn try_wait(&mut self) -> io::Result<Option<ExitOutcome>> {
            let mut state = self.state.lock().expect("state");
            state.polls += 1;
            match &self.behavior {
                FakeBehavior::Complete => Ok(Some(ExitOutcome {
                    success: true,
                    code: Some(0),
                })),
                FakeBehavior::Cancel(token) => {
                    token.cancel();
                    Ok(None)
                }
                FakeBehavior::NeverExit | FakeBehavior::CleanupFailure => Ok(None),
                FakeBehavior::MonitorFailure => Err(io::Error::other("injected monitor failure")),
                FakeBehavior::Stale(guard) => {
                    guard
                        .replace(identity("generation-2"))
                        .expect("replace generation");
                    Ok(None)
                }
            }
        }

        fn request_graceful_stop(&mut self) -> io::Result<GracefulStop> {
            self.state.lock().expect("state").graceful += 1;
            Ok(GracefulStop::Unsupported)
        }

        fn force_stop_and_wait(&mut self, _deadline: Duration) -> io::Result<Option<ExitOutcome>> {
            self.state.lock().expect("state").forced += 1;
            if matches!(self.behavior, FakeBehavior::CleanupFailure) {
                return Err(io::Error::other("injected cleanup failure"));
            }
            Ok(Some(ExitOutcome {
                success: false,
                code: None,
            }))
        }

        fn seal_descendants(&mut self, _deadline: Duration) -> io::Result<()> {
            self.state.lock().expect("state").sealed += 1;
            Ok(())
        }

        fn collect_output(
            self: Box<Self>,
            _deadline: Duration,
        ) -> io::Result<(CapturedStream, CapturedStream)> {
            let stream = CapturedStream::from_captured(b"fixture".to_vec(), false);
            Ok((stream.clone(), stream))
        }
    }

    type SupervisorFixture = (
        ProcessSupervisor<FakeSpawner>,
        Arc<Mutex<FakeState>>,
        Arc<StdMutex<Vec<OutputMode>>>,
    );

    fn supervisor(behavior: FakeBehavior) -> SupervisorFixture {
        let state = Arc::new(Mutex::new(FakeState::default()));
        let output_modes = Arc::new(StdMutex::new(Vec::new()));
        (
            ProcessSupervisor::new(FakeSpawner {
                state: Arc::clone(&state),
                behavior,
                output_modes: Arc::clone(&output_modes),
            })
            .with_timing(Duration::from_millis(1), Duration::from_millis(1)),
            state,
            output_modes,
        )
    }

    #[test]
    fn completed_process_is_sealed_before_acceptance() {
        let (supervisor, state, _) = supervisor(FakeBehavior::Complete);
        let request = request();
        let guard = GenerationGuard::new(request.identity.clone());
        let result = supervisor
            .execute(&request, &CancellationToken::default(), &guard)
            .expect("process succeeds");

        assert_eq!(result.termination, ProcessTermination::Completed);
        assert_eq!(result.cleanup, CleanupStatus::Verified);
        assert_eq!(result.exit.expect("exit").code, Some(0));
        assert_eq!(state.lock().expect("state").sealed, 1);
    }

    #[test]
    fn pre_cancelled_request_never_spawns() {
        let (supervisor, state, _) = supervisor(FakeBehavior::NeverExit);
        let request = request();
        let guard = GenerationGuard::new(request.identity.clone());
        let cancellation = CancellationToken::default();
        cancellation.cancel();

        let result = supervisor
            .execute(&request, &cancellation, &guard)
            .expect("pre-cancel is truthful result");

        assert_eq!(result.termination, ProcessTermination::Cancelled);
        assert_eq!(result.cleanup, CleanupStatus::NotRequired);
        assert_eq!(state.lock().expect("state").spawned, 0);
    }

    #[test]
    fn timeout_forces_and_verifies_cleanup() {
        let (supervisor, state, _) = supervisor(FakeBehavior::NeverExit);
        let mut request = request();
        request.timeout = Duration::from_millis(2);
        let guard = GenerationGuard::new(request.identity.clone());

        let result = supervisor
            .execute(&request, &CancellationToken::default(), &guard)
            .expect("timeout cleanup succeeds");

        assert_eq!(result.termination, ProcessTermination::TimedOut);
        let state = state.lock().expect("state");
        assert_eq!(state.forced, 1);
        assert_eq!(state.sealed, 1);
    }

    #[test]
    fn active_cancellation_forces_and_verifies_cleanup() {
        let cancellation = CancellationToken::default();
        let (supervisor, state, _) = supervisor(FakeBehavior::Cancel(cancellation.clone()));
        let request = request();
        let guard = GenerationGuard::new(request.identity.clone());

        let result = supervisor
            .execute(&request, &cancellation, &guard)
            .expect("active cancellation cleanup succeeds");

        assert_eq!(result.termination, ProcessTermination::Cancelled);
        let state = state.lock().expect("state");
        assert_eq!(state.forced, 1);
        assert_eq!(state.sealed, 1);
    }

    #[test]
    fn stale_generation_is_rejected_after_cleanup() {
        let request = request();
        let guard = GenerationGuard::new(request.identity.clone());
        let (supervisor, state, _) = supervisor(FakeBehavior::Stale(guard.clone()));

        let error = supervisor
            .execute(&request, &CancellationToken::default(), &guard)
            .expect_err("stale result must fail closed");

        assert!(matches!(error, ProcessError::StaleGeneration));
        let state = state.lock().expect("state");
        assert_eq!(state.forced, 1);
        assert_eq!(state.sealed, 1);
    }

    #[test]
    fn uncertain_cleanup_is_never_returned_as_result() {
        let (supervisor, state, _) = supervisor(FakeBehavior::CleanupFailure);
        let mut request = request();
        request.timeout = Duration::from_millis(2);
        let guard = GenerationGuard::new(request.identity.clone());

        let error = supervisor
            .execute(&request, &CancellationToken::default(), &guard)
            .expect_err("cleanup failure must fail closed");

        assert!(matches!(error, ProcessError::CleanupUncertain { .. }));
        assert_eq!(state.lock().expect("state").sealed, 1);
    }

    #[test]
    fn monitor_failure_attempts_bounded_cleanup_before_failing_closed() {
        let (supervisor, state, _) = supervisor(FakeBehavior::MonitorFailure);
        let request = request();
        let guard = GenerationGuard::new(request.identity.clone());

        let error = supervisor
            .execute(&request, &CancellationToken::default(), &guard)
            .expect_err("monitor failure is surfaced");

        assert!(matches!(error, ProcessError::Monitor(_)));
        let state = state.lock().expect("state");
        assert_eq!(state.graceful, 1);
        assert_eq!(state.forced, 1);
        assert_eq!(state.sealed, 1);
    }

    #[test]
    fn invalid_request_is_rejected_before_spawn() {
        let (supervisor, state, _) = supervisor(FakeBehavior::Complete);
        let mut request = request();
        request.timeout = Duration::ZERO;
        let guard = GenerationGuard::new(request.identity.clone());

        assert!(matches!(
            supervisor.execute(&request, &CancellationToken::default(), &guard),
            Err(ProcessError::InvalidRequest(_))
        ));
        assert_eq!(state.lock().expect("state").spawned, 0);
    }

    #[test]
    fn cancellation_token_is_thread_safe() {
        let token = CancellationToken::default();
        let other = token.clone();
        let updates = Arc::new(AtomicUsize::new(0));
        let updates_other = Arc::clone(&updates);
        thread::spawn(move || {
            other.cancel();
            updates_other.fetch_add(1, Ordering::SeqCst);
        })
        .join()
        .expect("cancellation thread");

        assert!(token.is_cancelled());
        assert_eq!(updates.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cancellation_reason_preserves_first_decisive_reason() {
        let token = CancellationToken::default();
        token.cancel_resource_pressure();
        token.cancel();
        assert_eq!(token.reason(), Some(CancellationReason::ResourcePressure));

        let user = CancellationToken::default();
        user.cancel();
        user.cancel_resource_pressure();
        assert_eq!(user.reason(), Some(CancellationReason::User));
    }

    #[test]
    fn capture_mode_regression_keeps_bounded_output_and_uses_capture_mode() {
        let (supervisor, _, output_modes) = supervisor(FakeBehavior::Complete);
        let request = request();
        let guard = GenerationGuard::new(request.identity.clone());

        let result = supervisor
            .execute_with_output(
                &request,
                &CancellationToken::default(),
                &guard,
                OutputMode::Capture,
            )
            .expect("capture mode");

        assert_eq!(result.stdout.bytes, b"fixture".to_vec());
        assert_eq!(result.stderr.bytes, b"fixture".to_vec());
        assert_eq!(
            output_modes.lock().expect("modes").as_slice(),
            &[OutputMode::Capture]
        );
    }

    #[test]
    fn tee_write_failure_fails_closed() {
        #[derive(Clone, Default)]
        struct FailingWriter;

        impl io::Write for FailingWriter {
            fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("tee failure"))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let error = spawn_reader(
            Cursor::new(b"stdout-bytes".to_vec()),
            32,
            Some(Box::new(FailingWriter)),
        )
        .join_within(Duration::from_secs(1))
        .expect_err("tee failure");

        assert_eq!(error.kind(), io::ErrorKind::Other);
    }

    #[test]
    fn blocked_reader_join_is_bounded_and_worker_can_finish_after_release() {
        struct GateReader {
            released: Arc<AtomicBool>,
            finished: Arc<AtomicBool>,
        }

        impl Read for GateReader {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                while !self.released.load(Ordering::Acquire) {
                    thread::yield_now();
                }
                self.finished.store(true, Ordering::Release);
                Ok(0)
            }
        }

        let released = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let error = spawn_reader(
            GateReader {
                released: Arc::clone(&released),
                finished: Arc::clone(&finished),
            },
            32,
            None,
        )
        .join_within(Duration::from_millis(2))
        .expect_err("blocked reader must respect the join deadline");

        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        released.store(true, Ordering::Release);
        let deadline = Instant::now() + Duration::from_millis(100);
        while !finished.load(Ordering::Acquire) && Instant::now() < deadline {
            thread::yield_now();
        }
        assert!(finished.load(Ordering::Acquire));
    }

    #[test]
    fn tee_reader_keeps_streams_separate_and_capture_bounded() {
        let stdout_sink = BufferWriter::default();
        let stderr_sink = BufferWriter::default();

        let stdout = spawn_reader(
            Cursor::new(b"stdout-bytes".to_vec()),
            6,
            Some(Box::new(stdout_sink.clone())),
        )
        .join_within(Duration::from_secs(1))
        .expect("stdout reader");
        let stderr = spawn_reader(
            Cursor::new(b"stderr-bytes".to_vec()),
            6,
            Some(Box::new(stderr_sink.clone())),
        )
        .join_within(Duration::from_secs(1))
        .expect("stderr reader");

        assert_eq!(stdout.bytes, b"stdout".to_vec());
        assert!(stdout.truncated);
        assert_eq!(stderr.bytes, b"stderr".to_vec());
        assert!(stderr.truncated);
        assert_eq!(stdout_sink.bytes(), b"stdout-bytes".to_vec());
        assert_eq!(stderr_sink.bytes(), b"stderr-bytes".to_vec());
    }

    #[test]
    fn full_stream_digest_and_byte_count_include_data_beyond_preview() {
        let first = spawn_reader(
            Cursor::new(b"shared-prefix-first-suffix".to_vec()),
            13,
            None,
        )
        .join_within(Duration::from_secs(1))
        .expect("first stream");
        let second = spawn_reader(
            Cursor::new(b"shared-prefix-second-suffix".to_vec()),
            13,
            None,
        )
        .join_within(Duration::from_secs(1))
        .expect("second stream");

        assert_eq!(first.bytes, second.bytes);
        assert!(first.truncated);
        assert!(second.truncated);
        assert_eq!(first.byte_count, 26);
        assert_eq!(second.byte_count, 27);
        assert_ne!(first.full_digest, second.full_digest);
    }
}
