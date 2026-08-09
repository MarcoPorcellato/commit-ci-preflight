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
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::config::{ExecutionPlanEnvelopeV1, NormalizedCheck, NormalizedRuntime, RuntimeKind};
use crate::process::{
    CancellationToken, CleanupStatus, GenerationGuard, ProcessError, ProcessRequest, ProcessResult,
    ProcessTermination, RunIdentity, SupervisorPort,
};
use crate::workspace::{MountAccess, WorkspaceError, WorkspacePlanV1, validate_host_path};

const DOCTOR_TIMEOUT: Duration = Duration::from_secs(5);
const DOCTOR_CAPTURE_BYTES: usize = 65_536;

pub trait RuntimePort: Send + Sync {
    fn kind(&self) -> RuntimeKind;

    fn probe(
        &self,
        envelope: &ExecutionPlanEnvelopeV1,
        supervisor: &dyn SupervisorPort,
        current_dir: &Path,
        cancellation: &CancellationToken,
        generation: &GenerationGuard,
    ) -> Result<RuntimeProbe, RuntimeError>;

    fn dry_run(
        &self,
        envelope: &ExecutionPlanEnvelopeV1,
        workspace: &WorkspacePlanV1,
    ) -> Result<DryRunPlan, RuntimeError>;
}

pub fn runtime_for(kind: RuntimeKind) -> Result<Box<dyn RuntimePort>, RuntimeError> {
    match kind {
        RuntimeKind::DockerCompatible => Ok(Box::new(DockerCompatibleRuntime)),
        RuntimeKind::Host => Err(RuntimeError::Unsupported(
            "host runtime execution is not qualified in receipt v1",
        )),
    }
}

#[derive(Debug, Default)]
pub struct DockerCompatibleRuntime;

impl RuntimePort for DockerCompatibleRuntime {
    fn kind(&self) -> RuntimeKind {
        RuntimeKind::DockerCompatible
    }

    fn probe(
        &self,
        envelope: &ExecutionPlanEnvelopeV1,
        supervisor: &dyn SupervisorPort,
        current_dir: &Path,
        cancellation: &CancellationToken,
        generation: &GenerationGuard,
    ) -> Result<RuntimeProbe, RuntimeError> {
        let identity = doctor_identity(envelope);
        generation
            .ensure_current(&identity)
            .map_err(RuntimeError::Process)?;
        let request = ProcessRequest {
            identity,
            program: OsString::from("docker"),
            argv: vec![
                OsString::from("info"),
                OsString::from("--format"),
                OsString::from("{{json .}}"),
            ],
            current_dir: current_dir.to_path_buf(),
            environment: runtime_environment(),
            timeout: DOCTOR_TIMEOUT,
            max_capture_bytes: DOCTOR_CAPTURE_BYTES,
        };
        let result = supervisor
            .execute(&request, cancellation, generation)
            .map_err(|error| match error {
                ProcessError::Spawn(_) => RuntimeError::Unavailable,
                error => RuntimeError::Process(error),
            })?;
        interpret_docker_probe(result)
    }

    fn dry_run(
        &self,
        envelope: &ExecutionPlanEnvelopeV1,
        workspace: &WorkspacePlanV1,
    ) -> Result<DryRunPlan, RuntimeError> {
        let checks = envelope
            .plan
            .checks
            .iter()
            .map(|check| {
                docker_dry_run_check(
                    &envelope.plan.runtime,
                    check,
                    &envelope.plan.environment_allow,
                    workspace,
                )
            })
            .collect::<Result<_, _>>()?;
        Ok(DryRunPlan {
            schema_version: "1.0",
            plan_digest: envelope.plan_digest.clone(),
            runtime: RuntimeKind::DockerCompatible,
            program: "docker",
            checks,
            workspace: workspace.clone(),
            workspace_mount_policy: WorkspaceMountPolicy::ExplicitBindings,
            executed: false,
        })
    }
}

fn doctor_identity(envelope: &ExecutionPlanEnvelopeV1) -> RunIdentity {
    RunIdentity {
        project: envelope.plan.project.clone(),
        commit: None,
        config_digest: envelope.plan_digest.clone(),
        generation: "doctor-v1".to_owned(),
    }
}

pub fn doctor_guard(envelope: &ExecutionPlanEnvelopeV1) -> GenerationGuard {
    GenerationGuard::new(doctor_identity(envelope))
}

