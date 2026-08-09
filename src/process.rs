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
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(windows)]
use process_wrap::std::JobObject;
#[cfg(unix)]
use process_wrap::std::ProcessGroup;
use process_wrap::std::{ChildWrapper, CommandWrap};
use serde::Serialize;

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

#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
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
    pub truncated: bool,
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
}

pub trait ManagedProcess: Send {
    fn try_wait(&mut self) -> io::Result<Option<ExitOutcome>>;
    fn request_graceful_stop(&mut self) -> io::Result<GracefulStop>;
    fn force_stop_and_wait(&mut self) -> io::Result<Option<ExitOutcome>>;
    fn seal_descendants(&mut self, deadline: Duration) -> io::Result<()>;
    fn collect_output(self: Box<Self>) -> io::Result<(CapturedStream, CapturedStream)>;
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

        Ok(Box::new(StdManagedProcess {
            child,
            pid,
            stdout: Some(spawn_reader(stdout, limit)),
            stderr: Some(spawn_reader(stderr, limit)),
            last_exit: None,
        }))
    }
}

pub struct ProcessSupervisor<S = StdProcessSpawner> {
    spawner: S,
    poll_interval: Duration,
    grace_period: Duration,
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
        request.validate()?;
        generation.ensure_current(&request.identity)?;
        let started = Instant::now();

        if cancellation.is_cancelled() {
            return Ok(empty_result(
                request,
                ProcessTermination::Cancelled,
                CleanupStatus::NotRequired,
                started,
            ));
        }

