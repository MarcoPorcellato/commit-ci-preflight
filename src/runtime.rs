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

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::config::{
    ExecutionPlanEnvelopeV1, NormalizedCheck, NormalizedRuntime, RuntimeKind, RuntimePullPolicy,
    RuntimeSwapMode,
};
use crate::process::{
    CancellationToken, CleanupStatus, GenerationGuard, ProcessError, ProcessRequest, ProcessResult,
    ProcessTermination, RunIdentity, SupervisorPort,
};
use crate::receipt::canonical_digest;
use crate::workspace::{
    MountAccess, WorkspaceError, WorkspacePlanV1, validate_container_mount_target,
    validate_host_path,
};

const DOCTOR_TIMEOUT: Duration = Duration::from_secs(5);
const DOCTOR_CAPTURE_BYTES: usize = 65_536;
const LIFECYCLE_CAPTURE_BYTES: usize = 1_048_576;

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

    fn execute_check(
        &self,
        _check: &NormalizedCheck,
        _rendered: &DryRunCheck,
        _context: &RuntimeExecutionContext<'_>,
    ) -> Result<ProcessResult, RuntimeError> {
        Err(RuntimeError::Unsupported(
            "runtime lifecycle execution is not qualified",
        ))
    }
}

pub trait RuntimeCapabilityProbe: Send + Sync {
    fn probe(
        &self,
        envelope: &ExecutionPlanEnvelopeV1,
        runtime_probe: &RuntimeProbe,
        supervisor: &dyn SupervisorPort,
        current_dir: &Path,
        cancellation: &CancellationToken,
        generation: &GenerationGuard,
    ) -> Result<Option<RuntimeCapabilityEvidenceV1>, RuntimeError>;
}

#[derive(Debug, Default)]
pub struct DockerRuntimeCapabilityProbe;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePreflight {
    pub runtime_probe: Option<RuntimeProbe>,
    pub capability_evidence: Option<RuntimeCapabilityEvidenceV1>,
}

pub fn preflight_runtime_capabilities(
    envelope: &ExecutionPlanEnvelopeV1,
    runtime: &dyn RuntimePort,
    capability_probe: &dyn RuntimeCapabilityProbe,
    supervisor: &dyn SupervisorPort,
    current_dir: &Path,
    cancellation: &CancellationToken,
    generation: &GenerationGuard,
) -> Result<RuntimePreflight, RuntimeError> {
    if envelope.plan.schema_version != "1.3" {
        return Ok(RuntimePreflight {
            runtime_probe: None,
            capability_evidence: None,
        });
    }
    let runtime_probe =
        runtime.probe(envelope, supervisor, current_dir, cancellation, generation)?;
    let capability_evidence = capability_probe.probe(
        envelope,
        &runtime_probe,
        supervisor,
        current_dir,
        cancellation,
        generation,
    )?;
    Ok(RuntimePreflight {
        runtime_probe: Some(runtime_probe),
        capability_evidence,
    })
}