fn interpret_docker_probe(result: ProcessResult) -> Result<RuntimeProbe, RuntimeError> {
    match result.termination {
        ProcessTermination::TimedOut => return Err(RuntimeError::TimedOut),
        ProcessTermination::Cancelled => return Err(RuntimeError::Cancelled),
        ProcessTermination::Completed => {}
    }
    if result.cleanup != CleanupStatus::Verified {
        return Err(RuntimeError::CleanupUncertain);
    }
    let exit = result.exit.ok_or(RuntimeError::InvalidProbe(
        "runtime probe completed without an exit status",
    ))?;
    if !exit.success {
        return Err(RuntimeError::Unavailable);
    }
    if result.stdout.truncated || result.stderr.truncated {
        return Err(RuntimeError::InvalidProbe(
            "runtime probe output exceeded the capture limit",
        ));
    }
    let output = std::str::from_utf8(&result.stdout.bytes)
        .map_err(|_| RuntimeError::InvalidProbe("runtime probe output is not UTF-8"))?;
    let value: Value = serde_json::from_str(output.trim())
        .map_err(|_| RuntimeError::InvalidProbe("runtime probe output is not valid JSON"))?;
    if !value.is_object() {
        return Err(RuntimeError::InvalidProbe(
            "runtime probe output is not a JSON object",
        ));
    }

    let operating_system = bounded_field(&value, "OperatingSystem");
    let server_version = bounded_field(&value, "ServerVersion").ok_or(
        RuntimeError::InvalidProbe("runtime probe did not report a server version"),
    )?;
    let os_type = bounded_field(&value, "OSType").ok_or(RuntimeError::InvalidProbe(
        "runtime probe did not report an OS type",
    ))?;
    if !os_type.eq_ignore_ascii_case("linux") {
        return Err(RuntimeError::Unsupported(
            "the Docker-compatible runtime is not serving Linux containers",
        ));
    }
    let private_name = bounded_field(&value, "Name");
    let is_orbstack = operating_system
        .iter()
        .chain(private_name.iter())
        .any(|field| field.to_ascii_lowercase().contains("orbstack"));

    Ok(RuntimeProbe {
        runtime: RuntimeKind::DockerCompatible,
        flavor: if is_orbstack {
            RuntimeFlavor::OrbStack
        } else {
            RuntimeFlavor::DockerCompatible
        },
        server_version: Some(server_version),
        operating_system,
        os_type: Some(os_type),
        containment: containment_mechanism(),
        graceful_stop: graceful_stop_capability(),
    })
}

fn bounded_field(value: &Value, key: &str) -> Option<String> {
    let field = value.get(key)?.as_str()?.trim();
    if field.is_empty() || field.len() > 128 || field.chars().any(char::is_control) {
        None
    } else {
        Some(field.to_owned())
    }
}

fn runtime_environment() -> BTreeMap<OsString, OsString> {
    const ALLOWED: &[&str] = &[
        "PATH",
        "HOME",
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "DOCKER_HOST",
        "DOCKER_CONTEXT",
        "DOCKER_CONFIG",
    ];
    ALLOWED
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|value| (OsString::from(name), value)))
        .collect()
}

pub fn docker_execution_environment(allowed_names: &[String]) -> BTreeMap<OsString, OsString> {
    let mut environment = runtime_environment();
    for name in allowed_names {
        if let Some(value) = std::env::var_os(name) {
            environment.insert(OsString::from(name), value);
        }
    }
    environment
}

fn docker_dry_run_check(
    runtime: &NormalizedRuntime,
    check: &NormalizedCheck,
    environment_allow: &[String],
    workspace: &WorkspacePlanV1,
) -> Result<DryRunCheck, RuntimeError> {
    let mut argv = vec![
        "run".to_owned(),
        "--rm".to_owned(),
        "--init".to_owned(),
        "--read-only".to_owned(),
        "--network".to_owned(),
        if runtime.network {
            "bridge".to_owned()
        } else {
            "none".to_owned()
        },
        "--cpus".to_owned(),
        runtime.cpu_count.to_string(),
        "--memory".to_owned(),
        format!("{}m", runtime.memory_mib),
        "--pids-limit".to_owned(),
        runtime.pids_limit.to_string(),
        "--tmpfs".to_owned(),
        "/tmp:rw,noexec,nosuid,nodev,size=64m".to_owned(),
    ];
    for name in environment_allow {
        argv.push("--env".to_owned());
        argv.push(name.clone());
    }
    argv.push("--env".to_owned());
    argv.push("TMPDIR=/tmp".to_owned());
    for mount in &workspace.mounts {
        argv.push("--mount".to_owned());
        argv.push(docker_mount_argument(mount)?);
    }
    argv.extend([
        "--workdir".to_owned(),
        container_working_directory(&check.working_directory),
    ]);
    for name in &check.argv {
        if name.contains('\0') {
            unreachable!("normalized configuration rejects NUL characters");
        }
    }
    argv.push(runtime.image.clone());
    argv.extend(check.argv.clone());
    Ok(DryRunCheck {
        id: check.id.clone(),
        program: "docker",
        argv,
        depends_on: check.depends_on.clone(),
    })
}