        let mut child = self.spawner.spawn(request)?;
        loop {
            if generation.ensure_current(&request.identity).is_err() {
                self.stop_and_collect(child, request, ProcessTermination::Cancelled, started)?;
                return Err(ProcessError::StaleGeneration);
            }
            if cancellation.is_cancelled() {
                return self.stop_and_collect(
                    child,
                    request,
                    ProcessTermination::Cancelled,
                    started,
                );
            }
            if started.elapsed() >= request.timeout {
                return self.stop_and_collect(
                    child,
                    request,
                    ProcessTermination::TimedOut,
                    started,
                );
            }
            if let Some(exit) = child.try_wait().map_err(ProcessError::Monitor)? {
                child
                    .seal_descendants(self.grace_period)
                    .map_err(|source| ProcessError::CleanupUncertain {
                        stage: "completed descendant seal",
                        source,
                    })?;
                let (stdout, stderr) = child.collect_output().map_err(ProcessError::Output)?;
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
}

impl<S: ProcessSpawner> ProcessSupervisor<S> {
    fn stop_and_collect(
        &self,
        mut child: Box<dyn ManagedProcess>,
        request: &ProcessRequest,
        termination: ProcessTermination,
        started: Instant,
    ) -> Result<ProcessResult, ProcessError> {
        let graceful = child.request_graceful_stop();
        if matches!(graceful, Ok(GracefulStop::Requested)) {
            let grace_started = Instant::now();
            while grace_started.elapsed() < self.grace_period {
                if let Some(exit) = child.try_wait().map_err(ProcessError::Monitor)? {
                    child
                        .seal_descendants(self.grace_period)
                        .map_err(|source| ProcessError::CleanupUncertain {
                            stage: "graceful descendant seal",
                            source,
                        })?;
                    let (stdout, stderr) = child.collect_output().map_err(ProcessError::Output)?;
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

        let exit =
            child
                .force_stop_and_wait()
                .map_err(|source| ProcessError::CleanupUncertain {
                    stage: "force stop",
                    source,
                })?;
        child
            .seal_descendants(self.grace_period)
            .map_err(|source| ProcessError::CleanupUncertain {
                stage: "forced descendant seal",
                source,
            })?;
        let (stdout, stderr) = child.collect_output().map_err(ProcessError::Output)?;
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
        stdout: CapturedStream {
            bytes: Vec::new(),
            truncated: false,
        },
        stderr: CapturedStream {
            bytes: Vec::new(),
            truncated: false,
        },
        elapsed_millis: started.elapsed().as_millis(),
    }
}

struct StdManagedProcess {
    child: Box<dyn ChildWrapper>,
    pid: u32,
    stdout: Option<JoinHandle<io::Result<CapturedStream>>>,
    stderr: Option<JoinHandle<io::Result<CapturedStream>>>,
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

    fn force_stop_and_wait(&mut self) -> io::Result<Option<ExitOutcome>> {
        match self.child.start_kill() {
            Ok(()) => {}
            Err(_) if self.try_wait()?.is_some() => {}
            Err(error) => return Err(error),
        }
        let exit = ExitOutcome::from(self.child.wait()?);
        self.last_exit = Some(exit);
        Ok(Some(exit))
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

    fn collect_output(mut self: Box<Self>) -> io::Result<(CapturedStream, CapturedStream)> {
        let stdout = join_reader(
            self.stdout
                .take()
                .ok_or_else(|| io::Error::other("stdout reader is missing"))?,
        )?;
        let stderr = join_reader(
            self.stderr
                .take()
                .ok_or_else(|| io::Error::other("stderr reader is missing"))?,
        )?;
        Ok((stdout, stderr))
    }
}

fn spawn_reader<R: Read + Send + 'static>(
    mut reader: R,
    limit: usize,
) -> JoinHandle<io::Result<CapturedStream>> {
    thread::spawn(move || {
        let mut captured = Vec::with_capacity(limit.min(8192));
        let mut buffer = [0_u8; 8192];
        let mut truncated = false;
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            let remaining = limit.saturating_sub(captured.len());
            let kept = remaining.min(read);
            captured.extend_from_slice(&buffer[..kept]);
            truncated |= kept < read;
        }
        Ok(CapturedStream {
            bytes: captured,
            truncated,
        })
    })
}

fn join_reader(reader: JoinHandle<io::Result<CapturedStream>>) -> io::Result<CapturedStream> {
    reader
        .join()
        .map_err(|_| io::Error::other("output reader thread panicked"))?
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
            Self::InvalidRequest(_) | Self::StaleGeneration | Self::Invariant(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    }

    #[derive(Clone)]
    enum FakeBehavior {
        Complete,
        Cancel(CancellationToken),
        NeverExit,
        Stale(GenerationGuard),
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
    }

    struct FakeProcess {
        state: Arc<Mutex<FakeState>>,
        behavior: FakeBehavior,
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

        fn force_stop_and_wait(&mut self) -> io::Result<Option<ExitOutcome>> {
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

        fn collect_output(self: Box<Self>) -> io::Result<(CapturedStream, CapturedStream)> {
            let stream = CapturedStream {
                bytes: b"fixture".to_vec(),
                truncated: false,
            };
            Ok((stream.clone(), stream))
        }
    }

    fn supervisor(
        behavior: FakeBehavior,
    ) -> (ProcessSupervisor<FakeSpawner>, Arc<Mutex<FakeState>>) {
        let state = Arc::new(Mutex::new(FakeState::default()));
        (
            ProcessSupervisor::new(FakeSpawner {
                state: Arc::clone(&state),
                behavior,
            })
            .with_timing(Duration::from_millis(1), Duration::from_millis(1)),
            state,
        )
    }

    #[test]
    fn completed_process_is_sealed_before_acceptance() {
        let (supervisor, state) = supervisor(FakeBehavior::Complete);
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
        let (supervisor, state) = supervisor(FakeBehavior::NeverExit);
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
        let (supervisor, state) = supervisor(FakeBehavior::NeverExit);
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
        let (supervisor, state) = supervisor(FakeBehavior::Cancel(cancellation.clone()));
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
        let (supervisor, state) = supervisor(FakeBehavior::Stale(guard.clone()));

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
        let (supervisor, _) = supervisor(FakeBehavior::CleanupFailure);
        let mut request = request();
        request.timeout = Duration::from_millis(2);
        let guard = GenerationGuard::new(request.identity.clone());

        let error = supervisor
            .execute(&request, &CancellationToken::default(), &guard)
            .expect_err("cleanup failure must fail closed");

        assert!(matches!(error, ProcessError::CleanupUncertain { .. }));
    }

    #[test]
    fn invalid_request_is_rejected_before_spawn() {
        let (supervisor, state) = supervisor(FakeBehavior::Complete);
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
}