pub struct RuntimeExecutionContext<'a> {
    pub execution_root: &'a Path,
    pub environment: &'a BTreeMap<OsString, OsString>,
    pub identity: &'a RunIdentity,
    pub run_id: &'a str,
    pub supervisor: &'a dyn SupervisorPort,
    pub cancellation: &'a CancellationToken,
    pub generation: &'a GenerationGuard,
    pub timeout_seconds: u64,
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
        validate_fixed_environment(envelope)?;
        let checks = envelope
            .plan
            .checks
            .iter()
            .map(|check| {
                docker_dry_run_check(
                    &envelope.plan.runtime,
                    check,
                    &envelope.plan.environment.names(),
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

    fn execute_check(
        &self,
        check: &NormalizedCheck,
        rendered: &DryRunCheck,
        context: &RuntimeExecutionContext<'_>,
    ) -> Result<ProcessResult, RuntimeError> {
        let lifecycle = DockerLifecyclePlan::build(context.run_id, check, rendered)?;
        let cleanup_cancellation = CancellationToken::default();
        let create = self.execute_cli(&lifecycle.create_argv, context, context.cancellation)?;
        if !completed_success(&create) {
            return Ok(create);
        }
        if !valid_container_id(&create.stdout.bytes) {
            self.cleanup_container(&lifecycle, context, &cleanup_cancellation)?;
            return Err(RuntimeError::LifecycleFailure(
                "docker create did not return a valid container id",
            ));
        }

        let start = self.execute_cli(&lifecycle.start_argv, context, context.cancellation);
        let start = match start {
            Ok(result) => result,
            Err(error) => {
                self.cleanup_container(&lifecycle, context, &cleanup_cancellation)?;
                return Err(error);
            }
        };
        if !completed_success(&start) {
            self.cleanup_container(&lifecycle, context, &cleanup_cancellation)?;
            return Ok(start);
        }

        let attach = match self.execute_cli(&lifecycle.attach_argv, context, context.cancellation) {
            Ok(result) => result,
            Err(error) => {
                self.cleanup_container(&lifecycle, context, &cleanup_cancellation)?;
                return Err(error);
            }
        };
        if attach.termination != ProcessTermination::Completed {
            self.cleanup_container(&lifecycle, context, &cleanup_cancellation)?;
            return Ok(attach);
        }

        let wait = self.execute_cli(&lifecycle.wait_argv, context, context.cancellation)?;
        if !completed_success(&wait) {
            self.cleanup_container(&lifecycle, context, &cleanup_cancellation)?;
            return Err(RuntimeError::LifecycleFailure(
                "docker wait did not return the container exit code",
            ));
        }
        let exit_code = parse_wait_exit_code(&wait.stdout.bytes)?;
        self.cleanup_container(&lifecycle, context, &cleanup_cancellation)?;
        let mut result = attach;
        result.exit = Some(crate::process::ExitOutcome {
            success: exit_code == 0,
            code: Some(exit_code),
        });
        Ok(result)
    }
}

impl DockerCompatibleRuntime {
    fn execute_cli(
        &self,
        argv: &[String],
        context: &RuntimeExecutionContext<'_>,
        cancellation: &CancellationToken,
    ) -> Result<ProcessResult, RuntimeError> {
        let request = ProcessRequest {
            identity: context.identity.clone(),
            program: OsString::from("docker"),
            argv: argv.iter().map(OsString::from).collect(),
            current_dir: context.execution_root.to_path_buf(),
            environment: context.environment.clone(),
            timeout: Duration::from_secs(context.timeout_seconds),
            max_capture_bytes: LIFECYCLE_CAPTURE_BYTES,
        };
        context
            .supervisor
            .execute(&request, cancellation, context.generation)
            .map_err(RuntimeError::Process)
    }

    fn execute_cleanup_cli(
        &self,
        argv: &[String],
        context: &RuntimeExecutionContext<'_>,
        cancellation: &CancellationToken,
    ) -> Result<ProcessResult, RuntimeError> {
        self.execute_cli(argv, context, cancellation)
            .map_err(|_| RuntimeError::LifecycleFailure("daemon cleanup command failed"))
    }

    fn cleanup_container(
        &self,
        lifecycle: &DockerLifecyclePlan,
        context: &RuntimeExecutionContext<'_>,
        cancellation: &CancellationToken,
    ) -> Result<(), RuntimeError> {
        let inspect = self.execute_cleanup_cli(&lifecycle.inspect_argv, context, cancellation)?;
        if !completed_success(&inspect) {
            return Err(RuntimeError::LifecycleFailure(
                "daemon inspection failed before cleanup",
            ));
        }
        let state = parse_container_state(&inspect.stdout.bytes, lifecycle)?;
        if matches!(
            state.as_str(),
            "created" | "running" | "restarting" | "paused"
        ) {
            let stop = self.execute_cleanup_cli(&lifecycle.stop_argv, context, cancellation);
            if !stop.as_ref().is_ok_and(completed_success) {
                let kill = self.execute_cleanup_cli(&lifecycle.kill_argv, context, cancellation)?;
                if !completed_success(&kill) {
                    return Err(RuntimeError::LifecycleFailure(
                        "daemon stop and kill both failed",
                    ));
                }
            }
        }
        let remove = self.execute_cleanup_cli(&lifecycle.remove_argv, context, cancellation)?;
        if !completed_success(&remove) {
            return Err(RuntimeError::LifecycleFailure("daemon removal failed"));
        }
        let final_inspect =
            self.execute_cleanup_cli(&lifecycle.inspect_argv, context, cancellation)?;
        if final_inspect.termination != ProcessTermination::Completed
            || final_inspect.cleanup != CleanupStatus::Verified
            || final_inspect.exit.is_none_or(|exit| exit.success)
        {
            return Err(RuntimeError::LifecycleFailure(
                "daemon removal was not proven by final not-found inspection",
            ));
        }
        Ok(())
    }
}

impl RuntimeCapabilityProbe for DockerRuntimeCapabilityProbe {
    fn probe(
        &self,
        envelope: &ExecutionPlanEnvelopeV1,
        runtime_probe: &RuntimeProbe,
        supervisor: &dyn SupervisorPort,
        current_dir: &Path,
        cancellation: &CancellationToken,
        generation: &GenerationGuard,
    ) -> Result<Option<RuntimeCapabilityEvidenceV1>, RuntimeError> {
        if envelope.plan.schema_version != "1.3" {
            return Ok(None);
        }
        if runtime_probe.runtime != RuntimeKind::DockerCompatible {
            return Err(RuntimeError::Unsupported(
                "schema 1.3 requires a Docker-compatible runtime",
            ));
        }
        let context = execute_capability_command(
            envelope,
            supervisor,
            current_dir,
            cancellation,
            generation,
            vec![OsString::from("context"), OsString::from("show")],
        )?;
        let image = execute_capability_command(
            envelope,
            supervisor,
            current_dir,
            cancellation,
            generation,
            vec![
                OsString::from("image"),
                OsString::from("inspect"),
                OsString::from("--format"),
                OsString::from("{{json .}}"),
                OsString::from(&envelope.plan.runtime.image),
            ],
        )?;
        let context = bounded_context(&context)?;
        interpret_runtime_capabilities(runtime_probe, context, &image, &envelope.plan.runtime.image)
            .map(Some)
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
        memory_limit_supported: value.get("MemoryLimit").and_then(Value::as_bool),
        swap_limit_supported: value.get("SwapLimit").and_then(Value::as_bool),
        containment: containment_mechanism(),
        graceful_stop: graceful_stop_capability(),
    })
}

fn execute_capability_command(
    envelope: &ExecutionPlanEnvelopeV1,
    supervisor: &dyn SupervisorPort,
    current_dir: &Path,
    cancellation: &CancellationToken,
    generation: &GenerationGuard,
    argv: Vec<OsString>,
) -> Result<Vec<u8>, RuntimeError> {
    let identity = doctor_identity(envelope);
    generation
        .ensure_current(&identity)
        .map_err(RuntimeError::Process)?;
    let request = ProcessRequest {
        identity,
        program: OsString::from("docker"),
        argv,
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
    bounded_successful_output(result)
}

fn bounded_successful_output(result: ProcessResult) -> Result<Vec<u8>, RuntimeError> {
    match result.termination {
        ProcessTermination::TimedOut => return Err(RuntimeError::TimedOut),
        ProcessTermination::Cancelled => return Err(RuntimeError::Cancelled),
        ProcessTermination::Completed => {}
    }
    if result.cleanup != CleanupStatus::Verified {
        return Err(RuntimeError::CleanupUncertain);
    }
    if result.exit.map(|exit| exit.success) != Some(true) {
        return Err(RuntimeError::Unavailable);
    }
    if result.stdout.truncated || result.stderr.truncated {
        return Err(RuntimeError::InvalidProbe(
            "runtime capability output exceeded the capture limit",
        ));
    }
    Ok(result.stdout.bytes)
}

fn bounded_context(bytes: &[u8]) -> Result<&str, RuntimeError> {
    let context = std::str::from_utf8(bytes)
        .map_err(|_| RuntimeError::InvalidProbe("Docker context output is not UTF-8"))?
        .trim();
    if context.is_empty() || context.len() > 128 || context.chars().any(char::is_control) {
        return Err(RuntimeError::InvalidProbe(
            "Docker context output is unsafe or out of bounds",
        ));
    }
    Ok(context)
}

fn interpret_runtime_capabilities(
    runtime_probe: &RuntimeProbe,
    context: &str,
    image: &[u8],
    expected_image_reference: &str,
) -> Result<RuntimeCapabilityEvidenceV1, RuntimeError> {
    if runtime_probe.memory_limit_supported != Some(true) {
        return Err(RuntimeError::UnsupportedCapability("memory_limit"));
    }
    if runtime_probe.swap_limit_supported != Some(true) {
        return Err(RuntimeError::UnsupportedCapability("swap_limit"));
    }
    let value: Value = serde_json::from_slice(image)
        .map_err(|_| RuntimeError::InvalidProbe("image inspect output is not valid JSON"))?;
    let image_id = value
        .get("Id")
        .and_then(Value::as_str)
        .filter(|value| valid_sha256_digest(value))
        .ok_or(RuntimeError::InvalidProbe(
            "image inspect output omitted a canonical image ID",
        ))?;
    let has_configured_reference = value
        .get("RepoDigests")
        .and_then(Value::as_array)
        .is_some_and(|digests| {
            digests
                .iter()
                .filter_map(Value::as_str)
                .any(|digest| digest == expected_image_reference)
        });
    if !has_configured_reference {
        return Err(RuntimeError::InvalidProbe(
            "image inspect output did not bind the configured image reference",
        ));
    }
    Ok(RuntimeCapabilityEvidenceV1 {
        schema_version: "1.0".to_owned(),
        memory_limit_supported: true,
        swap_limit_supported: true,
        context_digest: canonical_digest(&context)
            .map_err(|_| RuntimeError::InvalidProbe("cannot digest Docker context"))?,
        resolved_image_id: image_id.to_owned(),
        resolved_image_reference: expected_image_reference.to_owned(),
    })
}

fn valid_sha256_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value
            .as_bytes()
            .iter()
            .skip(7)
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
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

pub fn docker_execution_environment(
    envelope: &ExecutionPlanEnvelopeV1,
) -> Result<BTreeMap<OsString, OsString>, RuntimeError> {
    let mut environment = runtime_environment();
    for name in &envelope.plan.environment.inherit {
        if let Some(value) = std::env::var_os(name) {
            environment.insert(OsString::from(name), value);
        }
    }
    for (name, value) in validate_fixed_environment(envelope)? {
        environment.insert(OsString::from(name), OsString::from(value));
    }
    for binding in &envelope.plan.environment.runtime_internal {
        environment.insert(
            OsString::from(&binding.name),
            OsString::from(&binding.container_target),
        );
    }
    Ok(environment)
}

fn validate_fixed_environment(
    envelope: &ExecutionPlanEnvelopeV1,
) -> Result<BTreeMap<String, String>, RuntimeError> {
    let declared = envelope
        .plan
        .environment
        .fixed
        .iter()
        .map(|binding| (binding.name.as_str(), binding.value_digest.as_str()))
        .collect::<BTreeMap<_, _>>();
    for name in envelope.fixed_environment.keys() {
        if !declared.contains_key(name.as_str()) {
            return Err(RuntimeError::FixedEnvironmentBinding {
                name: name.clone(),
                reason: "is not declared in the normalized plan",
            });
        }
    }
    let mut values = BTreeMap::new();
    for binding in &envelope.plan.environment.fixed {
        let value = envelope
            .fixed_environment
            .get(&binding.name)
            .ok_or_else(|| RuntimeError::FixedEnvironmentBinding {
                name: binding.name.clone(),
                reason: "is missing from the private envelope",
            })?;
        let actual =
            canonical_digest(value).map_err(|_| RuntimeError::FixedEnvironmentBinding {
                name: binding.name.clone(),
                reason: "cannot be canonically digested",
            })?;
        if actual != binding.value_digest {
            return Err(RuntimeError::FixedEnvironmentBinding {
                name: binding.name.clone(),
                reason: "does not match the normalized plan digest",
            });
        }
        values.insert(binding.name.clone(), value.clone());
    }
    Ok(values)
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
    ];
    if runtime.pull_policy == Some(RuntimePullPolicy::Never)
        && runtime.swap_mode == Some(RuntimeSwapMode::Disabled)
    {
        argv.extend(["--pull".to_owned(), "never".to_owned()]);
    }
    argv.extend(["--memory".to_owned(), format!("{}m", runtime.memory_mib)]);
    if runtime.pull_policy == Some(RuntimePullPolicy::Never)
        && runtime.swap_mode == Some(RuntimeSwapMode::Disabled)
    {
        argv.extend([
            "--memory-swap".to_owned(),
            format!("{}m", runtime.memory_mib),
        ]);
    }
    argv.extend([
        "--pids-limit".to_owned(),
        runtime.pids_limit.to_string(),
        "--tmpfs".to_owned(),
        "/tmp:rw,noexec,nosuid,nodev,size=64m".to_owned(),
    ]);
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
    validate_container_mount_target(&mount.target).map_err(RuntimeError::Workspace)?;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_limit_supported: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swap_limit_supported: Option<bool>,
    pub containment: ContainmentMechanism,
    pub graceful_stop: GracefulStopCapability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCapabilityEvidenceV1 {
    pub schema_version: String,
    pub memory_limit_supported: bool,
    pub swap_limit_supported: bool,
    pub context_digest: String,
    pub resolved_image_id: String,
    pub resolved_image_reference: String,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerLifecyclePlan {
    pub container_name: String,
    pub owner_label: String,
    pub run_label: String,
    pub check_label: String,
    pub create_argv: Vec<String>,
    pub start_argv: Vec<String>,
    pub attach_argv: Vec<String>,
    pub wait_argv: Vec<String>,
    pub inspect_argv: Vec<String>,
    pub stop_argv: Vec<String>,
    pub kill_argv: Vec<String>,
    pub remove_argv: Vec<String>,
}

impl DockerLifecyclePlan {
    pub fn build(
        run_id: &str,
        check: &NormalizedCheck,
        rendered: &DryRunCheck,
    ) -> Result<Self, RuntimeError> {
        if run_id.is_empty() || check.id.is_empty() || rendered.program != "docker" {
            return Err(RuntimeError::LifecycleFailure(
                "lifecycle identity or rendered runtime is invalid",
            ));
        }
        if rendered.argv.first().map(String::as_str) != Some("run") {
            return Err(RuntimeError::LifecycleFailure(
                "lifecycle requires a docker run plan",
            ));
        }
        let mut hasher = Sha256::new();
        hasher.update(b"commit-ci-preflight-container-v1\0");
        hasher.update(run_id.as_bytes());
        hasher.update([0]);
        hasher.update(check.id.as_bytes());
        let digest: String = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let container_name = format!("ccp-{}", &digest[..24]);
        let owner_label = "com.commit-ci-preflight.owner=commit-ci-preflight".to_owned();
        let run_label = format!("com.commit-ci-preflight.run-id={run_id}");
        let check_label = format!("com.commit-ci-preflight.check-id={}", check.id);

        let mut create_argv = vec![
            "create".to_owned(),
            "--name".to_owned(),
            container_name.clone(),
            "--label".to_owned(),
            owner_label.clone(),
            "--label".to_owned(),
            run_label.clone(),
            "--label".to_owned(),
            check_label.clone(),
        ];
        create_argv.extend(
            rendered
                .argv
                .iter()
                .skip(1)
                .filter(|argument| argument.as_str() != "--rm")
                .cloned(),
        );

        Ok(Self {
            start_argv: vec!["start".to_owned(), container_name.clone()],
            attach_argv: vec![
                "attach".to_owned(),
                "--sig-proxy=false".to_owned(),
                container_name.clone(),
            ],
            wait_argv: vec!["wait".to_owned(), container_name.clone()],
            inspect_argv: vec![
                "inspect".to_owned(),
                "--format".to_owned(),
                "{{json .}}".to_owned(),
                container_name.clone(),
            ],
            stop_argv: vec![
                "stop".to_owned(),
                "--time".to_owned(),
                "5".to_owned(),
                container_name.clone(),
            ],
            kill_argv: vec!["kill".to_owned(), container_name.clone()],
            remove_argv: vec![
                "rm".to_owned(),
                "--force".to_owned(),
                container_name.clone(),
            ],
            container_name,
            owner_label,
            run_label,
            check_label,
            create_argv,
        })
    }
}

fn completed_success(result: &ProcessResult) -> bool {
    result.termination == ProcessTermination::Completed
        && result.cleanup == CleanupStatus::Verified
        && result.exit.is_some_and(|exit| exit.success)
}

fn valid_container_id(bytes: &[u8]) -> bool {
    let value = String::from_utf8_lossy(bytes).trim().to_owned();
    (12..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn parse_wait_exit_code(bytes: &[u8]) -> Result<i32, RuntimeError> {
    let value = std::str::from_utf8(bytes)
        .map_err(|_| RuntimeError::LifecycleFailure("docker wait output is not UTF-8"))?
        .trim();
    let code = value
        .parse::<i32>()
        .map_err(|_| RuntimeError::LifecycleFailure("docker wait output is not an exit code"))?;
    if !(0..=255).contains(&code) {
        return Err(RuntimeError::LifecycleFailure(
            "docker wait returned an invalid exit code",
        ));
    }
    Ok(code)
}

fn parse_container_state(
    bytes: &[u8],
    lifecycle: &DockerLifecyclePlan,
) -> Result<String, RuntimeError> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| RuntimeError::LifecycleFailure("docker inspect output is not JSON"))?;
    let name = value
        .get("Name")
        .and_then(Value::as_str)
        .map(|name| name.trim_start_matches('/'));
    if name != Some(lifecycle.container_name.as_str()) {
        return Err(RuntimeError::LifecycleFailure(
            "docker inspect returned an unexpected container name",
        ));
    }
    let labels = value
        .get("Config")
        .and_then(|config| config.get("Labels"))
        .and_then(Value::as_object)
        .ok_or(RuntimeError::LifecycleFailure(
            "docker inspect omitted container labels",
        ))?;
    if !label_matches(labels, &lifecycle.owner_label)
        || !label_matches(labels, &lifecycle.run_label)
        || !label_matches(labels, &lifecycle.check_label)
    {
        return Err(RuntimeError::LifecycleFailure(
            "docker inspect returned non-CCP ownership labels",
        ));
    }
    let state = value
        .get("State")
        .and_then(|state| state.get("Status"))
        .and_then(Value::as_str)
        .ok_or(RuntimeError::LifecycleFailure(
            "docker inspect omitted container state",
        ))?;
    match state {
        "created" | "running" | "restarting" | "paused" | "exited" | "dead" => Ok(state.to_owned()),
        _ => Err(RuntimeError::LifecycleFailure(
            "docker inspect returned an unknown container state",
        )),
    }
}

fn label_matches(labels: &serde_json::Map<String, Value>, key_value: &str) -> bool {
    let Some((key, value)) = key_value.split_once('=') else {
        return false;
    };
    labels.get(key).and_then(Value::as_str) == Some(value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMountPolicy {
    ExplicitBindings,
}

#[derive(Debug)]
pub enum RuntimeError {
    Unsupported(&'static str),
    UnsupportedCapability(&'static str),
    Unavailable,
    TimedOut,
    Cancelled,
    CleanupUncertain,
    InvalidProbe(&'static str),
    LifecycleFailure(&'static str),
    FixedEnvironmentBinding { name: String, reason: &'static str },
    Workspace(WorkspaceError),
    Process(ProcessError),
}

impl RuntimeError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Unsupported(_)
            | Self::UnsupportedCapability(_)
            | Self::Unavailable
            | Self::InvalidProbe(_) => 4,
            Self::TimedOut | Self::Cancelled => 5,
            Self::Workspace(_) => 2,
            Self::CleanupUncertain
            | Self::LifecycleFailure(_)
            | Self::FixedEnvironmentBinding { .. }
            | Self::Process(ProcessError::CleanupUncertain { .. }) => 70,
            Self::Process(ProcessError::StaleGeneration) => 5,
            Self::Process(_) => 70,
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(message) => write!(formatter, "unsupported runtime: {message}"),
            Self::UnsupportedCapability(capability) => {
                write!(
                    formatter,
                    "runtime does not support required capability: {capability}"
                )
            }
            Self::Unavailable => formatter.write_str("Docker-compatible runtime is unavailable"),
            Self::TimedOut => formatter.write_str("runtime probe exceeded its deadline"),
            Self::Cancelled => formatter.write_str("runtime probe was cancelled"),
            Self::CleanupUncertain => formatter.write_str("runtime probe cleanup is uncertain"),
            Self::InvalidProbe(message) => write!(formatter, "invalid runtime probe: {message}"),
            Self::LifecycleFailure(message) => {
                write!(formatter, "runtime lifecycle failed: {message}")
            }
            Self::FixedEnvironmentBinding { name, reason } => {
                write!(formatter, "fixed environment binding {name:?} {reason}")
            }
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
            | Self::UnsupportedCapability(_)
            | Self::Unavailable
            | Self::TimedOut
            | Self::Cancelled
            | Self::CleanupUncertain
            | Self::InvalidProbe(_)
            | Self::LifecycleFailure(_)
            | Self::FixedEnvironmentBinding { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{CacheRootSource, ResolvedCacheRoot};
    use crate::config::ConfigV1;
    use crate::process::{CapturedStream, ExitOutcome};
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

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

    #[test]
    fn v1_1_runtime_internal_environment_uses_declared_cache_target() {
        let envelope = ConfigV1::parse(
            r#"
schema_version = "1.1"
project = "owner/project"

[runtime]
kind = "docker_compatible"
image = "registry.example/ci@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
cpu_count = 2
memory_mib = 256
pids_limit = 64

[environment.fixed]
SOURCE_DATE_EPOCH = "0"

[[environment.runtime_internal]]
name = "CARGO_HOME"
cache_id = "cargo-home"

[[caches]]
id = "cargo-home"
mount_path = ".ccp-mounts/cargo-home"

[[checks]]
id = "format"
required = true
argv = ["cargo", "fmt", "--check"]
working_directory = "."
timeout_seconds = 60
"#,
        )
        .expect("config")
        .into_plan()
        .expect("plan");

        let environment = docker_execution_environment(&envelope).expect("environment");
        assert_eq!(
            environment.get(&OsString::from("CARGO_HOME")),
            Some(&OsString::from("/workspace/.ccp-mounts/cargo-home"))
        );
        assert_eq!(
            environment.get(&OsString::from("SOURCE_DATE_EPOCH")),
            Some(&OsString::from("0"))
        );
    }

    #[test]
    fn fixed_environment_binding_must_be_present_and_match_its_plan_digest() {
        let mut envelope = ConfigV1::parse(
            r#"
schema_version = "1.1"
project = "owner/project"

[runtime]
kind = "docker_compatible"
image = "registry.example/ci@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
cpu_count = 2
memory_mib = 256
pids_limit = 64

[environment.fixed]
SOURCE_DATE_EPOCH = "0"

[[checks]]
id = "format"
required = true
argv = ["cargo", "fmt", "--check"]
working_directory = "."
timeout_seconds = 60
"#,
        )
        .expect("config")
        .into_plan()
        .expect("plan");

        envelope.fixed_environment.remove("SOURCE_DATE_EPOCH");
        assert!(matches!(
            docker_execution_environment(&envelope),
            Err(RuntimeError::FixedEnvironmentBinding { name, .. }) if name == "SOURCE_DATE_EPOCH"
        ));

        envelope
            .fixed_environment
            .insert("SOURCE_DATE_EPOCH".to_owned(), "1".to_owned());
        assert!(matches!(
            docker_execution_environment(&envelope),
            Err(RuntimeError::FixedEnvironmentBinding { name, .. }) if name == "SOURCE_DATE_EPOCH"
        ));
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

    #[derive(Default)]
    struct CapabilitySupervisor {
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl SupervisorPort for CapabilitySupervisor {
        fn execute(
            &self,
            request: &ProcessRequest,
            _cancellation: &CancellationToken,
            generation: &GenerationGuard,
        ) -> Result<ProcessResult, ProcessError> {
            generation.ensure_current(&request.identity)?;
            let argv = request
                .argv
                .iter()
                .map(|value| value.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            self.calls.lock().expect("calls").push(argv.clone());
            let stdout = match argv.first().map(String::as_str) {
                Some("info") => br#"{"OperatingSystem":"Docker Engine","ServerVersion":"28.3.2","OSType":"linux","MemoryLimit":true,"SwapLimit":true}"#.to_vec(),
                Some("context") => b"default\n".to_vec(),
                Some("image") => br#"{"Id":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","RepoDigests":["ghcr.io/example/ci@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]}"#.to_vec(),
                other => panic!("unexpected capability argv: {other:?}"),
            };
            Ok(ProcessResult {
                identity: request.identity.clone(),
                termination: ProcessTermination::Completed,
                cleanup: CleanupStatus::Verified,
                exit: Some(ExitOutcome {
                    success: true,
                    code: Some(0),
                }),
                stdout: CapturedStream::from_captured(stdout, false),
                stderr: CapturedStream::from_captured(Vec::new(), false),
                elapsed_millis: 1,
            })
        }
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
                stdout: CapturedStream::from_captured(stdout, truncated),
                stderr: CapturedStream::from_captured(Vec::new(), false),
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
    fn schema_1_3_rejects_daemon_without_swap_limit_capability() {
        let probe = RuntimeProbe {
            runtime: RuntimeKind::DockerCompatible,
            flavor: RuntimeFlavor::DockerCompatible,
            server_version: Some("28.3.2".to_owned()),
            operating_system: Some("Docker Engine".to_owned()),
            os_type: Some("linux".to_owned()),
            memory_limit_supported: Some(true),
            swap_limit_supported: Some(false),
            containment: containment_mechanism(),
            graceful_stop: graceful_stop_capability(),
        };
        let error = interpret_runtime_capabilities(
            &probe,
            "default",
            br#"{"Id":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","RepoDigests":["ghcr.io/example/ci@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]}"#,
            "ghcr.io/example/ci@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .expect_err("swap capability is required by schema 1.3");

        assert!(matches!(
            error,
            RuntimeError::UnsupportedCapability("swap_limit")
        ));
    }

    #[test]
    fn capability_evidence_digests_context_and_binds_the_exact_image_reference() {
        let probe = RuntimeProbe {
            runtime: RuntimeKind::DockerCompatible,
            flavor: RuntimeFlavor::DockerCompatible,
            server_version: Some("28.3.2".to_owned()),
            operating_system: Some("Docker Engine".to_owned()),
            os_type: Some("linux".to_owned()),
            memory_limit_supported: Some(true),
            swap_limit_supported: Some(true),
            containment: containment_mechanism(),
            graceful_stop: graceful_stop_capability(),
        };
        let context = "private-context";
        let image = "ghcr.io/example/ci@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let evidence = interpret_runtime_capabilities(
            &probe,
            context,
            format!(
                r#"{{"Id":"sha256:{id}","RepoDigests":["{image}"]}}"#,
                id = "b".repeat(64),
            )
            .as_bytes(),
            image,
        )
        .expect("capability evidence");

        assert_eq!(evidence.resolved_image_reference, image);
        assert_eq!(
            evidence.resolved_image_id,
            format!("sha256:{}", "b".repeat(64))
        );
        assert_eq!(
            evidence.context_digest,
            canonical_digest(&context).expect("digest")
        );
        assert!(
            !serde_json::to_string(&evidence)
                .expect("evidence JSON")
                .contains(context)
        );
    }

    #[test]
    fn schema_1_3_capability_preflight_uses_bounded_read_only_docker_argv() {
        let envelope = ConfigV1::parse(
            &CONFIG.replace("schema_version = \"1.0\"", "schema_version = \"1.3\"")
                .replace(
                    "network = false\n\n[[checks]]",
                    "network = false\npull_policy = \"never\"\nswap_mode = \"disabled\"\n\n[storage]\nmin_free_bytes = 1073741824\nreceipt_journal_reserve_bytes = 1048576\nmax_cache_growth_bytes = 2147483648\n\n[[checks]]",
                ),
        )
        .expect("config")
        .into_plan()
        .expect("schema 1.3 plan");
        let supervisor = CapabilitySupervisor::default();
        let generation = doctor_guard(&envelope);
        let preflight = preflight_runtime_capabilities(
            &envelope,
            &DockerCompatibleRuntime,
            &DockerRuntimeCapabilityProbe,
            &supervisor,
            &std::env::current_dir().expect("current dir"),
            &CancellationToken::default(),
            &generation,
        )
        .expect("capability preflight");

        assert_eq!(
            preflight
                .capability_evidence
                .as_ref()
                .expect("schema 1.3 evidence")
                .resolved_image_reference,
            envelope.plan.runtime.image
        );
        assert_eq!(
            *supervisor.calls.lock().expect("calls"),
            vec![
                vec!["info", "--format", "{{json .}}"],
                vec!["context", "show"],
                vec![
                    "image",
                    "inspect",
                    "--format",
                    "{{json .}}",
                    "ghcr.io/example/ci@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                ],
            ]
        );
    }

    #[test]
    fn historical_schema_skips_runtime_capability_preflight() {
        let envelope = envelope();
        let supervisor = FakeSupervisor::new(ProbeMode::OrbStack);
        let generation = doctor_guard(&envelope);
        let preflight = preflight_runtime_capabilities(
            &envelope,
            &DockerCompatibleRuntime,
            &DockerRuntimeCapabilityProbe,
            &supervisor,
            &std::env::current_dir().expect("current dir"),
            &CancellationToken::default(),
            &generation,
        )
        .expect("historical no-op preflight");

        assert!(preflight.runtime_probe.is_none());
        assert!(preflight.capability_evidence.is_none());
        assert_eq!(supervisor.calls.load(Ordering::SeqCst), 0);
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
    fn mount_renderer_rejects_ambiguous_source_and_target_bindings() {
        let base = crate::workspace::MountBinding {
            source: std::path::PathBuf::from("/safe/source"),
            target: "/workspace/safe-target".to_owned(),
            access: MountAccess::ReadOnly,
            purpose: crate::workspace::MountPurpose::Repository,
            logical_id: None,
        };
        for source in [
            "/safe,comma",
            "/safe=equals",
            "/safe\nnewline",
            "/safe\0nul",
        ] {
            let mut mount = base.clone();
            mount.source = std::path::PathBuf::from(source);
            assert!(matches!(
                docker_mount_argument(&mount),
                Err(RuntimeError::Workspace(WorkspaceError::UnsupportedHostPath))
            ));
        }
        for target in [
            "/workspace/safe,comma",
            "/workspace/safe=equals",
            "/workspace/../escape",
            "/not-workspace/safe",
        ] {
            let mut mount = base.clone();
            mount.target = target.to_owned();
            assert!(matches!(
                docker_mount_argument(&mount),
                Err(RuntimeError::Workspace(WorkspaceError::InvalidLogicalPath))
            ));
        }
    }

    #[test]
    fn schema_1_3_dry_run_declares_no_pull_and_disabled_swap() {
        let envelope = ConfigV1::parse(
            &CONFIG.replace("schema_version = \"1.0\"", "schema_version = \"1.3\"")
                .replace("memory_mib = 8192", "memory_mib = 256")
                .replace(
                    "network = false\n\n[[checks]]",
                    "network = false\npull_policy = \"never\"\nswap_mode = \"disabled\"\n\n[storage]\nmin_free_bytes = 1073741824\nreceipt_journal_reserve_bytes = 1048576\nmax_cache_growth_bytes = 2147483648\n\n[[checks]]",
                ),
        )
        .expect("config")
        .into_plan()
        .expect("schema 1.3 plan");
        let workspace = workspace(&envelope);
        let dry_run = DockerCompatibleRuntime
            .dry_run(&envelope, &workspace)
            .expect("dry run");

        assert!(
            dry_run.checks[0]
                .argv
                .windows(2)
                .any(|pair| { pair == ["--pull", "never"] })
        );
        assert!(
            dry_run.checks[0]
                .argv
                .windows(2)
                .any(|pair| { pair == ["--memory", "256m"] })
        );
        assert!(
            dry_run.checks[0]
                .argv
                .windows(2)
                .any(|pair| { pair == ["--memory-swap", "256m"] })
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
    fn lifecycle_plan_is_deterministic_and_replaces_rm_with_owned_commands() {
        let envelope = envelope();
        let workspace = workspace(&envelope);
        let dry_run = DockerCompatibleRuntime
            .dry_run(&envelope, &workspace)
            .expect("dry run");
        let check = &envelope.plan.checks[0];
        let first = DockerLifecyclePlan::build("sha256:run", check, &dry_run.checks[0])
            .expect("lifecycle plan");
        let second = DockerLifecyclePlan::build("sha256:run", check, &dry_run.checks[0])
            .expect("lifecycle replay");

        assert_eq!(first, second);
        assert!(first.container_name.starts_with("ccp-"));
        assert_eq!(first.container_name.len(), 28);
        assert_eq!(
            first.create_argv.first().map(String::as_str),
            Some("create")
        );
        assert!(!first.create_argv.iter().any(|argument| argument == "--rm"));
        assert!(first.create_argv.windows(2).any(|pair| {
            pair == [
                "--label",
                "com.commit-ci-preflight.owner=commit-ci-preflight",
            ]
        }));
        assert!(
            first
                .create_argv
                .iter()
                .any(|argument| { argument == "com.commit-ci-preflight.run-id=sha256:run" })
        );
        assert!(
            first
                .create_argv
                .iter()
                .any(|argument| { argument == "com.commit-ci-preflight.check-id=test" })
        );
        assert_eq!(first.start_argv[0], "start");
        assert_eq!(first.attach_argv[0], "attach");
        assert_eq!(first.wait_argv[0], "wait");
        assert_eq!(first.stop_argv[0], "stop");
        assert_eq!(first.kill_argv[0], "kill");
        assert_eq!(first.remove_argv[0], "rm");
        assert!(first.remove_argv.contains(&"--force".to_owned()));
    }

    #[test]
    fn lifecycle_plan_rejects_non_run_and_helpers_fail_closed() {
        let envelope = envelope();
        let workspace = workspace(&envelope);
        let dry_run = DockerCompatibleRuntime
            .dry_run(&envelope, &workspace)
            .expect("dry run");
        let lifecycle =
            DockerLifecyclePlan::build("run", &envelope.plan.checks[0], &dry_run.checks[0])
                .expect("lifecycle");
        let mut rendered = dry_run.checks[0].clone();
        rendered.argv[0] = "exec".to_owned();
        assert!(matches!(
            DockerLifecyclePlan::build("run", &envelope.plan.checks[0], &rendered),
            Err(RuntimeError::LifecycleFailure(_))
        ));
        assert!(parse_wait_exit_code(b"256\n").is_err());
        assert!(parse_container_state(b"{}", &lifecycle).is_err());
        assert!(!valid_container_id(b"not-an-id\n"));
    }

    struct LifecycleSupervisor {
        calls: Arc<Mutex<Vec<String>>>,
        inspect_calls: AtomicUsize,
    }

    impl LifecycleSupervisor {
        fn new() -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                inspect_calls: AtomicUsize::new(0),
            }
        }
    }

    impl SupervisorPort for LifecycleSupervisor {
        fn execute(
            &self,
            request: &ProcessRequest,
            _cancellation: &CancellationToken,
            generation: &GenerationGuard,
        ) -> Result<ProcessResult, ProcessError> {
            generation.ensure_current(&request.identity)?;
            let command = request
                .argv
                .first()
                .and_then(|value| value.to_str())
                .expect("command");
            self.calls.lock().expect("calls").push(command.to_owned());
            let (success, code, stdout) = match command {
                "create" => (true, 0, format!("{}\n", "a".repeat(64)).into_bytes()),
                "start" => (true, 0, Vec::new()),
                "attach" => (true, 0, b"check output\n".to_vec()),
                "wait" => (true, 0, b"17\n".to_vec()),
                "inspect" if self.inspect_calls.fetch_add(1, Ordering::SeqCst) == 0 => {
                    let name = request
                        .argv
                        .last()
                        .and_then(|value| value.to_str())
                        .expect("container name");
                    let inspect = format!(
                        r#"{{"Name":"/{name}","Config":{{"Labels":{{"com.commit-ci-preflight.owner":"commit-ci-preflight","com.commit-ci-preflight.run-id":"sha256:run","com.commit-ci-preflight.check-id":"test"}}}},"State":{{"Status":"running"}}}}"#
                    );
                    (true, 0, inspect.into_bytes())
                }
                "inspect" => (false, 1, b"Error: No such container\n".to_vec()),
                "stop" | "kill" | "rm" => (true, 0, Vec::new()),
                _ => (false, 1, Vec::new()),
            };
            Ok(ProcessResult {
                identity: request.identity.clone(),
                termination: ProcessTermination::Completed,
                cleanup: CleanupStatus::Verified,
                exit: Some(ExitOutcome {
                    success,
                    code: Some(code),
                }),
                stdout: CapturedStream::from_captured(stdout, false),
                stderr: CapturedStream::from_captured(Vec::new(), false),
                elapsed_millis: 1,
            })
        }
    }

    #[test]
    fn lifecycle_executes_owned_sequence_and_proves_final_absence() {
        let envelope = envelope();
        let workspace = workspace(&envelope);
        let dry_run = DockerCompatibleRuntime
            .dry_run(&envelope, &workspace)
            .expect("dry run");
        let identity = RunIdentity {
            project: envelope.plan.project.clone(),
            commit: Some("a".repeat(40)),
            config_digest: envelope.plan_digest.clone(),
            generation: "1".to_owned(),
        };
        let generation = GenerationGuard::new(identity.clone());
        let supervisor = LifecycleSupervisor::new();
        let cancellation = CancellationToken::default();
        let execution_root = std::path::Path::new("/workspace");
        let environment = BTreeMap::new();
        let context = RuntimeExecutionContext {
            execution_root,
            environment: &environment,
            identity: &identity,
            run_id: "sha256:run",
            supervisor: &supervisor,
            cancellation: &cancellation,
            generation: &generation,
            timeout_seconds: envelope.plan.checks[0].timeout_seconds,
        };
        let result = DockerCompatibleRuntime
            .execute_check(&envelope.plan.checks[0], &dry_run.checks[0], &context)
            .expect("lifecycle execution");

        assert_eq!(result.exit.expect("exit").code, Some(17));
        assert_eq!(result.stdout.bytes, b"check output\n");
        assert_eq!(
            *supervisor.calls.lock().expect("calls"),
            vec![
                "create", "start", "attach", "wait", "inspect", "stop", "rm", "inspect"
            ]
        );
    }

    #[test]
    fn characterizes_one_shot_docker_client_without_daemon_identity_before_t3() {
        let envelope = envelope();
        let workspace = workspace(&envelope);
        let dry_run = DockerCompatibleRuntime
            .dry_run(&envelope, &workspace)
            .expect("dry run");
        let argv = &dry_run.checks[0].argv;

        assert_eq!(argv.first().map(String::as_str), Some("run"));
        assert!(argv.iter().any(|part| part == "--rm"));
        assert!(!argv.iter().any(|part| {
            matches!(
                part.as_str(),
                "--name" | "--label" | "--cidfile" | "create" | "start"
            )
        }));
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