fn docker_mount_argument(mount: &crate::workspace::MountBinding) -> Result<String, RuntimeError> {
    validate_host_path(&mount.source).map_err(RuntimeError::Workspace)?;
    let source = mount
        .source
        .to_str()
        .ok_or(RuntimeError::Workspace(WorkspaceError::UnsupportedHostPath))?;
    let readonly = if mount.access == MountAccess::ReadOnly {
        ",readonly"
    } else {
        ""
    };
    Ok(format!(
        "type=bind,src={source},dst={}{}",
        mount.target, readonly
    ))
}

fn container_working_directory(relative: &str) -> String {
    if relative == "." {
        "/workspace".to_owned()
    } else {
        format!("/workspace/{relative}")
    }
}

#[cfg(unix)]
fn containment_mechanism() -> ContainmentMechanism {
    ContainmentMechanism::ProcessGroup
}

#[cfg(windows)]
fn containment_mechanism() -> ContainmentMechanism {
    ContainmentMechanism::JobObject
}

#[cfg(unix)]
fn graceful_stop_capability() -> GracefulStopCapability {
    GracefulStopCapability::ProcessGroupSignal
}

#[cfg(windows)]
fn graceful_stop_capability() -> GracefulStopCapability {
    GracefulStopCapability::HardStopOnly
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFlavor {
    DockerCompatible,
    OrbStack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainmentMechanism {
    ProcessGroup,
    JobObject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GracefulStopCapability {
    ProcessGroupSignal,
    HardStopOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeProbe {
    pub runtime: RuntimeKind,
    pub flavor: RuntimeFlavor,
    pub server_version: Option<String>,
    pub operating_system: Option<String>,
    pub os_type: Option<String>,
    pub containment: ContainmentMechanism,
    pub graceful_stop: GracefulStopCapability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DryRunPlan {
    pub schema_version: &'static str,
    pub plan_digest: String,
    pub runtime: RuntimeKind,
    pub program: &'static str,
    pub checks: Vec<DryRunCheck>,
    pub workspace: WorkspacePlanV1,
    pub workspace_mount_policy: WorkspaceMountPolicy,
    pub executed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DryRunCheck {
    pub id: String,
    pub program: &'static str,
    pub argv: Vec<String>,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMountPolicy {
    ExplicitBindings,
}

#[derive(Debug)]
pub enum RuntimeError {
    Unsupported(&'static str),
    Unavailable,
    TimedOut,
    Cancelled,
    CleanupUncertain,
    InvalidProbe(&'static str),
    Workspace(WorkspaceError),
    Process(ProcessError),
}

impl RuntimeError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Unsupported(_) | Self::Unavailable | Self::InvalidProbe(_) => 4,
            Self::TimedOut | Self::Cancelled => 5,
            Self::Workspace(_) => 2,
            Self::CleanupUncertain | Self::Process(ProcessError::CleanupUncertain { .. }) => 70,
            Self::Process(ProcessError::StaleGeneration) => 5,
            Self::Process(_) => 70,
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(message) => write!(formatter, "unsupported runtime: {message}"),
            Self::Unavailable => formatter.write_str("Docker-compatible runtime is unavailable"),
            Self::TimedOut => formatter.write_str("runtime probe exceeded its deadline"),
            Self::Cancelled => formatter.write_str("runtime probe was cancelled"),
            Self::CleanupUncertain => formatter.write_str("runtime probe cleanup is uncertain"),
            Self::InvalidProbe(message) => write!(formatter, "invalid runtime probe: {message}"),
            Self::Workspace(error) => write!(formatter, "invalid workspace plan: {error}"),
            Self::Process(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Workspace(error) => Some(error),
            Self::Process(error) => Some(error),
            Self::Unsupported(_)
            | Self::Unavailable
            | Self::TimedOut
            | Self::Cancelled
            | Self::CleanupUncertain
            | Self::InvalidProbe(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{CacheRootSource, ResolvedCacheRoot};
    use crate::config::ConfigV1;
    use crate::process::{CapturedStream, ExitOutcome};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const CONFIG: &str = r#"
schema_version = "1.0"
project = "owner/repository"

[runtime]
kind = "docker_compatible"
image = "ghcr.io/example/ci@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
cpu_count = 4
memory_mib = 8192
pids_limit = 512
network = false

[[checks]]
id = "test"
required = true
argv = ["cargo", "test", "--locked"]
working_directory = "."
timeout_seconds = 60
"#;

    fn envelope() -> ExecutionPlanEnvelopeV1 {
        ConfigV1::parse(CONFIG)
            .expect("config")
            .into_plan()
            .expect("plan")
    }

    fn workspace(envelope: &ExecutionPlanEnvelopeV1) -> WorkspacePlanV1 {
        let repository = std::env::current_dir().expect("current dir");
        let cache = ResolvedCacheRoot {
            path: std::env::var_os("CCP_TEST_ROOT")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| {
                    repository
                        .parent()
                        .expect("repository parent")
                        .to_path_buf()
                })
                .join(format!(".ccp-runtime-test-cache-{}", std::process::id())),
            source: CacheRootSource::Explicit,
        };
        WorkspacePlanV1::build(envelope, &repository, &cache).expect("workspace")
    }

    #[derive(Clone, Copy)]
    enum ProbeMode {
        OrbStack,
        NonZero,
        SpawnFailure,
        TimedOut,
        Truncated,
        Incomplete,
    }

    struct FakeSupervisor {
        mode: ProbeMode,
        calls: AtomicUsize,
    }

    impl FakeSupervisor {
        fn new(mode: ProbeMode) -> Self {
            Self {
                mode,
                calls: AtomicUsize::new(0),
            }
        }
    }

    impl SupervisorPort for FakeSupervisor {
        fn execute(
            &self,
            request: &ProcessRequest,
            _cancellation: &CancellationToken,
            generation: &GenerationGuard,
        ) -> Result<ProcessResult, ProcessError> {
            generation.ensure_current(&request.identity)?;
            self.calls.fetch_add(1, Ordering::SeqCst);
            if matches!(self.mode, ProbeMode::SpawnFailure) {
                return Err(ProcessError::Spawn(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "fixture runtime is absent",
                )));
            }
            let (termination, success, stdout, truncated) = match self.mode {
                ProbeMode::OrbStack => (
                    ProcessTermination::Completed,
                    true,
                    br#"{"OperatingSystem":"OrbStack","ServerVersion":"28.3.2","OSType":"linux","Name":"private-machine"}"#.to_vec(),
                    false,
                ),
                ProbeMode::NonZero => (
                    ProcessTermination::Completed,
                    false,
                    Vec::new(),
                    false,
                ),
                ProbeMode::TimedOut => (
                    ProcessTermination::TimedOut,
                    false,
                    Vec::new(),
                    false,
                ),
                ProbeMode::Truncated => (
                    ProcessTermination::Completed,
                    true,
                    b"{}".to_vec(),
                    true,
                ),
                ProbeMode::Incomplete => (
                    ProcessTermination::Completed,
                    true,
                    b"{}".to_vec(),
                    false,
                ),
                ProbeMode::SpawnFailure => unreachable!("handled before result construction"),
            };
            Ok(ProcessResult {
                identity: request.identity.clone(),
                termination,
                cleanup: CleanupStatus::Verified,
                exit: Some(ExitOutcome {
                    success,
                    code: Some(if success { 0 } else { 1 }),
                }),
                stdout: CapturedStream {
                    bytes: stdout,
                    truncated,
                },
                stderr: CapturedStream {
                    bytes: Vec::new(),
                    truncated: false,
                },
                elapsed_millis: 1,
            })
        }
    }

    #[test]
    fn probe_qualifies_orbstack_without_exposing_machine_name() {
        let envelope = envelope();
        let supervisor = FakeSupervisor::new(ProbeMode::OrbStack);
        let guard = doctor_guard(&envelope);
        let probe = DockerCompatibleRuntime
            .probe(
                &envelope,
                &supervisor,
                &std::env::current_dir().expect("current dir"),
                &CancellationToken::default(),
                &guard,
            )
            .expect("probe");

        assert_eq!(probe.flavor, RuntimeFlavor::OrbStack);
        assert_eq!(probe.server_version.as_deref(), Some("28.3.2"));
        let json = serde_json::to_string(&probe).expect("serialize");
        assert!(!json.contains("private-machine"));
    }

    #[test]
    fn nonzero_probe_is_unavailable_without_raw_stderr() {
        let envelope = envelope();
        let supervisor = FakeSupervisor::new(ProbeMode::NonZero);
        let guard = doctor_guard(&envelope);
        let error = DockerCompatibleRuntime
            .probe(
                &envelope,
                &supervisor,
                &std::env::current_dir().expect("current dir"),
                &CancellationToken::default(),
                &guard,
            )
            .expect_err("unavailable");

        assert!(matches!(error, RuntimeError::Unavailable));
        assert_eq!(error.exit_code(), 4);
    }

    #[test]
    fn timeout_and_truncation_fail_closed() {
        for (mode, expected_code) in [
            (ProbeMode::TimedOut, 5),
            (ProbeMode::Truncated, 4),
            (ProbeMode::Incomplete, 4),
        ] {
            let envelope = envelope();
            let supervisor = FakeSupervisor::new(mode);
            let guard = doctor_guard(&envelope);
            let error = DockerCompatibleRuntime
                .probe(
                    &envelope,
                    &supervisor,
                    &std::env::current_dir().expect("current dir"),
                    &CancellationToken::default(),
                    &guard,
                )
                .expect_err("probe fails closed");
            assert_eq!(error.exit_code(), expected_code);
        }
    }

    #[test]
    fn missing_runtime_is_classified_as_unavailable() {
        let envelope = envelope();
        let supervisor = FakeSupervisor::new(ProbeMode::SpawnFailure);
        let guard = doctor_guard(&envelope);
        let error = DockerCompatibleRuntime
            .probe(
                &envelope,
                &supervisor,
                &std::env::current_dir().expect("current dir"),
                &CancellationToken::default(),
                &guard,
            )
            .expect_err("missing runtime");

        assert!(matches!(error, RuntimeError::Unavailable));
        assert_eq!(error.exit_code(), 4);
    }

    #[test]
    fn dry_run_is_deterministic_shell_free_and_non_executing() {
        let envelope = envelope();
        let workspace = workspace(&envelope);
        let first = DockerCompatibleRuntime
            .dry_run(&envelope, &workspace)
            .expect("dry run");
        let second = DockerCompatibleRuntime
            .dry_run(&envelope, &workspace)
            .expect("dry run replay");

        assert_eq!(first, second);
        assert!(!first.executed);
        assert_eq!(
            first.workspace_mount_policy,
            WorkspaceMountPolicy::ExplicitBindings
        );
        assert_eq!(first.checks[0].program, "docker");
        assert!(
            first.checks[0]
                .argv
                .windows(2)
                .any(|pair| pair == ["--network", "none"])
        );
        assert!(first.checks[0].argv.contains(&"--init".to_owned()));
        assert!(first.checks[0].argv.contains(&"--read-only".to_owned()));
        assert!(
            first.checks[0]
                .argv
                .windows(2)
                .any(|pair| { pair == ["--tmpfs", "/tmp:rw,noexec,nosuid,nodev,size=64m"] })
        );
        assert!(
            first.checks[0]
                .argv
                .windows(2)
                .any(|pair| pair == ["--env", "TMPDIR=/tmp"])
        );
        assert!(
            !first.checks[0]
                .argv
                .iter()
                .any(|part| part.contains("dst=/tmp"))
        );
        assert!(first.checks[0].argv.contains(&"--mount".to_owned()));
        assert!(
            first.checks[0]
                .argv
                .iter()
                .any(|part| part.ends_with("dst=/workspace,readonly"))
        );
        assert!(
            !first.checks[0]
                .argv
                .iter()
                .any(|part| part == "sh" || part == "-c")
        );
    }

    #[test]
    fn dry_run_exposes_only_allowlisted_environment_names() {
        let envelope = ConfigV1::parse(&CONFIG.replace(
            "[[checks]]",
            "[environment]\nallow = [\"SAFE_TOKEN\", \"TMPDIR\"]\n\n[[checks]]",
        ))
        .expect("config")
        .into_plan()
        .expect("plan");
        let workspace = workspace(&envelope);
        let dry_run = DockerCompatibleRuntime
            .dry_run(&envelope, &workspace)
            .expect("dry run");
        let argv = &dry_run.checks[0].argv;
        assert!(argv.windows(2).any(|pair| pair == ["--env", "SAFE_TOKEN"]));
        let tmpdir_values: Vec<_> = argv
            .windows(2)
            .filter(|pair| pair[0] == "--env" && pair[1].starts_with("TMPDIR"))
            .map(|pair| pair[1].as_str())
            .collect();
        assert_eq!(tmpdir_values.last(), Some(&"TMPDIR=/tmp"));
        assert!(!argv.iter().any(|part| part.contains("secret-value")));
    }

    #[test]
    fn host_runtime_is_explicitly_unsupported() {
        let error = runtime_for(RuntimeKind::Host)
            .err()
            .expect("host runtime must remain unqualified");
        assert!(matches!(error, RuntimeError::Unsupported(_)));
        assert_eq!(error.exit_code(), 4);
    }
}
