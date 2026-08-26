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

mod terminal;
use terminal::{TerminalFailure, finalize_owned_terminal};

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use commit_ci_preflight::admission::{
    ADMISSION_STATUS_SCHEMA_VERSION, AdmissionCoordinator, AdmissionError, AdmissionGuard,
    DEFAULT_QUEUE_TIMEOUT, DEFAULT_STATUS_TIMEOUT,
};
use commit_ci_preflight::benchmark::{
    BenchmarkError, run_benchmark, verify_benchmark_document, write_new_receipt,
};
use commit_ci_preflight::cache::{
    CacheError, CacheRootOptions, CacheUsePin, ManagedCache, ResolvedCacheRoot,
};
use commit_ci_preflight::config::{ConfigError, ConfigV1, ExecutionPlanEnvelopeV1};
use commit_ci_preflight::github_actions::{
    GithubActionsError, MigrationReadiness, analyze_workflow_file,
};
use commit_ci_preflight::matrix::{
    MatrixConfigV2, MatrixError, MatrixPlanEnvelopeV2, MatrixRunRequestV2, execute_matrix_run_v2,
};
use commit_ci_preflight::process::{
    CancellationReason, CancellationToken, GenerationGuard, OutputMode, ProcessRequest,
    ProcessResult, ProcessSupervisor, ProcessTermination, RunIdentity, SupervisorPort,
};
use commit_ci_preflight::receipt::{CheckEvidence, EvidenceStatus, canonical_digest};
use commit_ci_preflight::resource::{
    ResourceGuardError, ResourceObservation, ResourcePlatform, ResourceProbe, ResourceProbeError,
    ResourceSnapshot, ResourceWatchdog, SupervisorResourceRunner, WatchdogTripReason,
    evaluate_pre_start, status_from_snapshot, unknown_status, unsupported_status,
};
use commit_ci_preflight::resource_history::{
    DEFAULT_RESOURCE_PROFILE, DEFAULT_WORKLOAD_FAMILY, ResourceCacheStateV2,
    ResourceExecutionContextV2, ResourceExecutionModeV2, ResourceExecutorV2,
    ResourceHistoryRecordV2, ResourceHistoryStore, ResourceRunOutcome, ResourceTerminalDetailV2,
    validate_profile,
};
use commit_ci_preflight::run::{
    Clock, CompletionBarrier, RunError, RunLifecycleObserver, RunLifecyclePhase, RunRequest,
    SystemClock, execute_local_run_with_barrier_and_lifecycle_and_runtime_preflight,
};
use commit_ci_preflight::run_journal::{
    RUN_JOURNAL_SCHEMA_VERSION, RecoveryStatusV1, RunFailureKindV1, RunJournalError,
    RunJournalStateV1, RunJournalStore,
};
use commit_ci_preflight::runtime::{
    DockerRuntimeCapabilityProbe, DryRunPlan, RuntimeError, RuntimeProbe, doctor_guard,
    preflight_runtime_capabilities, runtime_for,
};
use commit_ci_preflight::source_snapshot::{SourceSnapshot, resolve_clean_head};
use commit_ci_preflight::storage::{SystemStorageProbe, preflight as preflight_storage};
use commit_ci_preflight::verify::{
    VerificationDecision, VerificationError, receipt_input_failure_report, system_evaluated_at_utc,
    validate_verification_policy_path, verify_receipt_document_for_policy_path,
};
use commit_ci_preflight::workspace::{WorkspaceError, WorkspacePlanV1};
use serde::Serialize;

const GUARD_EXEC_DEFAULT_TIMEOUT: Duration = Duration::from_secs(6 * 60 * 60);
const GUARD_EXEC_MAX_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);
const GUARD_EXEC_CAPTURE_BYTES: usize = 1_048_576;
static JOURNAL_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Parser)]
#[command(name = "commit-ci-preflight", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Inspect the host-wide heavy-command admission queue.
    Admission {
        #[command(subcommand)]
        action: AdmissionCommand,
    },
    /// Inspect the host-memory resource guard without changing host state.
    Resource {
        #[command(subcommand)]
        action: ResourceAction,
    },
    /// Serialize and supervise one explicit external program.
    Guard {
        #[command(subcommand)]
        action: GuardCommand,
    },
    /// Validate configuration and print the normalized read-only execution plan.
    Plan {
        /// Configuration file to validate.
        #[arg(long, default_value = ".commit-ci-preflight.toml")]
        config: PathBuf,
        /// Emit canonical machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Probe the configured runtime without running project checks.
    Doctor {
        /// Configuration file to validate.
        #[arg(long, default_value = ".commit-ci-preflight.toml")]
        config: PathBuf,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Render runtime argv without spawning a process.
    DryRun {
        /// Configuration file to validate.
        #[arg(long, default_value = ".commit-ci-preflight.toml")]
        config: PathBuf,
        #[command(flatten)]
        location: CacheLocationArgs,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Execute the validated checks locally and write a canonical receipt.
    Run {
        /// Configuration file to validate.
        #[arg(long, default_value = ".commit-ci-preflight.toml")]
        config: PathBuf,
        #[command(flatten)]
        location: CacheLocationArgs,
        /// Monotonic operator-selected generation bound into the receipt.
        #[arg(long, default_value_t = 1)]
        generation: u64,
        /// Emit the canonical receipt as JSON.
        #[arg(long)]
        json: bool,
        /// Maximum time to wait for the host-wide heavy-command slot.
        #[arg(long, default_value_t = DEFAULT_QUEUE_TIMEOUT.as_secs())]
        admission_timeout_seconds: u64,
    },
    /// Independently verify receipt integrity and repository policy.
    Verify {
        /// Canonical receipt JSON to verify.
        #[arg(long, default_value = ".ccp/receipt.json")]
        receipt: PathBuf,
        /// Strict repository verification policy.
        #[arg(long, default_value = ".commit-ci-policy.toml")]
        policy: PathBuf,
        /// Exact lowercase Git SHA supplied by the calling trust boundary.
        #[arg(long)]
        expected_commit: String,
        /// Explicit strict UTC evaluation instant; defaults to the local system clock.
        #[arg(long)]
        evaluated_at_utc: Option<String>,
        /// Emit the canonical machine-readable verification report.
        #[arg(long)]
        json: bool,
    },
    /// Analyze a GitHub Actions workflow as data and report migration compatibility.
    MigrateGithubActions {
        /// Workflow YAML file to analyze without executing any action or command.
        #[arg(long)]
        workflow: PathBuf,
        /// Emit the versioned machine-readable compatibility report.
        #[arg(long)]
        json: bool,
    },
    /// Run the fixed deterministic native benchmark and emit a verifiable receipt.
    Benchmark {
        /// Exact lowercase Git commit represented by this benchmark execution.
        #[arg(long)]
        commit: String,
        /// Optional configuration whose Docker-compatible runtime is probed read-only.
        #[arg(long)]
        runtime_config: Option<PathBuf>,
        /// Optional new output file; existing files are never overwritten.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Emit the canonical benchmark receipt as JSON.
        #[arg(long)]
        json: bool,
        /// Maximum time to wait for the host-wide heavy-command slot.
        #[arg(long, default_value_t = DEFAULT_QUEUE_TIMEOUT.as_secs())]
        admission_timeout_seconds: u64,
    },
    /// Independently verify a benchmark receipt and its expected native platform.
    VerifyBenchmark {
        /// Benchmark receipt JSON to verify.
        #[arg(long)]
        receipt: PathBuf,
        /// Exact lowercase Git commit expected by the calling boundary.
        #[arg(long)]
        expected_commit: String,
        /// Expected native process operating system: linux, macos, or windows.
        #[arg(long)]
        expected_os: String,
        /// Expected native process architecture: aarch64 or x86_64.
        #[arg(long)]
        expected_arch: String,
        /// Optional expected CI environment, currently github_actions.
        #[arg(long)]
        expected_ci_environment: Option<String>,
        /// Optional expected probed runtime flavor, such as orbstack.
        #[arg(long)]
        expected_runtime_flavor: Option<String>,
        /// Emit a machine-readable PASS report.
        #[arg(long)]
        json: bool,
    },
    /// Inspect or initialize the persistent managed cache.
    Cache {
        #[command(subcommand)]
        action: CacheCommand,
    },
    /// Inspect or quarantine unfinished CCP-owned run journal state.
    Recover {
        #[command(subcommand)]
        action: RecoverCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ResourceAction {
    /// Report the bounded resource-guard capability and current decision.
    Status {
        /// Emit the versioned machine-readable status.
        #[arg(long)]
        json: bool,
    },
    /// Read the bounded privacy-minimized local v2 observation history.
    History {
        /// Emit the versioned machine-readable history report.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum GuardCommand {
    /// Run one explicit argv without invoking a shell.
    Exec(GuardExecArgs),
}

#[derive(Debug, Args)]
struct GuardExecArgs {
    /// Maximum time to wait for the admission slot in seconds, capped at 24 hours.
    #[arg(long, default_value_t = GUARD_EXEC_DEFAULT_TIMEOUT.as_secs())]
    admission_timeout_seconds: u64,
    /// Maximum child runtime in seconds, capped at 24 hours.
    #[arg(long, default_value_t = GUARD_EXEC_DEFAULT_TIMEOUT.as_secs())]
    timeout_seconds: u64,
    /// Stable, non-sensitive workload class used only by local resource history.
    #[arg(long, default_value = DEFAULT_RESOURCE_PROFILE)]
    resource_profile: String,
    /// Stable, non-sensitive pipeline/cohort identifier used only by local resource history.
    #[arg(long, default_value = DEFAULT_WORKLOAD_FAMILY)]
    resource_workload_family: String,
    /// Execution substrate; direct `docker --context orbstack` argv is detected automatically.
    #[arg(long, value_enum)]
    resource_executor: Option<ResourceExecutorArg>,
    /// Cache classification supplied by the caller; it never affects admission.
    #[arg(long, value_enum, default_value_t = ResourceCacheStateArg::Unknown)]
    resource_cache_state: ResourceCacheStateArg,
    /// Native or emulated target execution supplied by the caller.
    #[arg(long, value_enum, default_value_t = ResourceExecutionModeArg::Unknown)]
    resource_execution_mode: ResourceExecutionModeArg,
    /// Bounded non-sensitive target label such as `linux-amd64`.
    #[arg(long)]
    resource_target_platform: Option<String>,
    /// Requested CPU ceiling in millicores, when the runner has one.
    #[arg(long)]
    resource_cpu_limit_millis: Option<u32>,
    /// Requested memory ceiling in bytes, when the runner has one.
    #[arg(long)]
    resource_memory_limit_bytes: Option<u64>,
    /// Disable local observation history without changing admission or watchdog behavior.
    #[arg(long)]
    no_resource_history: bool,
    /// Managed cache root paired with one or more completed cache sources.
    #[arg(long)]
    managed_cache_root: Vec<PathBuf>,
    /// Completed managed-cache source to pin for the guarded child; repeatable.
    #[arg(long)]
    managed_cache_source: Vec<PathBuf>,
    /// Program and arguments. The `--` separator is required.
    #[arg(last = true, required = true, allow_hyphen_values = true)]
    argv: Vec<OsString>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ResourceExecutorArg {
    Native,
    Orbstack,
    Docker,
    Vm,
    Unknown,
}

impl From<ResourceExecutorArg> for ResourceExecutorV2 {
    fn from(value: ResourceExecutorArg) -> Self {
        match value {
            ResourceExecutorArg::Native => Self::Native,
            ResourceExecutorArg::Orbstack => Self::Orbstack,
            ResourceExecutorArg::Docker => Self::Docker,
            ResourceExecutorArg::Vm => Self::Vm,
            ResourceExecutorArg::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum ResourceCacheStateArg {
    Cold,
    Warm,
    Mixed,
    #[default]
    Unknown,
}

impl From<ResourceCacheStateArg> for ResourceCacheStateV2 {
    fn from(value: ResourceCacheStateArg) -> Self {
        match value {
            ResourceCacheStateArg::Cold => Self::Cold,
            ResourceCacheStateArg::Warm => Self::Warm,
            ResourceCacheStateArg::Mixed => Self::Mixed,
            ResourceCacheStateArg::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum ResourceExecutionModeArg {
    Native,
    Emulated,
    #[default]
    Unknown,
}

impl From<ResourceExecutionModeArg> for ResourceExecutionModeV2 {
    fn from(value: ResourceExecutionModeArg) -> Self {
        match value {
            ResourceExecutionModeArg::Native => Self::Native,
            ResourceExecutionModeArg::Emulated => Self::Emulated,
            ResourceExecutionModeArg::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Args)]
struct CacheLocationArgs {
    /// Repository whose checkout must remain outside the cache root.
    #[arg(long, default_value = ".")]
    repository: PathBuf,
    /// Explicit persistent cache root; overrides CCP_CACHE_DIR and platform defaults.
    #[arg(long)]
    cache_dir: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum CacheCommand {
    /// Resolve the persistent cache path without creating it.
    Path {
        #[command(flatten)]
        location: CacheLocationArgs,
        #[arg(long)]
        json: bool,
    },
    /// Create and atomically mark the managed cache root.
    Init {
        #[command(flatten)]
        location: CacheLocationArgs,
        #[arg(long)]
        json: bool,
    },
    /// Inventory an initialized cache without modifying it.
    Inventory {
        #[command(flatten)]
        location: CacheLocationArgs,
        #[arg(long, default_value_t = commit_ci_preflight::cache::DEFAULT_DISK_BUDGET_BYTES)]
        disk_budget_bytes: u64,
        #[arg(long)]
        json: bool,
    },
    /// Plan cleanup of incomplete entries; deletion is intentionally unavailable.
    Cleanup {
        #[command(flatten)]
        location: CacheLocationArgs,
        /// Required acknowledgement that this command only reports candidates.
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value_t = commit_ci_preflight::cache::DEFAULT_DISK_BUDGET_BYTES)]
        disk_budget_bytes: u64,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum RecoverCommand {
    /// Classify persisted run journals without modifying them.
    Status {
        #[command(flatten)]
        location: CacheLocationArgs,
        #[arg(long)]
        json: bool,
    },
    /// Quarantine one exact unfinished CCP-owned run journal.
    Apply {
        /// Exact lowercase 64-character run identifier.
        run_id: String,
        #[command(flatten)]
        location: CacheLocationArgs,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum AdmissionCommand {
    /// Report bounded coordinator state, lock roles, and lease ownership.
    Status {
        /// Emit the versioned machine-readable status.
        #[arg(long)]
        json: bool,
        /// Maximum time to wait for a consistent status snapshot.
        #[arg(long, default_value_t = DEFAULT_STATUS_TIMEOUT.as_secs())]
        timeout_seconds: u64,
    },
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Command::Admission { action }) => run_admission_command(action),
        Some(Command::Resource { action }) => run_resource_command(action),
        Some(Command::Guard { action }) => run_guard_command(action),
        Some(Command::Plan { config, json }) => print_plan(&config, json),
        Some(Command::Doctor { config, json }) => print_doctor(&config, json),
        Some(Command::DryRun {
            config,
            location,
            json,
        }) => print_dry_run(&config, &location, json),
        Some(Command::Run {
            config,
            location,
            generation,
            json,
            admission_timeout_seconds,
        }) => print_run(
            &config,
            &location,
            generation,
            admission_timeout_seconds,
            json,
        ),
        Some(Command::Verify {
            receipt,
            policy,
            expected_commit,
            evaluated_at_utc,
            json,
        }) => print_verify(
            &receipt,
            &policy,
            &expected_commit,
            evaluated_at_utc.as_deref(),
            json,
        ),
        Some(Command::MigrateGithubActions { workflow, json }) => {
            print_github_actions_migration(&workflow, json)
        }
        Some(Command::Benchmark {
            commit,
            runtime_config,
            output,
            json,
            admission_timeout_seconds,
        }) => print_benchmark(
            &commit,
            runtime_config.as_deref(),
            output.as_deref(),
            admission_timeout_seconds,
            json,
        ),
        Some(Command::VerifyBenchmark {
            receipt,
            expected_commit,
            expected_os,
            expected_arch,
            expected_ci_environment,
            expected_runtime_flavor,
            json,
        }) => print_verify_benchmark(
            &receipt,
            &expected_commit,
            &expected_os,
            &expected_arch,
            expected_ci_environment.as_deref(),
            expected_runtime_flavor.as_deref(),
            json,
        ),
        Some(Command::Cache { action }) => run_cache_command(action),
        Some(Command::Recover { action }) => run_recover_command(action),
        None => {
            Cli::command()
                .print_help()
                .expect("writing command help to stdout must succeed");
            println!();
            Ok(())
        }
    };
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(error.exit_code());
    }
}

fn print_benchmark(
    commit: &str,
    runtime_config: Option<&Path>,
    output: Option<&Path>,
    admission_timeout_seconds: u64,
    json: bool,
) -> Result<(), CliError> {
    let cancellation = CancellationToken::default();
    install_cancellation_handler(&cancellation)?;
    let supervisor = Arc::new(ProcessSupervisor::standard());
    let runtime_probe = runtime_config
        .map(|path| collect_runtime_probe(path, &cancellation, supervisor.as_ref()))
        .transpose()?;
    let admission = AdmissionCoordinator::platform().map_err(CliError::Admission)?;
    let guard = admission
        .acquire(
            Duration::from_secs(admission_timeout_seconds),
            &cancellation,
        )
        .map_err(CliError::Admission)?;
    let result = resource_pre_start(supervisor.clone(), &cancellation)
        .and_then(|_| run_benchmark(commit, runtime_probe.as_ref()).map_err(CliError::Benchmark));
    let envelope = finalize_benchmark_terminal(result, || guard.release())?;
    if let Some(path) = output {
        write_new_receipt(path, &envelope).map_err(CliError::Benchmark)?;
    }
    if json {
        let bytes = envelope.canonical_bytes().map_err(CliError::Benchmark)?;
        println!("{}", String::from_utf8(bytes).map_err(CliError::internal)?);
    } else {
        println!("Benchmark: {}", envelope.benchmark_id);
        println!(
            "Platform: {}/{}",
            envelope.receipt.platform.host_os, envelope.receipt.platform.host_arch
        );
        println!("Median: {} ns", envelope.receipt.median_ns);
        println!("Correctness: PASS");
        if let Some(path) = output {
            println!("Receipt: {}", path.display());
        }
    }
    Ok(())
}

fn run_guard_command(action: GuardCommand) -> Result<(), CliError> {
    match action {
        GuardCommand::Exec(args) => print_guard_exec(args),
    }
}

fn print_guard_exec(args: GuardExecArgs) -> Result<(), CliError> {
    validate_guard_cache_args(&args).map_err(CliError::Guard)?;
    let admission_timeout = Duration::from_secs(args.admission_timeout_seconds);
    if admission_timeout.is_zero() || admission_timeout > GUARD_EXEC_MAX_TIMEOUT {
        return Err(CliError::Guard(GuardExecError::InvalidAdmissionTimeout));
    }
    let timeout = Duration::from_secs(args.timeout_seconds);
    if timeout.is_zero() || timeout > GUARD_EXEC_MAX_TIMEOUT {
        return Err(CliError::Guard(GuardExecError::InvalidTimeout));
    }
    if args.argv.is_empty() {
        return Err(CliError::Guard(GuardExecError::MissingProgram));
    }
    validate_profile(&args.resource_profile)
        .map_err(|_| CliError::Guard(GuardExecError::InvalidResourceProfile))?;
    let context = ResourceExecutionContextV2 {
        profile: args.resource_profile,
        workload_family: args.resource_workload_family,
        executor: args
            .resource_executor
            .map(Into::into)
            .unwrap_or_else(|| detect_resource_executor(&args.argv)),
        cache_state: args.resource_cache_state.into(),
        execution_mode: args.resource_execution_mode.into(),
        target_platform: args.resource_target_platform,
        requested_cpu_millis: args.resource_cpu_limit_millis,
        requested_memory_bytes: args.resource_memory_limit_bytes,
    };
    context
        .validate()
        .map_err(|_| CliError::Guard(GuardExecError::InvalidResourceContext))?;
    let current_dir = fs::canonicalize(".")
        .map_err(|_| CliError::Guard(GuardExecError::InvalidCurrentDirectory))?;
    if !current_dir.is_dir() {
        return Err(CliError::Guard(GuardExecError::InvalidCurrentDirectory));
    }

    let managed_cache_root = if args.managed_cache_root.is_empty() {
        None
    } else {
        Some(
            ResolvedCacheRoot::resolve(
                &current_dir,
                &CacheRootOptions::from_process(Some(args.managed_cache_root[0].clone())),
            )
            .map_err(GuardExecError::from_cache)
            .map_err(CliError::Guard)?,
        )
    };

    let cancellation = CancellationToken::default();
    install_cancellation_handler(&cancellation)?;
    let supervisor = Arc::new(ProcessSupervisor::standard());
    let mut session = GuardExecSession::acquire(&cancellation, admission_timeout)?;

    let managed_cache = match managed_cache_root {
        None => None,
        Some(root) => match ManagedCache::open(root).map_err(GuardExecError::from_cache) {
            Ok(cache) => Some(cache),
            Err(error) => {
                let result = session.finish(Err(error), &cancellation);
                return result.map(|_| ()).map_err(CliError::Guard);
            }
        },
    };

    let baseline = match resource_pre_start(supervisor.clone(), &cancellation) {
        Ok(baseline) => baseline,
        Err(error) => {
            let result = session.finish(Err(GuardExecError::from_cli(error)), &cancellation);
            return result.map(|_| ()).map_err(CliError::Guard);
        }
    };

    session.start_watchdog(
        Arc::clone(&supervisor),
        current_dir.clone(),
        cancellation.clone(),
        baseline,
        context,
        !args.no_resource_history,
    );

    let mut argv = args.argv;
    let program = argv.remove(0);
    let request = ProcessRequest {
        identity: RunIdentity {
            project: "commit-ci-preflight.guard-exec".to_owned(),
            commit: None,
            config_digest: "guard-exec-v1".to_owned(),
            generation: "guard-exec-v1".to_owned(),
        },
        program,
        argv,
        current_dir,
        environment: std::env::vars_os().collect::<BTreeMap<_, _>>(),
        timeout,
        max_capture_bytes: GUARD_EXEC_CAPTURE_BYTES,
    };
    let pins = match managed_cache.as_ref() {
        None => None,
        Some(_) if args.managed_cache_source.is_empty() => {
            let error = GuardExecError::InvalidManagedCache;
            let result = session.finish(Err(error), &cancellation);
            return result.map(|_| ()).map_err(CliError::Guard);
        }
        Some(cache) => match cache
            .pin_completed_sources(&args.managed_cache_source)
            .map_err(GuardExecError::from_cache)
        {
            Ok(pins) => Some(pins),
            Err(error) => {
                let result = session.finish(Err(error), &cancellation);
                return result.map(|_| ()).map_err(CliError::Guard);
            }
        },
    };
    let mut session = Some(session);
    let process_and_finish = || {
        let session = session.take().ok_or(GuardExecError::InternalFailure)?;
        let process = supervisor
            .execute_with_output(
                &request,
                &cancellation,
                &GenerationGuard::new(request.identity.clone()),
                OutputMode::Tee,
            )
            .map_err(GuardExecError::Process);
        session.finish(process, &cancellation)
    };
    let result = match pins.as_ref() {
        Some(pins) => execute_with_guard_cache_pins(pins, process_and_finish),
        None => with_guard_cache_pins(None, &[], process_and_finish),
    };
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            if let Some(session) = session {
                let finished = session.finish(Err(error), &cancellation);
                return finished.map(|_| ()).map_err(CliError::Guard);
            }
            return Err(CliError::Guard(error));
        }
    };
    classify_guard_result(result, &cancellation).map_err(CliError::Guard)
}

fn execute_with_guard_cache_pins<T>(
    pins: &[CacheUsePin],
    child: impl FnOnce() -> Result<T, GuardExecError>,
) -> Result<T, GuardExecError> {
    for pin in pins {
        pin.revalidate().map_err(GuardExecError::from_cache)?;
    }
    child()
}

fn with_guard_cache_pins<T>(
    cache: Option<&ManagedCache>,
    sources: &[PathBuf],
    child: impl FnOnce() -> Result<T, GuardExecError>,
) -> Result<T, GuardExecError> {
    match (cache, sources.is_empty()) {
        (None, true) => child(),
        (None, false) => Err(GuardExecError::InvalidManagedCache),
        (Some(cache), false) => {
            let pins = cache
                .pin_completed_sources(sources)
                .map_err(GuardExecError::from_cache)?;
            execute_with_guard_cache_pins(&pins, child)
        }
        (Some(_), true) => Err(GuardExecError::InvalidManagedCache),
    }
}

fn validate_guard_cache_args(args: &GuardExecArgs) -> Result<(), GuardExecError> {
    let root_empty = args.managed_cache_root.is_empty();
    let sources_empty = args.managed_cache_source.is_empty();
    if root_empty && sources_empty {
        return Ok(());
    }
    if args.managed_cache_root.len() != 1 || sources_empty {
        return Err(GuardExecError::InvalidManagedCache);
    }
    Ok(())
}

fn detect_resource_executor(argv: &[OsString]) -> ResourceExecutorV2 {
    let Some(program) = argv.first().and_then(|value| Path::new(value).file_name()) else {
        return ResourceExecutorV2::Unknown;
    };
    if program != "docker" {
        return ResourceExecutorV2::Unknown;
    }
    let mut arguments = argv.iter().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--context" {
            if arguments.next().is_some_and(|value| value == "orbstack") {
                return ResourceExecutorV2::Orbstack;
            }
        } else if argument == "--context=orbstack" {
            return ResourceExecutorV2::Orbstack;
        }
    }
    ResourceExecutorV2::Docker
}

fn classify_guard_result(
    result: ProcessResult,
    cancellation: &CancellationToken,
) -> Result<(), GuardExecError> {
    match result.termination {
        ProcessTermination::TimedOut => Err(GuardExecError::TimedOut),
        ProcessTermination::Cancelled => match cancellation.reason() {
            Some(commit_ci_preflight::process::CancellationReason::ResourcePressure) => {
                Err(GuardExecError::ResourcePressure)
            }
            Some(commit_ci_preflight::process::CancellationReason::User) | None => {
                Err(GuardExecError::UserCancelled)
            }
        },
        ProcessTermination::Completed => {
            if cancellation.reason() == Some(commit_ci_preflight::process::CancellationReason::User)
            {
                return Err(GuardExecError::UserCancelled);
            }
            match result.exit {
                Some(exit) if exit.success && exit.code == Some(0) => Ok(()),
                Some(exit) if exit.code.is_some_and(|code| (1..=255).contains(&code)) => Err(
                    GuardExecError::ChildExit(exit.code.expect("checked child exit code")),
                ),
                Some(_) | None => Err(GuardExecError::InternalFailure),
            }
        }
    }
}

fn print_verify_benchmark(
    receipt_path: &Path,
    expected_commit: &str,
    expected_os: &str,
    expected_arch: &str,
    expected_ci_environment: Option<&str>,
    expected_runtime_flavor: Option<&str>,
    json: bool,
) -> Result<(), CliError> {
    let input = fs::read(receipt_path).map_err(|source| {
        CliError::BenchmarkVerification(BenchmarkError::Io {
            path: receipt_path.to_path_buf(),
            source,
        })
    })?;
    let envelope = verify_benchmark_document(
        &input,
        expected_commit,
        expected_os,
        expected_arch,
        expected_ci_environment,
        expected_runtime_flavor,
    )
    .map_err(CliError::BenchmarkVerification)?;
    if json {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "benchmark_id": envelope.benchmark_id,
                "decision": "PASS",
                "expected_commit": expected_commit,
                "expected_os": expected_os,
                "expected_arch": expected_arch,
            }))
            .map_err(CliError::internal)?
        );
    } else {
        println!("Benchmark: {}", envelope.benchmark_id);
        println!("Decision: PASS");
    }
    Ok(())
}

fn collect_runtime_probe(
    path: &Path,
    cancellation: &CancellationToken,
    supervisor: &dyn SupervisorPort,
) -> Result<RuntimeProbe, CliError> {
    let envelope = load_plan(path)?;
    let runtime = runtime_for(envelope.plan.runtime.kind).map_err(CliError::Runtime)?;
    let generation = doctor_guard(&envelope);
    let current_dir = std::env::current_dir().map_err(CliError::internal)?;
    runtime
        .probe(
            &envelope,
            supervisor,
            &current_dir,
            cancellation,
            &generation,
        )
        .map_err(CliError::Runtime)
}

fn print_github_actions_migration(path: &Path, json: bool) -> Result<(), CliError> {
    let report = analyze_workflow_file(path).map_err(CliError::GithubActions)?;
    if json {
        let bytes = report.json_bytes().map_err(CliError::GithubActions)?;
        println!("{}", String::from_utf8(bytes).map_err(CliError::internal)?);
    } else {
        println!(
            "Workflow: {}",
            report.workflow_name.as_deref().unwrap_or("unnamed")
        );
        println!("Readiness: {:?}", report.readiness);
        println!("Translated: {}", report.summary.translated);
        println!("Manual review: {}", report.summary.manual_review);
        println!("Unsupported: {}", report.summary.unsupported);
        println!("Proposed checks: {}", report.proposed_checks.len());
        println!("Executable configuration emitted: false");
        println!("Read-only: no action or command was executed.");
    }
    if report.readiness == MigrationReadiness::Blocked {
        Err(CliError::MigrationBlocked)
    } else {
        Ok(())
    }
}

fn print_verify(
    receipt_path: &Path,
    policy_path: &Path,
    expected_commit: &str,
    evaluated_at_utc: Option<&str>,
    json: bool,
) -> Result<(), CliError> {
    validate_verification_policy_path(policy_path).map_err(CliError::Verification)?;
    let evaluated_at = evaluated_at_utc
        .map(str::to_owned)
        .map(Ok)
        .unwrap_or_else(system_evaluated_at_utc)
        .map_err(CliError::Verification)?;
    let report = match fs::read(receipt_path) {
        Ok(receipt) => verify_receipt_document_for_policy_path(
            &receipt,
            policy_path,
            expected_commit,
            &evaluated_at,
        ),
        Err(_) => receipt_input_failure_report(expected_commit, &evaluated_at),
    }
    .map_err(CliError::Verification)?;
    if json {
        let bytes = report.canonical_bytes().map_err(CliError::Verification)?;
        println!("{}", String::from_utf8(bytes).map_err(CliError::internal)?);
    } else {
        println!("Integrity: {:?}", report.integrity_status);
        println!("Policy: {:?}", report.policy_status);
        for finding in &report.findings {
            println!(
                "  - {} [{}]: {}",
                finding.code, finding.field, finding.message
            );
        }
        println!("Decision: {:?}", report.decision);
    }
    if report.decision == VerificationDecision::Pass {
        Ok(())
    } else {
        Err(CliError::VerifyOutcome(report.decision))
    }
}

fn print_run(
    path: &Path,
    location: &CacheLocationArgs,
    generation: u64,
    admission_timeout_seconds: u64,
    json: bool,
) -> Result<(), CliError> {
    if config_schema_version(path)?.as_deref() == Some("2.0") {
        return print_matrix_run(path, location, generation, admission_timeout_seconds, json);
    }
    let envelope = load_plan(path)?;
    if !envelope.plan.environment.remote_secret_only.is_empty() {
        return Err(CliError::Run(RunError::RemoteSecretOnly));
    }
    let root = resolve_cache_root(location)?;
    let cache = ManagedCache::initialize(root).map_err(CliError::Cache)?;
    if let Some(storage) = &envelope.plan.storage {
        let storage_probe = SystemStorageProbe;
        preflight_storage(storage, &cache.root().path, &storage_probe)
            .map_err(RunError::Storage)
            .map_err(CliError::Run)?;
    }
    let runtime = runtime_for(envelope.plan.runtime.kind).map_err(CliError::Runtime)?;
    let supervisor = Arc::new(ProcessSupervisor::standard());
    let cancellation = CancellationToken::default();
    install_cancellation_handler(&cancellation)?;
    let runtime_preflight = preflight_runtime_capabilities(
        &envelope,
        runtime.as_ref(),
        &DockerRuntimeCapabilityProbe,
        supervisor.as_ref(),
        &location.repository,
        &cancellation,
        &doctor_guard(&envelope),
    )
    .map_err(CliError::Runtime)?;
    let journal = RunJournalStore::initialize(&cache.root().path).map_err(CliError::RunJournal)?;
    let journal_id = new_journal_id(&envelope.plan_digest, generation)?;
    let journal_clock = SystemClock;
    journal
        .create_run(
            &journal_id,
            &journal_clock.now_utc().map_err(CliError::Run)?,
        )
        .map_err(CliError::RunJournal)?;
    let mut lifecycle = JournalLifecycleObserver {
        store: &journal,
        run_id: &journal_id,
        clock: &journal_clock,
    };
    let source_identity = RunIdentity {
        project: envelope.plan.project.clone(),
        commit: None,
        config_digest: envelope.plan_digest.clone(),
        generation: generation.to_string(),
    };
    let source_generation = GenerationGuard::new(source_identity.clone());
    let commit = match resolve_clean_head(
        &location.repository,
        &envelope.plan.receipt.output,
        supervisor.as_ref(),
        &cancellation,
        &source_generation,
        &source_identity,
    ) {
        Ok(commit) => commit,
        Err(error) => {
            lifecycle.fail(RunFailureKindV1::PreparationFailed)?;
            return Err(CliError::Run(RunError::SourceSnapshot(error)));
        }
    };
    let source_identity = RunIdentity {
        commit: Some(commit.clone()),
        ..source_identity
    };
    source_generation
        .replace(source_identity.clone())
        .map_err(|error| CliError::Run(RunError::Process(error)))?;
    let source_resource = match journal.reserve_resource(&journal_id, "source-snapshot-v1") {
        Ok(path) => path,
        Err(error) => {
            lifecycle.fail(RunFailureKindV1::PreparationFailed)?;
            return Err(CliError::RunJournal(error));
        }
    };
    let mut source_snapshot = match SourceSnapshot::materialize(
        &location.repository,
        &commit,
        &source_resource,
        supervisor.as_ref(),
        &cancellation,
        &source_generation,
        &source_identity,
    ) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            lifecycle.fail(RunFailureKindV1::PreparationFailed)?;
            return Err(CliError::Run(RunError::SourceSnapshot(error)));
        }
    };
    if let Err(error) = source_snapshot.prepare_mount_overlay(&envelope) {
        lifecycle.fail(RunFailureKindV1::PreparationFailed)?;
        return Err(CliError::Run(RunError::SourceSnapshot(error)));
    }
    if let Err(error) = journal.bind_source(
        &journal_id,
        &commit,
        &source_snapshot.evidence().manifest_digest,
        source_snapshot.evidence().entry_count,
    ) {
        lifecycle.fail(RunFailureKindV1::PreparationFailed)?;
        return Err(CliError::RunJournal(error));
    }
    let admission =
        AdmissionCoordinator::platform_for(&location.repository).map_err(CliError::Admission)?;
    let guard = match admission.acquire(
        Duration::from_secs(admission_timeout_seconds),
        &cancellation,
    ) {
        Ok(guard) => guard,
        Err(error) => {
            lifecycle.fail(RunFailureKindV1::AdmissionRejected)?;
            return Err(CliError::Admission(error));
        }
    };
    lifecycle.transition_state(RunJournalStateV1::Admitted, None)?;
    if let Err(error) = resource_pre_start(supervisor.clone(), &cancellation) {
        return finalize_run_terminal(
            Err::<(), _>(error),
            std::convert::identity,
            || guard.release(),
            |event| match event {
                RunTerminalJournalEvent::PrimaryFailure(kind) => lifecycle.fail(kind),
                RunTerminalJournalEvent::ReleaseFailure => {
                    lifecycle.transition_state(RunJournalStateV1::CleanupPending, None)
                }
            },
        );
    }
    let watchdog = if ResourcePlatform::current() == ResourcePlatform::MacOs {
        let current_dir = match std::env::current_dir() {
            Ok(path) => path,
            Err(error) => {
                return finalize_run_terminal(
                    Err::<(), _>(CliError::internal(error)),
                    std::convert::identity,
                    || guard.release(),
                    |event| match event {
                        RunTerminalJournalEvent::PrimaryFailure(kind) => lifecycle.fail(kind),
                        RunTerminalJournalEvent::ReleaseFailure => {
                            lifecycle.transition_state(RunJournalStateV1::CleanupPending, None)
                        }
                    },
                );
            }
        };
        Some(ResourceWatchdog::start(
            ResourceProbe::new(SupervisorResourceRunner::new(
                supervisor.clone(),
                current_dir,
                cancellation.clone(),
            )),
            cancellation.clone(),
        ))
    } else {
        None
    };
    let mut completion_barrier = WatchdogCompletionBarrier::new(watchdog);
    let run_result = execute_local_run_with_barrier_and_lifecycle_and_runtime_preflight(
        &RunRequest {
            envelope: &envelope,
            repository: &location.repository,
            cache: &cache,
            generation,
            source_snapshot: Some(&source_snapshot),
        },
        runtime.as_ref(),
        supervisor.as_ref(),
        &cancellation,
        &SystemClock,
        &mut completion_barrier,
        &mut lifecycle,
        runtime_preflight,
    );
    let outcome = finalize_run_terminal(
        run_result.map_err(CliError::Run),
        |outcome| {
            completion_barrier.ensure_joined();
            reconcile_watchdog_outcome(outcome, &mut completion_barrier)
        },
        || guard.release(),
        |event| match event {
            RunTerminalJournalEvent::PrimaryFailure(kind) => lifecycle.fail(kind),
            RunTerminalJournalEvent::ReleaseFailure => {
                lifecycle.transition_state(RunJournalStateV1::CleanupPending, None)
            }
        },
    )?;
    if let Err(error) = source_snapshot.cleanup() {
        lifecycle.transition_state(RunJournalStateV1::CleanupPending, None)?;
        return Err(CliError::Run(RunError::SourceSnapshot(error)));
    }
    lifecycle
        .transition(RunLifecyclePhase::Sealed)
        .map_err(CliError::Run)?;
    if json {
        let bytes = outcome
            .published_canonical_bytes()
            .map_err(CliError::internal)?;
        println!("{}", String::from_utf8(bytes).map_err(CliError::internal)?);
    } else {
        println!("Run: {}", outcome.receipt.receipt.run.run_id);
        println!("Commit: {}", outcome.receipt.receipt.repository.commit_sha);
        for check in &outcome.receipt.receipt.checks {
            println!("  - {}: {:?}", check.id, check.status);
        }
        println!("Receipt: {}", envelope.plan.receipt.output);
        println!("Overall: {:?}", outcome.receipt.receipt.overall_status);
    }
    if outcome.exit_code() == 0 {
        Ok(())
    } else {
        Err(CliError::RunOutcome(outcome.receipt.receipt.overall_status))
    }
}

fn config_schema_version(path: &Path) -> Result<Option<String>, CliError> {
    let source = fs::read_to_string(path).map_err(|source| {
        CliError::usage(ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })
    })?;
    let value: toml::Value = toml::from_str(&source).map_err(CliError::usage)?;
    Ok(value
        .get("schema_version")
        .and_then(toml::Value::as_str)
        .map(str::to_owned))
}

fn print_matrix_run(
    path: &Path,
    location: &CacheLocationArgs,
    generation: u64,
    admission_timeout_seconds: u64,
    json: bool,
) -> Result<(), CliError> {
    let envelope = MatrixConfigV2::load(path)
        .map_err(CliError::Matrix)?
        .into_plan()
        .map_err(CliError::Matrix)?;
    let root = resolve_cache_root(location)?;
    let cache = ManagedCache::initialize(root).map_err(CliError::Cache)?;
    let journal = RunJournalStore::initialize(&cache.root().path).map_err(CliError::RunJournal)?;
    let journal_id = new_journal_id(&envelope.plan_digest, generation)?;
    let journal_clock = SystemClock;
    journal
        .create_run(
            &journal_id,
            &journal_clock.now_utc().map_err(CliError::Run)?,
        )
        .map_err(CliError::RunJournal)?;
    let mut lifecycle = JournalLifecycleObserver {
        store: &journal,
        run_id: &journal_id,
        clock: &journal_clock,
    };
    let supervisor = Arc::new(ProcessSupervisor::standard());
    let cancellation = CancellationToken::default();
    install_cancellation_handler(&cancellation)?;
    let admission =
        AdmissionCoordinator::platform_for(&location.repository).map_err(CliError::Admission)?;
    let guard = match admission.acquire(
        Duration::from_secs(admission_timeout_seconds),
        &cancellation,
    ) {
        Ok(guard) => guard,
        Err(error) => {
            lifecycle.fail(RunFailureKindV1::AdmissionRejected)?;
            return Err(CliError::Admission(error));
        }
    };
    lifecycle.transition_state(RunJournalStateV1::Admitted, None)?;
    if let Err(error) = resource_pre_start(supervisor.clone(), &cancellation) {
        return finalize_run_terminal(
            Err::<(), _>(error),
            std::convert::identity,
            || guard.release(),
            |event| match event {
                RunTerminalJournalEvent::PrimaryFailure(kind) => lifecycle.fail(kind),
                RunTerminalJournalEvent::ReleaseFailure => {
                    lifecycle.transition_state(RunJournalStateV1::CleanupPending, None)
                }
            },
        );
    }
    let watchdog = if ResourcePlatform::current() == ResourcePlatform::MacOs {
        let current_dir = match std::env::current_dir() {
            Ok(path) => path,
            Err(error) => {
                return finalize_run_terminal(
                    Err::<(), _>(CliError::internal(error)),
                    std::convert::identity,
                    || guard.release(),
                    |event| match event {
                        RunTerminalJournalEvent::PrimaryFailure(kind) => lifecycle.fail(kind),
                        RunTerminalJournalEvent::ReleaseFailure => {
                            lifecycle.transition_state(RunJournalStateV1::CleanupPending, None)
                        }
                    },
                );
            }
        };
        Some(ResourceWatchdog::start(
            ResourceProbe::new(SupervisorResourceRunner::new(
                supervisor.clone(),
                current_dir,
                cancellation.clone(),
            )),
            cancellation.clone(),
        ))
    } else {
        None
    };
    let mut completion_barrier = WatchdogCompletionBarrier::new(watchdog);
    lifecycle
        .transition(RunLifecyclePhase::Prepared)
        .map_err(CliError::Run)?;
    lifecycle
        .transition(RunLifecyclePhase::Executing)
        .map_err(CliError::Run)?;
    let result = execute_matrix_run_v2(
        &MatrixRunRequestV2 {
            envelope: &envelope,
            repository: &location.repository,
            cache: &cache,
            generation,
        },
        supervisor.as_ref(),
        &cancellation,
        &SystemClock,
        &mut completion_barrier,
    )
    .map_err(CliError::Matrix);
    let outcome = finalize_run_terminal(
        result,
        |result| {
            completion_barrier.ensure_joined();
            if let Some(error) = completion_barrier.take_join_error() {
                Err(CliError::Resource(ResourceGuardError::Watchdog(error)))
            } else {
                result
            }
        },
        || guard.release(),
        |event| match event {
            RunTerminalJournalEvent::PrimaryFailure(kind) => lifecycle.fail(kind),
            RunTerminalJournalEvent::ReleaseFailure => {
                lifecycle.transition_state(RunJournalStateV1::CleanupPending, None)
            }
        },
    )?;
    lifecycle
        .transition(RunLifecyclePhase::Finalizing)
        .map_err(CliError::Run)?;
    lifecycle
        .transition(RunLifecyclePhase::Sealed)
        .map_err(CliError::Run)?;
    if json {
        let bytes = outcome
            .receipt
            .canonical_bytes()
            .map_err(CliError::Matrix)?;
        println!("{}", String::from_utf8(bytes).map_err(CliError::internal)?);
    } else {
        println!("Matrix receipt: {}", outcome.receipt.receipt_id);
        println!(
            "Runtimes: {}",
            outcome.receipt.receipt.runtime_receipts.len()
        );
        println!("Receipt: {}", outcome.receipt_path.display());
        println!("Status: {:?}", outcome.receipt.receipt.overall_status);
    }
    match outcome.receipt.receipt.overall_status {
        EvidenceStatus::Pass => Ok(()),
        status => Err(CliError::RunOutcome(status)),
    }
}

#[derive(Serialize)]
struct JournalIdInput<'a> {
    schema_version: &'static str,
    plan_digest: &'a str,
    generation: u64,
    unix_nanos: u128,
    process_id: u32,
    sequence: u64,
}

fn new_journal_id(plan_digest: &str, generation: u64) -> Result<String, CliError> {
    let unix_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(CliError::internal)?
        .as_nanos();
    let digest = canonical_digest(&JournalIdInput {
        schema_version: RUN_JOURNAL_SCHEMA_VERSION,
        plan_digest,
        generation,
        unix_nanos,
        process_id: std::process::id(),
        sequence: JOURNAL_SEQUENCE.fetch_add(1, Ordering::Relaxed),
    })
    .map_err(CliError::internal)?;
    digest
        .strip_prefix("sha256:")
        .filter(|run_id| run_id.len() == 64)
        .map(str::to_owned)
        .ok_or_else(|| {
            CliError::internal(std::io::Error::other(
                "canonical journal digest has an invalid format",
            ))
        })
}

struct JournalLifecycleObserver<'a> {
    store: &'a RunJournalStore,
    run_id: &'a str,
    clock: &'a dyn Clock,
}

impl JournalLifecycleObserver<'_> {
    fn transition_state(
        &mut self,
        state: RunJournalStateV1,
        failure_kind: Option<RunFailureKindV1>,
    ) -> Result<(), CliError> {
        let at_utc = self.clock.now_utc().map_err(CliError::Run)?;
        self.store
            .transition(self.run_id, state, &at_utc, failure_kind)
            .map_err(CliError::RunJournal)?;
        Ok(())
    }

    fn fail(&mut self, kind: RunFailureKindV1) -> Result<(), CliError> {
        self.transition_state(RunJournalStateV1::Failed, Some(kind))
    }
}

impl RunLifecycleObserver for JournalLifecycleObserver<'_> {
    fn transition(&mut self, phase: RunLifecyclePhase) -> Result<(), RunError> {
        let state = match phase {
            RunLifecyclePhase::Prepared => RunJournalStateV1::Prepared,
            RunLifecyclePhase::Executing => RunJournalStateV1::Executing,
            RunLifecyclePhase::Finalizing => RunJournalStateV1::Finalizing,
            RunLifecyclePhase::Sealed => RunJournalStateV1::Sealed,
        };
        let at_utc = self.clock.now_utc()?;
        self.store
            .transition(self.run_id, state, &at_utc, None)
            .map_err(|_| RunError::Invariant("run journal transition failed"))?;
        Ok(())
    }
}

fn run_failure_kind(error: &RunError) -> RunFailureKindV1 {
    match error {
        RunError::ResourcePressure => RunFailureKindV1::ResourcePressure,
        RunError::StaleCommit => RunFailureKindV1::StaleCommit,
        RunError::Workspace(_) | RunError::Cache(_) | RunError::Storage(_) => {
            RunFailureKindV1::PreparationFailed
        }
        RunError::Runtime(_) | RunError::Process(_) => RunFailureKindV1::ExecutionFailed,
        RunError::Receipt(_) | RunError::UnsafeReceiptPath => RunFailureKindV1::FinalizationFailed,
        RunError::Invariant(_) => RunFailureKindV1::Invariant,
        _ => RunFailureKindV1::Unknown,
    }
}

fn cli_failure_kind(error: &CliError) -> RunFailureKindV1 {
    match error {
        CliError::Run(error) => run_failure_kind(error),
        CliError::Matrix(MatrixError::Run(error)) => run_failure_kind(error),
        CliError::Matrix(MatrixError::Runtime(_)) => RunFailureKindV1::ExecutionFailed,
        CliError::Resource(_) => RunFailureKindV1::ResourcePressure,
        CliError::Admission(_) => RunFailureKindV1::CleanupFailed,
        _ => RunFailureKindV1::Unknown,
    }
}

fn load_plan(path: &Path) -> Result<ExecutionPlanEnvelopeV1, CliError> {
    ConfigV1::load(path)
        .map_err(CliError::usage)?
        .into_plan()
        .map_err(CliError::usage)
}

fn print_plan(path: &Path, json: bool) -> Result<(), CliError> {
    if config_schema_version(path)?.as_deref() == Some("2.0") {
        let envelope = MatrixConfigV2::load(path)
            .map_err(CliError::Matrix)?
            .into_plan()
            .map_err(CliError::Matrix)?;
        if json {
            let bytes = envelope.canonical_bytes().map_err(CliError::Matrix)?;
            println!("{}", String::from_utf8(bytes).map_err(CliError::internal)?);
        } else {
            println!("Matrix plan: {}", envelope.plan_digest);
            println!("Project: {}", envelope.plan.project);
            for runtime in &envelope.plan.runtimes {
                println!("  - {}: {}", runtime.id, runtime.configuration_digest);
            }
            println!("Read-only: no command was executed.");
        }
        return Ok(());
    }
    let envelope = load_plan(path)?;
    if json {
        let bytes = envelope.canonical_bytes().map_err(CliError::usage)?;
        println!("{}", String::from_utf8(bytes).map_err(CliError::internal)?);
    } else {
        println!("Plan: {}", envelope.plan_digest);
        println!("Project: {}", envelope.plan.project);
        println!("Runtime: {:?}", envelope.plan.runtime.kind);
        println!("Checks: {}", envelope.plan.checks.len());
        for check in &envelope.plan.checks {
            println!("  - {}", check.id);
        }
        println!("Read-only: no command was executed.");
    }
    Ok(())
}

fn print_dry_run(path: &Path, location: &CacheLocationArgs, json: bool) -> Result<(), CliError> {
    if config_schema_version(path)?.as_deref() == Some("2.0") {
        return print_matrix_dry_run(path, location, json);
    }
    let envelope = load_plan(path)?;
    let cache = resolve_cache_root(location)?;
    let workspace = WorkspacePlanV1::build(&envelope, &location.repository, &cache)
        .map_err(CliError::Workspace)?;
    let runtime = runtime_for(envelope.plan.runtime.kind).map_err(CliError::Runtime)?;
    let dry_run = runtime
        .dry_run(&envelope, &workspace)
        .map_err(CliError::Runtime)?;
    if json {
        println!(
            "{}",
            serde_json::to_string(&dry_run).map_err(CliError::internal)?
        );
    } else {
        print_human_dry_run(&dry_run)?;
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct MatrixDryRunReportV2 {
    schema_version: &'static str,
    plan_digest: String,
    runtimes: Vec<MatrixRuntimeDryRunV2>,
}

#[derive(Debug, Serialize)]
struct MatrixRuntimeDryRunV2 {
    runtime_id: String,
    configuration_digest: String,
    dry_run: DryRunPlan,
}

fn print_matrix_dry_run(
    path: &Path,
    location: &CacheLocationArgs,
    json: bool,
) -> Result<(), CliError> {
    let envelope = MatrixConfigV2::load(path)
        .map_err(CliError::Matrix)?
        .into_plan()
        .map_err(CliError::Matrix)?;
    let cache = resolve_cache_root(location)?;
    let mut runtimes = Vec::with_capacity(envelope.plan.runtimes.len());
    for (runtime_id, runtime_envelope) in envelope.runtime_envelopes().map_err(CliError::Matrix)? {
        let workspace = WorkspacePlanV1::build(&runtime_envelope, &location.repository, &cache)
            .map_err(CliError::Workspace)?;
        let runtime = runtime_for(runtime_envelope.plan.runtime.kind).map_err(CliError::Runtime)?;
        let dry_run = runtime
            .dry_run(&runtime_envelope, &workspace)
            .map_err(CliError::Runtime)?;
        runtimes.push(MatrixRuntimeDryRunV2 {
            runtime_id,
            configuration_digest: runtime_envelope.plan_digest,
            dry_run,
        });
    }
    let report = MatrixDryRunReportV2 {
        schema_version: "2.0",
        plan_digest: envelope.plan_digest,
        runtimes,
    };
    if json {
        println!(
            "{}",
            serde_json::to_string(&report).map_err(CliError::internal)?
        );
    } else {
        println!("Matrix plan: {}", report.plan_digest);
        for runtime in &report.runtimes {
            println!("Runtime ID: {}", runtime.runtime_id);
            print_human_dry_run(&runtime.dry_run)?;
        }
    }
    Ok(())
}

fn print_human_dry_run(dry_run: &DryRunPlan) -> Result<(), CliError> {
    println!("Plan: {}", dry_run.plan_digest);
    println!("Runtime: {:?}", dry_run.runtime);
    println!(
        "Workspace mounts: {} explicit bindings",
        dry_run.workspace.mounts.len()
    );
    for check in &dry_run.checks {
        println!("Check: {}", check.id);
        println!("  Program: {}", check.program);
        println!(
            "  Argv (not shell): {}",
            serde_json::to_string(&check.argv).map_err(CliError::internal)?
        );
    }
    println!("Dry-run: no command was executed.");
    Ok(())
}

fn run_cache_command(action: CacheCommand) -> Result<(), CliError> {
    match action {
        CacheCommand::Path { location, json } => {
            let root = resolve_cache_root(&location)?;
            print_serializable_or_path(&root, &root.path, json)
        }
        CacheCommand::Init { location, json } => {
            let root = resolve_cache_root(&location)?;
            let cache = ManagedCache::initialize(root).map_err(CliError::Cache)?;
            print_serializable_or_path(cache.root(), &cache.root().path, json)
        }
        CacheCommand::Inventory {
            location,
            disk_budget_bytes,
            json,
        } => {
            let root = resolve_cache_root(&location)?;
            let inventory = ManagedCache::open(root)
                .map_err(CliError::Cache)?
                .with_disk_budget(disk_budget_bytes)
                .map_err(CliError::Cache)?
                .inventory()
                .map_err(CliError::Cache)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string(&inventory).map_err(CliError::internal)?
                );
            } else {
                println!("Cache root: {}", inventory.root.display());
                println!("Entries: {}", inventory.entries.len());
                println!("Bytes: {}", inventory.total_bytes);
                println!("Budget exceeded: {}", inventory.budget_exceeded);
            }
            Ok(())
        }
        CacheCommand::Cleanup {
            location,
            dry_run,
            disk_budget_bytes,
            json,
        } => {
            if !dry_run {
                return Err(CliError::usage(CliMessageError(
                    "cache cleanup requires --dry-run; deletion is unavailable",
                )));
            }
            let root = resolve_cache_root(&location)?;
            let plan = ManagedCache::open(root)
                .map_err(CliError::Cache)?
                .with_disk_budget(disk_budget_bytes)
                .map_err(CliError::Cache)?
                .cleanup_dry_run()
                .map_err(CliError::Cache)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string(&plan).map_err(CliError::internal)?
                );
            } else {
                println!("Cache root: {}", plan.root.display());
                println!("Candidates: {}", plan.candidates.len());
                println!("Reclaimable bytes: {}", plan.reclaimable_bytes);
                println!("Deletion performed: false");
            }
            Ok(())
        }
    }
}

fn run_recover_command(action: RecoverCommand) -> Result<(), CliError> {
    match action {
        RecoverCommand::Status { location, json } => {
            let root = resolve_cache_root(&location)?;
            let cache = ManagedCache::open(root).map_err(CliError::Cache)?;
            let report =
                match RunJournalStore::open(&cache.root().path).map_err(CliError::RunJournal)? {
                    Some(store) => store.status().map_err(CliError::RunJournal)?,
                    None => RecoveryStatusV1 {
                        schema_version: RUN_JOURNAL_SCHEMA_VERSION.to_owned(),
                        runs: Vec::new(),
                    },
                };
            if json {
                println!(
                    "{}",
                    serde_json::to_string(&report).map_err(CliError::internal)?
                );
            } else {
                println!("Run journals: {}", report.runs.len());
                for run in report.runs {
                    println!("  - {}: {:?}", run.run_id, run.classification);
                }
                println!("Read-only: no state was changed.");
            }
            Ok(())
        }
        RecoverCommand::Apply {
            run_id,
            location,
            json,
        } => {
            let root = resolve_cache_root(&location)?;
            let cache = ManagedCache::open(root).map_err(CliError::Cache)?;
            let store = RunJournalStore::open(&cache.root().path)
                .map_err(CliError::RunJournal)?
                .ok_or(CliError::RunJournal(RunJournalError::NonActionable))?;
            let result = store.apply(&run_id).map_err(CliError::RunJournal)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string(&result).map_err(CliError::internal)?
                );
            } else {
                println!("Run: {}", result.run_id);
                println!("Recovery outcome: {:?}", result.outcome);
            }
            Ok(())
        }
    }
}

fn run_admission_command(action: AdmissionCommand) -> Result<(), CliError> {
    match action {
        AdmissionCommand::Status {
            json,
            timeout_seconds,
        } => {
            let cancellation = CancellationToken::default();
            let status = AdmissionCoordinator::platform()
                .map_err(CliError::Admission)?
                .status_with_timeout(Duration::from_secs(timeout_seconds), &cancellation)
                .map_err(CliError::Admission)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string(&status).map_err(CliError::internal)?
                );
            } else {
                println!("Admission schema: {ADMISSION_STATUS_SCHEMA_VERSION}");
                println!("Active: {}", status.active);
                println!("Queued: {}", status.queue_count);
                println!("Slot lock: {}", status.slot.state);
                println!("Slot owner/run: {:?}", status.slot.owner_run_id);
                println!("Slot lease: {}", status.slot.lease_state);
                println!("Queue lock: {}", status.queue_lock.state);
                for ticket in &status.ticket_ids {
                    println!("  - {ticket}");
                }
                println!("Note: {}", status.process_visibility_note);
            }
            Ok(())
        }
    }
}

fn run_resource_command(action: ResourceAction) -> Result<(), CliError> {
    match action {
        ResourceAction::Status { json } => print_resource_status(json),
        ResourceAction::History { json } => print_resource_history(json),
    }
}

fn print_resource_history(json: bool) -> Result<(), CliError> {
    let report = ResourceHistoryStore::platform()
        .and_then(|store| store.report_v2())
        .map_err(CliError::internal)?;
    if json {
        println!(
            "{}",
            serde_json::to_string(&report).map_err(CliError::internal)?
        );
    } else {
        println!("Resource history schema: {}", report.schema_version);
        println!("Records: {}", report.record_count);
        for record in report.records {
            println!(
                "{} {} {} {:?} {:?} {}ms {:?}",
                record.started_at_unix_seconds,
                record.context.profile,
                record.context.workload_family,
                record.context.executor,
                record.context.execution_mode,
                record.duration_milliseconds,
                record.outcome
            );
        }
    }
    Ok(())
}

fn print_resource_status(json: bool) -> Result<(), CliError> {
    let status = if ResourcePlatform::current() != ResourcePlatform::MacOs {
        unsupported_status()
    } else {
        let cancellation = CancellationToken::default();
        let supervisor = Arc::new(ProcessSupervisor::standard());
        let current_dir = std::env::current_dir().map_err(CliError::internal)?;
        let runner = SupervisorResourceRunner::new(supervisor, current_dir, cancellation);
        match ResourceProbe::new(runner).sample() {
            Ok(snapshot) => status_from_snapshot(&snapshot)
                .map_err(|error| CliError::Resource(ResourceGuardError::Probe(error)))?,
            Err(_) => unknown_status(),
        }
    };
    if json {
        println!(
            "{}",
            serde_json::to_string(&status).map_err(CliError::internal)?
        );
    } else {
        println!("Resource schema: {}", status.schema_version);
        println!("Policy: {}", status.policy_version);
        println!("Platform: {}", status.platform);
        println!("Capability: {:?}", status.capability);
        println!("Decision: {:?}", status.decision);
    }
    Ok(())
}

fn resource_pre_start(
    supervisor: Arc<ProcessSupervisor>,
    cancellation: &CancellationToken,
) -> Result<Option<ResourceSnapshot>, CliError> {
    if ResourcePlatform::current() != ResourcePlatform::MacOs {
        return Ok(None);
    }
    let current_dir = std::env::current_dir().map_err(CliError::internal)?;
    let runner = SupervisorResourceRunner::new(supervisor, current_dir, cancellation.clone());
    let snapshot = ResourceProbe::new(runner)
        .sample()
        .map_err(|error| CliError::Resource(ResourceGuardError::Probe(error)))?;
    match evaluate_pre_start(&snapshot)
        .map_err(|error| CliError::Resource(ResourceGuardError::Probe(error)))?
    {
        commit_ci_preflight::resource::PreStartDecision::Admit => Ok(Some(snapshot)),
        commit_ci_preflight::resource::PreStartDecision::Deny => {
            Err(CliError::Resource(ResourceGuardError::PreStartDenied))
        }
    }
}

fn finalize_benchmark_terminal<T>(
    primary: Result<T, CliError>,
    release: impl FnOnce() -> Result<(), AdmissionError>,
) -> Result<T, CliError> {
    match finalize_owned_terminal(primary, std::convert::identity, release) {
        Ok(value) => Ok(value),
        Err(TerminalFailure::Primary(error)) => Err(error),
        Err(TerminalFailure::Release(error)) => Err(CliError::Admission(error)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunTerminalJournalEvent {
    PrimaryFailure(RunFailureKindV1),
    ReleaseFailure,
}

fn finalize_run_terminal<T>(
    primary: Result<T, CliError>,
    complete_owned: impl FnOnce(Result<T, CliError>) -> Result<T, CliError>,
    release: impl FnOnce() -> Result<(), AdmissionError>,
    mut journal: impl FnMut(RunTerminalJournalEvent) -> Result<(), CliError>,
) -> Result<T, CliError> {
    match finalize_owned_terminal(primary, complete_owned, release) {
        Ok(value) => Ok(value),
        Err(TerminalFailure::Primary(error)) => {
            journal(RunTerminalJournalEvent::PrimaryFailure(cli_failure_kind(
                &error,
            )))?;
            Err(error)
        }
        Err(TerminalFailure::Release(error)) => {
            journal(RunTerminalJournalEvent::ReleaseFailure)?;
            Err(CliError::Admission(error))
        }
    }
}

struct GuardExecSession {
    admission: Option<AdmissionGuard>,
    watchdog: WatchdogCompletionBarrier,
    resource_observation: Option<GuardResourceObservation>,
}

struct GuardResourceObservation {
    observation: ResourceObservation,
    store: ResourceHistoryStore,
    context: ResourceExecutionContextV2,
    started_at_unix_seconds: u64,
    started_at: Instant,
}

impl GuardResourceObservation {
    fn persist(
        self,
        outcome: ResourceRunOutcome,
        terminal_detail: Option<ResourceTerminalDetailV2>,
        watchdog_trip_reason: Option<WatchdogTripReason>,
    ) {
        let Some(summary) = self.observation.summary() else {
            eprintln!("warning: local resource observation could not be summarized");
            return;
        };
        let duration_milliseconds =
            u64::try_from(self.started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
        let record = ResourceHistoryRecordV2::from_summary(
            self.context,
            self.started_at_unix_seconds,
            duration_milliseconds,
            outcome,
            terminal_detail,
            watchdog_trip_reason,
            &summary,
        );
        if record
            .and_then(|record| self.store.append_v2(&record))
            .is_err()
        {
            eprintln!("warning: local resource history could not be updated");
        }
    }
}

impl GuardExecSession {
    fn acquire(
        cancellation: &CancellationToken,
        admission_timeout: Duration,
    ) -> Result<Self, CliError> {
        let admission = AdmissionCoordinator::platform().map_err(CliError::Admission)?;
        let guard = admission
            .acquire(admission_timeout, cancellation)
            .map_err(CliError::Admission)?;
        Ok(Self {
            admission: Some(guard),
            watchdog: WatchdogCompletionBarrier::new(None),
            resource_observation: None,
        })
    }

    fn start_watchdog(
        &mut self,
        supervisor: Arc<ProcessSupervisor>,
        current_dir: PathBuf,
        cancellation: CancellationToken,
        baseline: Option<ResourceSnapshot>,
        context: ResourceExecutionContextV2,
        history_enabled: bool,
    ) {
        if ResourcePlatform::current() == ResourcePlatform::MacOs {
            let runner =
                SupervisorResourceRunner::new(supervisor, current_dir, cancellation.clone());
            if history_enabled && let Some(baseline) = baseline {
                match ResourceHistoryStore::platform() {
                    Ok(store) => {
                        let observation = ResourceObservation::new(baseline);
                        self.watchdog =
                            WatchdogCompletionBarrier::new(Some(ResourceWatchdog::start_observed(
                                ResourceProbe::new(runner),
                                cancellation,
                                observation.clone(),
                            )));
                        self.resource_observation = Some(GuardResourceObservation {
                            observation,
                            store,
                            context,
                            started_at_unix_seconds: SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            started_at: Instant::now(),
                        });
                        return;
                    }
                    Err(_) => {
                        eprintln!("warning: local resource history is unavailable");
                    }
                }
            }
            self.watchdog = WatchdogCompletionBarrier::new(Some(ResourceWatchdog::start(
                ResourceProbe::new(runner),
                cancellation,
            )));
        }
    }

    fn finish(
        mut self,
        result: Result<ProcessResult, GuardExecError>,
        cancellation: &CancellationToken,
    ) -> Result<ProcessResult, GuardExecError> {
        self.watchdog.ensure_joined();
        let trip = self.watchdog.trip();
        if let Some(observation) = self.resource_observation.take() {
            observation.persist(
                resource_run_outcome(&result, cancellation, trip),
                resource_terminal_detail(&result, cancellation, trip),
                trip,
            );
        }
        let guard = self
            .admission
            .take()
            .ok_or(GuardExecError::InternalFailure)?;
        finalize_guard_exec_result(
            result,
            cancellation,
            self.watchdog.take_join_error(),
            trip,
            || guard.release().map_err(GuardExecError::Admission),
        )
    }
}

fn resource_run_outcome(
    result: &Result<ProcessResult, GuardExecError>,
    cancellation: &CancellationToken,
    trip: Option<WatchdogTripReason>,
) -> ResourceRunOutcome {
    if trip.is_some() || cancellation.reason() == Some(CancellationReason::ResourcePressure) {
        return ResourceRunOutcome::ResourcePressure;
    }
    if cancellation.reason() == Some(CancellationReason::User)
        || matches!(
            result,
            Ok(ProcessResult {
                termination: ProcessTermination::Cancelled,
                ..
            })
        )
    {
        return ResourceRunOutcome::Cancelled;
    }
    if matches!(
        result,
        Ok(ProcessResult {
            termination: ProcessTermination::TimedOut,
            ..
        }) | Err(GuardExecError::TimedOut)
    ) {
        return ResourceRunOutcome::TimedOut;
    }
    if matches!(
        result,
        Ok(ProcessResult {
            termination: ProcessTermination::Completed,
            exit: Some(exit),
            ..
        }) if exit.success
    ) {
        ResourceRunOutcome::Completed
    } else {
        ResourceRunOutcome::Failed
    }
}

fn resource_terminal_detail(
    result: &Result<ProcessResult, GuardExecError>,
    cancellation: &CancellationToken,
    trip: Option<WatchdogTripReason>,
) -> Option<ResourceTerminalDetailV2> {
    if trip.is_some() || cancellation.reason() == Some(CancellationReason::ResourcePressure) {
        return Some(ResourceTerminalDetailV2::ResourcePressure);
    }
    if cancellation.reason() == Some(CancellationReason::User)
        || matches!(
            result,
            Ok(ProcessResult {
                termination: ProcessTermination::Cancelled,
                ..
            }) | Err(GuardExecError::UserCancelled)
        )
    {
        return Some(ResourceTerminalDetailV2::UserCancelled);
    }
    if matches!(
        result,
        Ok(ProcessResult {
            termination: ProcessTermination::TimedOut,
            ..
        }) | Err(GuardExecError::TimedOut)
    ) {
        return Some(ResourceTerminalDetailV2::TimedOut);
    }
    match result {
        Ok(ProcessResult {
            termination: ProcessTermination::Completed,
            exit: Some(exit),
            ..
        }) if exit.success => None,
        Ok(ProcessResult {
            termination: ProcessTermination::Completed,
            exit: Some(exit),
            ..
        }) if exit.code.is_some_and(|code| (1..=255).contains(&code)) => {
            Some(ResourceTerminalDetailV2::ChildExit {
                exit_code: exit.code.expect("checked child exit code") as u8,
            })
        }
        Err(GuardExecError::ChildExit(code)) if (1..=255).contains(code) => {
            Some(ResourceTerminalDetailV2::ChildExit {
                exit_code: *code as u8,
            })
        }
        Err(GuardExecError::Resource(_)) => Some(ResourceTerminalDetailV2::ResourceMonitorFailure),
        Err(GuardExecError::Process(_)) => {
            Some(ResourceTerminalDetailV2::ProcessSupervisionFailure)
        }
        _ => Some(ResourceTerminalDetailV2::InternalFailure),
    }
}

fn finalize_guard_exec_result(
    result: Result<ProcessResult, GuardExecError>,
    cancellation: &CancellationToken,
    join_error: Option<ResourceProbeError>,
    trip: Option<WatchdogTripReason>,
    release: impl FnOnce() -> Result<(), GuardExecError>,
) -> Result<ProcessResult, GuardExecError> {
    match finalize_owned_terminal(
        result,
        |result| {
            if let Some(error) = join_error {
                Err(GuardExecError::Resource(ResourceGuardError::Watchdog(
                    error,
                )))
            } else if let Some(reason) = trip {
                Err(GuardExecError::Resource(
                    ResourceGuardError::WatchdogTripped(reason),
                ))
            } else if cancellation.reason() == Some(CancellationReason::ResourcePressure) {
                Err(GuardExecError::ResourcePressure)
            } else {
                result
            }
        },
        release,
    ) {
        Ok(value) => Ok(value),
        Err(TerminalFailure::Primary(error) | TerminalFailure::Release(error)) => Err(error),
    }
}

impl Drop for GuardExecSession {
    fn drop(&mut self) {
        self.watchdog.ensure_joined();
        if let Some(guard) = self.admission.take() {
            let _ = guard.release();
        }
    }
}

struct WatchdogCompletionBarrier {
    watchdog: Option<ResourceWatchdog>,
    trip: Option<WatchdogTripReason>,
    join_error: Option<ResourceProbeError>,
}

impl WatchdogCompletionBarrier {
    fn new(watchdog: Option<ResourceWatchdog>) -> Self {
        Self {
            watchdog,
            trip: None,
            join_error: None,
        }
    }

    fn join_once(&mut self) {
        let Some(watchdog) = self.watchdog.take() else {
            return;
        };
        match watchdog.stop_and_join() {
            Ok(trip) => self.trip = trip,
            Err(error) => self.join_error = Some(error),
        }
    }

    fn ensure_joined(&mut self) {
        self.join_once();
    }

    fn trip(&self) -> Option<WatchdogTripReason> {
        self.trip
    }

    fn take_join_error(&mut self) -> Option<ResourceProbeError> {
        self.join_error.take()
    }
}

impl CompletionBarrier for WatchdogCompletionBarrier {
    fn finalize(&mut self, checks: &[CheckEvidence]) -> Result<(), RunError> {
        self.join_once();
        if self.join_error.is_some() {
            return Err(RunError::ResourcePressure);
        }
        match self.trip {
            None => Ok(()),
            Some(WatchdogTripReason::HardPressure | WatchdogTripReason::SoftPressure)
                if checks_have_resource_not_run(checks) =>
            {
                Ok(())
            }
            Some(_) => Err(RunError::ResourcePressure),
        }
    }
}

impl Drop for WatchdogCompletionBarrier {
    fn drop(&mut self) {
        self.join_once();
    }
}

fn reconcile_watchdog_outcome(
    outcome: Result<commit_ci_preflight::run::RunOutcome, CliError>,
    barrier: &mut WatchdogCompletionBarrier,
) -> Result<commit_ci_preflight::run::RunOutcome, CliError> {
    if let Some(error) = barrier.take_join_error() {
        return Err(CliError::Resource(ResourceGuardError::Watchdog(error)));
    }
    match barrier.trip() {
        Some(reason)
            if matches!(
                reason,
                WatchdogTripReason::HardPressure | WatchdogTripReason::SoftPressure
            ) && matches!(&outcome, Ok(value) if outcome_has_resource_not_run(value)) =>
        {
            outcome
        }
        Some(reason) => Err(CliError::Resource(ResourceGuardError::WatchdogTripped(
            reason,
        ))),
        None => outcome,
    }
}

fn checks_have_resource_not_run(checks: &[CheckEvidence]) -> bool {
    checks.iter().any(|check| {
        check.status == EvidenceStatus::NotRun
            && check.incomplete_reason.as_deref() == Some("host resource pressure watchdog tripped")
    })
}

fn outcome_has_resource_not_run(outcome: &commit_ci_preflight::run::RunOutcome) -> bool {
    checks_have_resource_not_run(&outcome.receipt.receipt.checks)
}

fn resolve_cache_root(location: &CacheLocationArgs) -> Result<ResolvedCacheRoot, CliError> {
    ResolvedCacheRoot::resolve(
        &location.repository,
        &CacheRootOptions::from_process(location.cache_dir.clone()),
    )
    .map_err(CliError::Cache)
}

fn print_serializable_or_path(
    value: &impl serde::Serialize,
    path: &Path,
    json: bool,
) -> Result<(), CliError> {
    if json {
        println!(
            "{}",
            serde_json::to_string(value).map_err(CliError::internal)?
        );
    } else {
        println!("{}", path.display());
    }
    Ok(())
}

fn print_doctor(path: &Path, json: bool) -> Result<(), CliError> {
    if config_schema_version(path)?.as_deref() == Some("2.0") {
        return print_matrix_doctor(path, json);
    }
    let envelope = load_plan(path)?;
    let runtime = runtime_for(envelope.plan.runtime.kind).map_err(CliError::Runtime)?;
    let supervisor = ProcessSupervisor::standard();
    let cancellation = CancellationToken::default();
    install_cancellation_handler(&cancellation)?;
    let generation = doctor_guard(&envelope);
    let current_dir = std::env::current_dir().map_err(CliError::internal)?;
    let probe = runtime
        .probe(
            &envelope,
            &supervisor,
            &current_dir,
            &cancellation,
            &generation,
        )
        .map_err(CliError::Runtime)?;
    if json {
        println!(
            "{}",
            serde_json::to_string(&probe).map_err(CliError::internal)?
        );
    } else {
        print_human_probe(&probe);
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct MatrixDoctorReportV2 {
    schema_version: &'static str,
    plan_digest: String,
    runtimes: Vec<MatrixRuntimeDoctorV2>,
}

#[derive(Debug, Serialize)]
struct MatrixRuntimeDoctorV2 {
    runtime_id: String,
    configuration_digest: String,
    probe: RuntimeProbe,
}

fn collect_matrix_doctor_report(
    envelope: &MatrixPlanEnvelopeV2,
    mut probe_runtime: impl FnMut(&ExecutionPlanEnvelopeV1) -> Result<RuntimeProbe, CliError>,
) -> Result<MatrixDoctorReportV2, CliError> {
    let mut runtimes = Vec::with_capacity(envelope.plan.runtimes.len());
    for (runtime_id, runtime_envelope) in envelope.runtime_envelopes().map_err(CliError::Matrix)? {
        let configuration_digest = runtime_envelope.plan_digest.clone();
        let probe = probe_runtime(&runtime_envelope)?;
        runtimes.push(MatrixRuntimeDoctorV2 {
            runtime_id,
            configuration_digest,
            probe,
        });
    }
    Ok(MatrixDoctorReportV2 {
        schema_version: "2.0",
        plan_digest: envelope.plan_digest.clone(),
        runtimes,
    })
}

fn print_matrix_doctor(path: &Path, json: bool) -> Result<(), CliError> {
    let envelope = MatrixConfigV2::load(path)
        .map_err(CliError::Matrix)?
        .into_plan()
        .map_err(CliError::Matrix)?;
    let supervisor = ProcessSupervisor::standard();
    let cancellation = CancellationToken::default();
    install_cancellation_handler(&cancellation)?;
    let current_dir = std::env::current_dir().map_err(CliError::internal)?;
    let report = collect_matrix_doctor_report(&envelope, |runtime_envelope| {
        let runtime = runtime_for(runtime_envelope.plan.runtime.kind).map_err(CliError::Runtime)?;
        let generation = doctor_guard(runtime_envelope);
        runtime
            .probe(
                runtime_envelope,
                &supervisor,
                &current_dir,
                &cancellation,
                &generation,
            )
            .map_err(CliError::Runtime)
    })?;
    if json {
        println!(
            "{}",
            serde_json::to_string(&report).map_err(CliError::internal)?
        );
    } else {
        println!("Matrix plan: {}", report.plan_digest);
        for runtime in &report.runtimes {
            println!("Runtime ID: {}", runtime.runtime_id);
            print_human_probe(&runtime.probe);
        }
    }
    Ok(())
}

fn install_cancellation_handler(cancellation: &CancellationToken) -> Result<(), CliError> {
    let cancellation = cancellation.clone();
    ctrlc::try_set_handler(move || cancellation.cancel()).map_err(CliError::internal)
}

fn print_human_probe(probe: &RuntimeProbe) {
    println!("Runtime: {:?}", probe.runtime);
    println!("Flavor: {:?}", probe.flavor);
    println!(
        "Server version: {}",
        probe.server_version.as_deref().unwrap_or("not reported")
    );
    println!(
        "Operating system: {}",
        probe.operating_system.as_deref().unwrap_or("not reported")
    );
    println!("Containment: {:?}", probe.containment);
    println!("Graceful stop: {:?}", probe.graceful_stop);
    println!("Doctor: PASS");
}

#[derive(Debug)]
enum GuardExecError {
    InvalidAdmissionTimeout,
    InvalidTimeout,
    MissingProgram,
    InvalidResourceProfile,
    InvalidResourceContext,
    InvalidManagedCache,
    InvalidCurrentDirectory,
    CacheInternal,
    Admission(AdmissionError),
    Resource(ResourceGuardError),
    ResourcePressure,
    Process(commit_ci_preflight::process::ProcessError),
    ChildExit(i32),
    TimedOut,
    UserCancelled,
    InternalFailure,
}

impl GuardExecError {
    fn from_cache(_: CacheError) -> Self {
        Self::CacheInternal
    }

    fn from_cli(error: CliError) -> Self {
        match error {
            CliError::Admission(error) => Self::Admission(error),
            CliError::Resource(error) => Self::Resource(error),
            _ => Self::InternalFailure,
        }
    }

    fn exit_code(&self) -> i32 {
        match self {
            Self::InvalidAdmissionTimeout
            | Self::InvalidTimeout
            | Self::MissingProgram
            | Self::InvalidResourceProfile
            | Self::InvalidResourceContext
            | Self::InvalidManagedCache
            | Self::InvalidCurrentDirectory => 2,
            Self::Admission(error) => error.exit_code(),
            Self::Resource(_) | Self::ResourcePressure => 6,
            Self::Process(_) | Self::CacheInternal | Self::InternalFailure => 70,
            Self::ChildExit(code) if (1..=255).contains(code) => *code,
            Self::ChildExit(_) => 70,
            Self::TimedOut => 124,
            Self::UserCancelled => 130,
        }
    }
}

impl fmt::Display for GuardExecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAdmissionTimeout => {
                formatter.write_str("guard exec admission timeout is outside the allowed range")
            }
            Self::InvalidTimeout => {
                formatter.write_str("guard exec timeout is outside the allowed range")
            }
            Self::MissingProgram => formatter.write_str("guard exec requires a program after --"),
            Self::InvalidResourceProfile => formatter.write_str(
                "guard exec resource profile must be 1-64 ASCII letters, digits, hyphens or underscores",
            ),
            Self::InvalidResourceContext => formatter.write_str(
                "guard exec resource context labels must be bounded ASCII tokens and numeric limits must be positive",
            ),
            Self::InvalidManagedCache => formatter.write_str(
                "guard exec managed cache requires exactly one root and at least one source",
            ),
            Self::InvalidCurrentDirectory => {
                formatter.write_str("guard exec current directory could not be canonicalized")
            }
            Self::CacheInternal => formatter.write_str("guard exec managed cache validation failed"),
            Self::Admission(error) => write!(formatter, "{error}"),
            Self::Resource(error) => write!(formatter, "{error}"),
            Self::ResourcePressure => {
                formatter.write_str("guarded execution was cancelled by host resource pressure")
            }
            Self::Process(error) => write!(formatter, "guarded process failed: {error}"),
            Self::ChildExit(code) => write!(formatter, "guarded child exited with code {code}"),
            Self::TimedOut => formatter.write_str("guarded child timed out"),
            Self::UserCancelled => formatter.write_str("guarded child cancelled by user"),
            Self::InternalFailure => formatter.write_str("guarded execution cleanup was uncertain"),
        }
    }
}

impl std::error::Error for GuardExecError {}

#[derive(Debug)]
enum CliError {
    Usage(Box<dyn std::error::Error>),
    Cache(CacheError),
    Workspace(WorkspaceError),
    Runtime(RuntimeError),
    Admission(AdmissionError),
    Resource(ResourceGuardError),
    Run(RunError),
    RunJournal(RunJournalError),
    RunOutcome(EvidenceStatus),
    Matrix(MatrixError),
    Verification(VerificationError),
    VerifyOutcome(VerificationDecision),
    GithubActions(GithubActionsError),
    MigrationBlocked,
    Benchmark(BenchmarkError),
    BenchmarkVerification(BenchmarkError),
    Guard(GuardExecError),
    Internal(Box<dyn std::error::Error>),
}

impl CliError {
    fn usage(error: impl std::error::Error + 'static) -> Self {
        Self::Usage(Box::new(error))
    }

    fn internal(error: impl std::error::Error + 'static) -> Self {
        Self::Internal(Box::new(error))
    }

    fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => 2,
            Self::Cache(error) => error.exit_code(),
            Self::Workspace(_) => 2,
            Self::Runtime(error) => error.exit_code(),
            Self::Admission(error) => error.exit_code(),
            Self::Resource(error) => error.exit_code(),
            Self::Run(error) => error.exit_code(),
            Self::RunJournal(error) => error.exit_code(),
            Self::RunOutcome(EvidenceStatus::Fail) => 1,
            Self::RunOutcome(EvidenceStatus::Pending | EvidenceStatus::NotRun) => 5,
            Self::RunOutcome(EvidenceStatus::Pass) => 0,
            Self::Matrix(error) => match error {
                MatrixError::Run(RunError::ResourcePressure) => 4,
                MatrixError::Runtime(error) => error.exit_code(),
                _ => 2,
            },
            Self::VerifyOutcome(_) => 3,
            Self::GithubActions(_) => 2,
            Self::MigrationBlocked => 4,
            Self::Benchmark(_) => 2,
            Self::BenchmarkVerification(_) => 3,
            Self::Guard(error) => error.exit_code(),
            Self::Verification(VerificationError::Policy(_))
            | Self::Verification(VerificationError::PolicyDocument(_))
            | Self::Verification(VerificationError::TrustedPlan(_))
            | Self::Verification(VerificationError::TrustedPolicyPathRequired)
            | Self::Verification(VerificationError::InvalidExpectedCommit)
            | Self::Verification(VerificationError::InvalidEvaluationTime)
            | Self::Verification(VerificationError::Matrix(_)) => 2,
            Self::Verification(VerificationError::Receipt(_)) => 70,
            Self::Internal(_) => 70,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(error) => write!(formatter, "{error}"),
            Self::Cache(error) => write!(formatter, "{error}"),
            Self::Workspace(error) => write!(formatter, "{error}"),
            Self::Runtime(error) => write!(formatter, "{error}"),
            Self::Admission(error) => write!(formatter, "{error}"),
            Self::Resource(error) => write!(formatter, "{error}"),
            Self::Run(error) => write!(formatter, "{error}"),
            Self::RunJournal(error) => write!(formatter, "{error}"),
            Self::RunOutcome(status) => write!(formatter, "run completed with {status:?}"),
            Self::Matrix(error) => write!(formatter, "{error}"),
            Self::Verification(error) => write!(formatter, "{error}"),
            Self::VerifyOutcome(decision) => {
                write!(formatter, "verification completed with {decision:?}")
            }
            Self::GithubActions(error) => write!(formatter, "{error}"),
            Self::MigrationBlocked => {
                formatter.write_str("workflow migration is blocked by unsupported features")
            }
            Self::Benchmark(error) => write!(formatter, "{error}"),
            Self::BenchmarkVerification(error) => {
                write!(formatter, "benchmark verification failed: {error}")
            }
            Self::Guard(error) => write!(formatter, "{error}"),
            Self::Internal(_) => formatter.write_str("internal command failure"),
        }
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Usage(error) | Self::Internal(error) => Some(error.as_ref()),
            Self::Cache(error) => Some(error),
            Self::Workspace(error) => Some(error),
            Self::Runtime(error) => Some(error),
            Self::Admission(error) => Some(error),
            Self::Resource(error) => Some(error),
            Self::Run(error) => Some(error),
            Self::RunJournal(error) => Some(error),
            Self::RunOutcome(_) => None,
            Self::Matrix(error) => Some(error),
            Self::Verification(error) => Some(error),
            Self::VerifyOutcome(_) => None,
            Self::GithubActions(error) => Some(error),
            Self::MigrationBlocked => None,
            Self::Benchmark(error) | Self::BenchmarkVerification(error) => Some(error),
            Self::Guard(error) => Some(error),
        }
    }
}

#[derive(Debug)]
struct CliMessageError(&'static str);

impl fmt::Display for CliMessageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for CliMessageError {}

#[cfg(test)]
mod tests {
    use super::{
        Cli, CliError, GuardCommand, GuardExecArgs, GuardExecError, ResourceCacheStateArg,
        ResourceExecutionModeArg, ResourceExecutorArg, RunTerminalJournalEvent,
        WatchdogCompletionBarrier, detect_resource_executor, finalize_benchmark_terminal,
        finalize_guard_exec_result, finalize_run_terminal, new_journal_id,
        reconcile_watchdog_outcome, resource_run_outcome, resource_terminal_detail,
    };
    use clap::{CommandFactory, Parser};
    use commit_ci_preflight::admission::AdmissionError;
    use commit_ci_preflight::cache::{
        CacheError, CacheKey, CacheRootSource, ManagedCache, ResolvedCacheRoot,
    };
    use commit_ci_preflight::config::ConfigV1;
    use commit_ci_preflight::process::CancellationToken;
    use commit_ci_preflight::process::{
        CleanupStatus, ExitOutcome, ProcessResult, ProcessTermination, RunIdentity,
    };
    use commit_ci_preflight::resource::ResourceGuardError;
    use commit_ci_preflight::resource::{
        ResourceCommand, ResourceCommandRunner, ResourceProbe, ResourceProbeError, ResourceWatchdog,
    };
    use commit_ci_preflight::resource_history::{ResourceExecutorV2, ResourceTerminalDetailV2};
    use commit_ci_preflight::run_journal::{RunFailureKindV1, RunJournalError};
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn guard_cache_fixture(name: &str) -> (ManagedCache, PathBuf, PathBuf) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let temp_root = fs::canonicalize(std::env::temp_dir()).expect("canonical temp root");
        let base = temp_root.join(format!("ccp-guard-cache-pins-{name}-{stamp}"));
        let repository = base.join("repository");
        let root = base.join("cache");
        fs::create_dir_all(&repository).expect("repository fixture");
        let resolved = ResolvedCacheRoot {
            path: root,
            source: CacheRootSource::Explicit,
        };
        let cache = ManagedCache::initialize(resolved).expect("initialize cache");
        let envelope = ConfigV1::parse(
            r#"
schema_version = "1.0"
project = "guard/cache-pins"

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
id = "test"
required = true
argv = ["true"]
working_directory = "."
timeout_seconds = 60
"#,
        )
        .expect("fixture config")
        .into_plan()
        .expect("fixture plan");
        let key = CacheKey::for_plan_cache(&envelope, &envelope.plan.caches[0]).expect("key");
        let prepared = cache
            .prepare_entry(&key, &envelope.plan_digest, 1)
            .expect("prepare");
        fs::write(prepared.data_path.join("payload"), b"fixture").expect("payload");
        cache.promote_entry(&prepared).expect("promote");
        let source = cache.entry_data_path(&key);
        (cache, source, base)
    }

    fn cleanup_guard_cache_fixture(base: &Path) {
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn guard_cache_pins_revalidate_before_child_after_noncooperative_change() {
        let (cache, source, base) = guard_cache_fixture("revalidate");
        let pins = cache
            .pin_completed_sources(std::slice::from_ref(&source))
            .expect("pin");
        fs::remove_dir_all(&source).expect("remove source");
        let calls = AtomicUsize::new(0);
        let result = super::execute_with_guard_cache_pins(&pins, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok::<_, GuardExecError>(())
        });
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        drop(pins);
        cleanup_guard_cache_fixture(&base);
    }

    #[test]
    fn guard_cache_pins_hold_lock_through_success_and_release_after_return() {
        let (cache, source, base) = guard_cache_fixture("success");
        let result =
            super::with_guard_cache_pins(Some(&cache), std::slice::from_ref(&source), || {
                assert!(matches!(
                    cache.pin_completed_sources(std::slice::from_ref(&source)),
                    Err(CacheError::LockBusy(_))
                ));
                Ok::<_, GuardExecError>(())
            });
        assert!(result.is_ok());
        assert!(
            cache
                .pin_completed_sources(std::slice::from_ref(&source))
                .is_ok()
        );
        cleanup_guard_cache_fixture(&base);
    }

    #[test]
    fn guard_cache_pins_hold_lock_through_child_error_and_release_after_return() {
        let (cache, source, base) = guard_cache_fixture("error");
        let result =
            super::with_guard_cache_pins(Some(&cache), std::slice::from_ref(&source), || {
                assert!(matches!(
                    cache.pin_completed_sources(std::slice::from_ref(&source)),
                    Err(CacheError::LockBusy(_))
                ));
                Err::<(), _>(GuardExecError::InternalFailure)
            });
        assert!(matches!(result, Err(GuardExecError::InternalFailure)));
        assert!(
            cache
                .pin_completed_sources(std::slice::from_ref(&source))
                .is_ok()
        );
        cleanup_guard_cache_fixture(&base);
    }

    #[test]
    fn guard_cache_pins_deduplicate_sources_and_call_child_once() {
        let (cache, source, base) = guard_cache_fixture("duplicate");
        let calls = AtomicUsize::new(0);
        let result =
            super::with_guard_cache_pins(Some(&cache), &[source.clone(), source.clone()], || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<_, GuardExecError>(())
            });
        assert!(result.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        cleanup_guard_cache_fixture(&base);
    }

    #[test]
    fn guard_cache_pins_legacy_none_empty_calls_child_once() {
        let calls = AtomicUsize::new(0);
        let result = super::with_guard_cache_pins(None, &[], || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok::<_, GuardExecError>(())
        });
        assert!(result.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn guard_cache_pins_none_with_source_fails_closed_without_child() {
        let (_cache, source, base) = guard_cache_fixture("none-source");
        let calls = AtomicUsize::new(0);
        let result = super::with_guard_cache_pins(None, std::slice::from_ref(&source), || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok::<_, GuardExecError>(())
        });
        assert!(matches!(result, Err(GuardExecError::InvalidManagedCache)));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        cleanup_guard_cache_fixture(&base);
    }

    #[test]
    fn guard_cache_pins_some_with_empty_sources_fails_closed_without_child() {
        let (cache, _source, base) = guard_cache_fixture("some-empty");
        let calls = AtomicUsize::new(0);
        let result = super::with_guard_cache_pins(Some(&cache), &[], || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok::<_, GuardExecError>(())
        });
        assert!(matches!(result, Err(GuardExecError::InvalidManagedCache)));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        cleanup_guard_cache_fixture(&base);
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn resource_history_read_command_parses() {
        let cli = Cli::try_parse_from(["commit-ci-preflight", "resource", "history", "--json"])
            .expect("resource history parses");
        assert!(matches!(
            cli.command,
            Some(super::Command::Resource {
                action: super::ResourceAction::History { json: true }
            })
        ));
    }

    #[test]
    fn stable_cli_exit_codes_are_distinct() {
        let usage = CliError::usage(std::io::Error::other("usage"));
        let internal = CliError::internal(std::io::Error::other("internal"));
        assert_eq!(usage.exit_code(), 2);
        assert_eq!(internal.exit_code(), 70);
    }

    #[test]
    fn journal_id_is_filesystem_safe_hex_without_digest_prefix() {
        let run_id = new_journal_id(
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            1,
        )
        .expect("journal id");

        assert_eq!(run_id.len(), 64);
        assert!(
            run_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
    }

    #[test]
    fn guard_exec_requires_double_dash_and_program() {
        let cli = Cli::try_parse_from([
            "commit-ci-preflight",
            "guard",
            "exec",
            "--timeout-seconds",
            "1",
            "--",
            "echo",
            "hello",
        ])
        .expect("guard exec parses");

        match cli.command.expect("command is present") {
            super::Command::Guard {
                action: GuardCommand::Exec(args),
            } => {
                assert_eq!(
                    args.admission_timeout_seconds,
                    super::GUARD_EXEC_DEFAULT_TIMEOUT.as_secs()
                );
                assert_eq!(args.timeout_seconds, 1);
                assert_eq!(args.resource_profile, super::DEFAULT_RESOURCE_PROFILE);
                assert_eq!(
                    args.resource_workload_family,
                    super::DEFAULT_WORKLOAD_FAMILY
                );
                assert!(args.resource_executor.is_none());
                assert!(matches!(
                    args.resource_cache_state,
                    ResourceCacheStateArg::Unknown
                ));
                assert!(matches!(
                    args.resource_execution_mode,
                    ResourceExecutionModeArg::Unknown
                ));
                assert!(args.resource_target_platform.is_none());
                assert!(args.resource_cpu_limit_millis.is_none());
                assert!(args.resource_memory_limit_bytes.is_none());
                assert!(!args.no_resource_history);
                assert_eq!(
                    args.argv,
                    vec![OsString::from("echo"), OsString::from("hello")]
                );
            }
            other => panic!("unexpected command: {other:?}"),
        }

        assert!(Cli::try_parse_from(["commit-ci-preflight", "guard", "exec", "echo"]).is_err());
    }

    #[test]
    fn guard_exec_parses_managed_cache_pins() {
        let root = PathBuf::from("/owned/cache");
        let source = PathBuf::from(
            "/owned/cache/entries/sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/data",
        );
        let second_source = PathBuf::from(
            "/owned/cache/entries/sha256-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/data",
        );
        let cli = Cli::try_parse_from([
            "commit-ci-preflight",
            "guard",
            "exec",
            "--managed-cache-root",
            "/owned/cache",
            "--managed-cache-source",
            source.to_str().expect("source is utf-8"),
            "--managed-cache-source",
            second_source.to_str().expect("second source is utf-8"),
            "--",
            "fixture",
        ])
        .expect("managed cache pin parses");

        let super::Command::Guard {
            action: GuardCommand::Exec(args),
        } = cli.command.expect("command is present")
        else {
            panic!("unexpected command");
        };
        assert_eq!(args.managed_cache_root, vec![root.clone()]);
        assert_eq!(
            args.managed_cache_source,
            vec![source.clone(), second_source.clone()]
        );
        super::validate_guard_cache_args(&args).expect("managed cache arguments are valid");

        let legacy = Cli::try_parse_from(["commit-ci-preflight", "guard", "exec", "--", "fixture"])
            .expect("legacy guard exec parses");
        let super::Command::Guard {
            action: GuardCommand::Exec(legacy_args),
        } = legacy.command.expect("legacy command is present")
        else {
            panic!("unexpected legacy command");
        };
        assert!(legacy_args.managed_cache_root.is_empty());
        assert!(legacy_args.managed_cache_source.is_empty());
        super::validate_guard_cache_args(&legacy_args).expect("legacy arguments are valid");

        for argv in [
            vec!["--managed-cache-root", "/owned/cache", "--", "fixture"],
            vec![
                "--managed-cache-source",
                "/owned/cache/source",
                "--",
                "fixture",
            ],
            vec![
                "--managed-cache-root",
                "/owned/cache-a",
                "--managed-cache-root",
                "/owned/cache-b",
                "--managed-cache-source",
                "/owned/cache-a/source",
                "--",
                "fixture",
            ],
        ] {
            let mut command = vec!["commit-ci-preflight", "guard", "exec"];
            command.extend(argv);
            let parsed = Cli::try_parse_from(command).expect("raw combinations parse");
            let super::Command::Guard {
                action: GuardCommand::Exec(invalid_args),
            } = parsed.command.expect("invalid command is present")
            else {
                panic!("unexpected invalid command");
            };
            let error = super::validate_guard_cache_args(&invalid_args)
                .expect_err("invalid managed cache arguments are rejected");
            assert_eq!(error.exit_code(), 2);
            assert_eq!(
                error.to_string(),
                "guard exec managed cache requires exactly one root and at least one source"
            );
            assert!(!error.to_string().contains("/owned"));
        }
    }

    #[test]
    fn guard_exec_parses_explicit_resource_context() {
        let cli = Cli::try_parse_from([
            "commit-ci-preflight",
            "guard",
            "exec",
            "--resource-profile",
            "ready",
            "--resource-workload-family",
            "brain-linux-ci-v1",
            "--resource-executor",
            "orbstack",
            "--resource-cache-state",
            "warm",
            "--resource-execution-mode",
            "emulated",
            "--resource-target-platform",
            "linux-amd64",
            "--resource-cpu-limit-millis",
            "2000",
            "--resource-memory-limit-bytes",
            "8589934592",
            "--",
            "make",
            "ci-linux-orbstack",
        ])
        .expect("resource context parses");

        let super::Command::Guard {
            action: GuardCommand::Exec(args),
        } = cli.command.expect("command is present")
        else {
            panic!("unexpected command");
        };
        assert_eq!(args.resource_profile, "ready");
        assert_eq!(args.resource_workload_family, "brain-linux-ci-v1");
        assert!(matches!(
            args.resource_executor,
            Some(ResourceExecutorArg::Orbstack)
        ));
        assert!(matches!(
            args.resource_cache_state,
            ResourceCacheStateArg::Warm
        ));
        assert!(matches!(
            args.resource_execution_mode,
            ResourceExecutionModeArg::Emulated
        ));
        assert_eq!(
            args.resource_target_platform.as_deref(),
            Some("linux-amd64")
        );
        assert_eq!(args.resource_cpu_limit_millis, Some(2_000));
        assert_eq!(args.resource_memory_limit_bytes, Some(8_589_934_592));
    }

    #[test]
    fn guard_exec_exit_codes_are_stable() {
        assert_eq!(CliError::Guard(GuardExecError::TimedOut).exit_code(), 124);
        assert_eq!(
            CliError::Guard(GuardExecError::UserCancelled).exit_code(),
            130
        );
        assert_eq!(
            CliError::Guard(GuardExecError::Resource(
                commit_ci_preflight::resource::ResourceGuardError::Probe(
                    commit_ci_preflight::resource::ResourceProbeError::CommandFailed,
                ),
            ))
            .exit_code(),
            6
        );
        assert_eq!(
            CliError::Guard(GuardExecError::InternalFailure).exit_code(),
            70
        );
        assert_eq!(
            CliError::Guard(GuardExecError::ChildExit(0)).exit_code(),
            70
        );
        assert_eq!(CliError::Guard(GuardExecError::ChildExit(1)).exit_code(), 1);
        assert_eq!(
            CliError::Guard(GuardExecError::ChildExit(255)).exit_code(),
            255
        );
        assert_eq!(
            CliError::Guard(GuardExecError::Process(
                commit_ci_preflight::process::ProcessError::UnsupportedOutputMode,
            ))
            .exit_code(),
            70
        );
    }

    fn completed_process_result() -> ProcessResult {
        ProcessResult {
            identity: RunIdentity {
                project: "commit-ci-preflight.guard-exec".to_owned(),
                commit: None,
                config_digest: "guard-exec-v1".to_owned(),
                generation: "guard-exec-v1".to_owned(),
            },
            termination: ProcessTermination::Completed,
            cleanup: CleanupStatus::Verified,
            exit: Some(ExitOutcome {
                success: true,
                code: Some(0),
            }),
            stdout: commit_ci_preflight::process::CapturedStream::from_captured(Vec::new(), false),
            stderr: commit_ci_preflight::process::CapturedStream::from_captured(Vec::new(), false),
            elapsed_millis: 0,
        }
    }

    fn guard_exec_args() -> GuardExecArgs {
        GuardExecArgs {
            admission_timeout_seconds: 1,
            timeout_seconds: 1,
            resource_profile: super::DEFAULT_RESOURCE_PROFILE.to_owned(),
            resource_workload_family: super::DEFAULT_WORKLOAD_FAMILY.to_owned(),
            resource_executor: None,
            resource_cache_state: ResourceCacheStateArg::Unknown,
            resource_execution_mode: ResourceExecutionModeArg::Unknown,
            resource_target_platform: None,
            resource_cpu_limit_millis: None,
            resource_memory_limit_bytes: None,
            no_resource_history: false,
            managed_cache_root: Vec::new(),
            managed_cache_source: Vec::new(),
            argv: vec![OsString::from("fixture")],
        }
    }

    #[test]
    fn guard_exec_finalization_releases_once_for_success_error_and_resource_pressure() {
        let release_count = AtomicUsize::new(0);
        let cancellation = CancellationToken::default();

        let success = finalize_guard_exec_result(
            Ok(completed_process_result()),
            &cancellation,
            None,
            None,
            || {
                release_count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        );
        assert!(success.is_ok());

        let internal = finalize_guard_exec_result(
            Err(GuardExecError::InternalFailure),
            &cancellation,
            None,
            None,
            || {
                release_count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        );
        assert!(matches!(internal, Err(GuardExecError::InternalFailure)));

        let resource = finalize_guard_exec_result(
            Err(GuardExecError::ResourcePressure),
            &cancellation,
            None,
            None,
            || {
                release_count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        );
        assert!(matches!(resource, Err(GuardExecError::ResourcePressure)));
        assert_eq!(release_count.load(Ordering::SeqCst), 3);

        let release_failure = finalize_guard_exec_result(
            Err(GuardExecError::InternalFailure),
            &cancellation,
            None,
            None,
            || {
                release_count.fetch_add(1, Ordering::SeqCst);
                Err(GuardExecError::Admission(AdmissionError::Clock))
            },
        );
        assert!(matches!(
            release_failure,
            Err(GuardExecError::Admission(AdmissionError::Clock))
        ));
        assert_eq!(release_count.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn benchmark_terminal_preserves_primary_and_release_precedence() {
        let releases = AtomicUsize::new(0);
        let primary = finalize_benchmark_terminal(
            Err::<(), _>(CliError::Benchmark(
                commit_ci_preflight::benchmark::BenchmarkError::NoSamples,
            )),
            || {
                releases.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        );
        assert!(matches!(primary, Err(CliError::Benchmark(_))));

        let release = finalize_benchmark_terminal(Ok(()), || {
            releases.fetch_add(1, Ordering::SeqCst);
            Err(AdmissionError::Clock)
        });
        assert!(matches!(
            release,
            Err(CliError::Admission(AdmissionError::Clock))
        ));
        assert_eq!(releases.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn run_terminal_orders_watchdog_release_and_primary_journal() {
        use std::cell::RefCell;
        let events = RefCell::new(Vec::new());
        let result = finalize_run_terminal(
            Err::<(), _>(CliError::Resource(ResourceGuardError::PreStartDenied)),
            |primary| {
                events.borrow_mut().push("complete");
                primary
            },
            || {
                events.borrow_mut().push("release");
                Ok(())
            },
            |event| {
                events.borrow_mut().push(match event {
                    RunTerminalJournalEvent::PrimaryFailure(RunFailureKindV1::ResourcePressure) => {
                        "journal-primary"
                    }
                    _ => "unexpected-journal",
                });
                Ok(())
            },
        );
        assert!(matches!(result, Err(CliError::Resource(_))));
        assert_eq!(
            &*events.borrow(),
            &["complete", "release", "journal-primary"]
        );
    }

    #[test]
    fn run_terminal_release_failure_journals_cleanup_pending() {
        use std::cell::RefCell;
        let events = RefCell::new(Vec::new());
        let result = finalize_run_terminal(
            Ok(()),
            |primary| primary,
            || Err(AdmissionError::Clock),
            |event| {
                events.borrow_mut().push(event);
                Ok(())
            },
        );
        assert!(matches!(
            result,
            Err(CliError::Admission(AdmissionError::Clock))
        ));
        assert_eq!(
            &*events.borrow(),
            &[RunTerminalJournalEvent::ReleaseFailure]
        );
    }

    #[test]
    fn run_terminal_journal_failure_overrides_release_failure() {
        let result = finalize_run_terminal(
            Ok(()),
            |primary| primary,
            || Err(AdmissionError::Clock),
            |_| Err(CliError::RunJournal(RunJournalError::InvalidTransition)),
        );
        assert!(matches!(
            result,
            Err(CliError::RunJournal(RunJournalError::InvalidTransition))
        ));
    }

    #[test]
    fn all_heavy_family_adapters_release_once_and_fail_closed() {
        let releases = AtomicUsize::new(0);
        let benchmark = finalize_benchmark_terminal(Ok(()), || {
            releases.fetch_add(1, Ordering::SeqCst);
            Err(AdmissionError::Clock)
        });
        assert!(matches!(
            benchmark,
            Err(CliError::Admission(AdmissionError::Clock))
        ));
        let run = finalize_run_terminal(
            Ok(()),
            std::convert::identity,
            || {
                releases.fetch_add(1, Ordering::SeqCst);
                Err(AdmissionError::Clock)
            },
            |_| Ok(()),
        );
        assert!(matches!(
            run,
            Err(CliError::Admission(AdmissionError::Clock))
        ));
        let cancellation = CancellationToken::default();
        let guard = finalize_guard_exec_result(
            Ok(completed_process_result()),
            &cancellation,
            None,
            None,
            || {
                releases.fetch_add(1, Ordering::SeqCst);
                Err(GuardExecError::Admission(AdmissionError::Clock))
            },
        );
        assert!(matches!(
            guard,
            Err(GuardExecError::Admission(AdmissionError::Clock))
        ));
        assert_eq!(releases.load(Ordering::SeqCst), 3);
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum OpaqueTerminalOutcome {
        TimedOut,
        UserCancelled,
        ResourcePressure,
    }

    #[test]
    fn benchmark_and_run_preserve_opaque_terminal_outcomes() {
        for expected in [
            OpaqueTerminalOutcome::TimedOut,
            OpaqueTerminalOutcome::UserCancelled,
            OpaqueTerminalOutcome::ResourcePressure,
        ] {
            assert_eq!(
                finalize_benchmark_terminal(Ok(expected), || Ok(()))
                    .expect("benchmark terminal outcome"),
                expected
            );
            assert_eq!(
                finalize_run_terminal(Ok(expected), std::convert::identity, || Ok(()), |_| Ok(()))
                    .expect("run terminal outcome"),
                expected
            );
        }
    }

    #[test]
    fn guard_cache_pin_remains_live_through_terminal_release() {
        let (cache, source, base) = guard_cache_fixture("terminal-release");
        let cancellation = CancellationToken::default();

        let result =
            super::with_guard_cache_pins(Some(&cache), std::slice::from_ref(&source), || {
                finalize_guard_exec_result(
                    Ok(completed_process_result()),
                    &cancellation,
                    None,
                    None,
                    || {
                        assert!(matches!(
                            cache.pin_completed_sources(std::slice::from_ref(&source)),
                            Err(CacheError::LockBusy(_))
                        ));
                        Ok(())
                    },
                )
            });

        assert!(result.is_ok());
        assert!(
            cache
                .pin_completed_sources(std::slice::from_ref(&source))
                .is_ok()
        );
        cleanup_guard_cache_fixture(&base);
    }

    #[test]
    fn guard_exec_rejects_invalid_timeouts() {
        let zero_admission = GuardExecArgs {
            admission_timeout_seconds: 0,
            ..guard_exec_args()
        };
        let zero_child = GuardExecArgs {
            timeout_seconds: 0,
            ..guard_exec_args()
        };
        let over_cap = GuardExecArgs {
            admission_timeout_seconds: super::GUARD_EXEC_MAX_TIMEOUT.as_secs() + 1,
            ..guard_exec_args()
        };
        let over_cap_child = GuardExecArgs {
            timeout_seconds: super::GUARD_EXEC_MAX_TIMEOUT.as_secs() + 1,
            ..guard_exec_args()
        };

        assert!(matches!(
            super::print_guard_exec(zero_admission),
            Err(CliError::Guard(GuardExecError::InvalidAdmissionTimeout))
        ));
        assert!(matches!(
            super::print_guard_exec(zero_child),
            Err(CliError::Guard(GuardExecError::InvalidTimeout))
        ));
        assert!(matches!(
            super::print_guard_exec(over_cap),
            Err(CliError::Guard(GuardExecError::InvalidAdmissionTimeout))
        ));
        assert!(matches!(
            super::print_guard_exec(over_cap_child),
            Err(CliError::Guard(GuardExecError::InvalidTimeout))
        ));
    }

    #[test]
    fn guard_exec_rejects_invalid_resource_profile_before_admission() {
        let invalid = GuardExecArgs {
            resource_profile: "repository/path".to_owned(),
            ..guard_exec_args()
        };
        assert!(matches!(
            super::print_guard_exec(invalid),
            Err(CliError::Guard(GuardExecError::InvalidResourceProfile))
        ));
    }

    #[test]
    fn guard_exec_rejects_invalid_resource_context_before_admission() {
        for invalid in [
            GuardExecArgs {
                resource_workload_family: "repository/path".to_owned(),
                ..guard_exec_args()
            },
            GuardExecArgs {
                resource_target_platform: Some("linux/amd64".to_owned()),
                ..guard_exec_args()
            },
            GuardExecArgs {
                resource_cpu_limit_millis: Some(0),
                ..guard_exec_args()
            },
            GuardExecArgs {
                resource_memory_limit_bytes: Some(0),
                ..guard_exec_args()
            },
        ] {
            assert!(matches!(
                super::print_guard_exec(invalid),
                Err(CliError::Guard(GuardExecError::InvalidResourceContext))
            ));
        }
    }

    #[test]
    fn direct_docker_executor_detection_is_deterministic() {
        assert_eq!(
            detect_resource_executor(&[
                OsString::from("docker"),
                OsString::from("--context"),
                OsString::from("orbstack"),
                OsString::from("run"),
            ]),
            ResourceExecutorV2::Orbstack
        );
        assert_eq!(
            detect_resource_executor(&[
                OsString::from("/usr/local/bin/docker"),
                OsString::from("--context=orbstack"),
                OsString::from("run"),
            ]),
            ResourceExecutorV2::Orbstack
        );
        assert_eq!(
            detect_resource_executor(&[OsString::from("docker"), OsString::from("run")]),
            ResourceExecutorV2::Docker
        );
        assert_eq!(
            detect_resource_executor(&[OsString::from("make"), OsString::from("all")]),
            ResourceExecutorV2::Unknown
        );
    }

    #[test]
    fn resource_observation_outcome_is_deterministic() {
        let cancellation = CancellationToken::default();
        assert_eq!(
            resource_run_outcome(&Ok(completed_process_result()), &cancellation, None),
            commit_ci_preflight::resource_history::ResourceRunOutcome::Completed
        );

        let mut failed = completed_process_result();
        failed.exit = Some(ExitOutcome {
            success: false,
            code: Some(1),
        });
        assert_eq!(
            resource_run_outcome(&Ok(failed.clone()), &cancellation, None),
            commit_ci_preflight::resource_history::ResourceRunOutcome::Failed
        );
        assert_eq!(
            resource_terminal_detail(&Err(GuardExecError::ChildExit(7)), &cancellation, None),
            Some(ResourceTerminalDetailV2::ChildExit { exit_code: 7 })
        );
        assert_eq!(
            resource_terminal_detail(&Ok(failed), &cancellation, None),
            Some(ResourceTerminalDetailV2::ChildExit { exit_code: 1 })
        );

        let user_cancelled = CancellationToken::default();
        user_cancelled.cancel();
        assert_eq!(
            resource_run_outcome(&Ok(completed_process_result()), &user_cancelled, None,),
            commit_ci_preflight::resource_history::ResourceRunOutcome::Cancelled
        );
        assert_eq!(
            resource_terminal_detail(&Ok(completed_process_result()), &user_cancelled, None,),
            Some(ResourceTerminalDetailV2::UserCancelled)
        );

        let resource_cancelled = CancellationToken::default();
        resource_cancelled.cancel_resource_pressure();
        assert_eq!(
            resource_run_outcome(
                &Err(GuardExecError::ResourcePressure),
                &resource_cancelled,
                None,
            ),
            commit_ci_preflight::resource_history::ResourceRunOutcome::ResourcePressure
        );
        assert_eq!(
            resource_terminal_detail(
                &Err(GuardExecError::ResourcePressure),
                &resource_cancelled,
                None,
            ),
            Some(ResourceTerminalDetailV2::ResourcePressure)
        );
    }

    struct FailingResourceRunner;

    impl ResourceCommandRunner for FailingResourceRunner {
        fn run(&self, _command: ResourceCommand) -> Result<Vec<u8>, ResourceProbeError> {
            Err(ResourceProbeError::CommandFailed)
        }
    }

    #[test]
    fn watchdog_barrier_joins_once_after_early_run_error() {
        let watchdog = ResourceWatchdog::start_with_interval(
            ResourceProbe::new(FailingResourceRunner),
            CancellationToken::default(),
            std::time::Duration::from_millis(1),
        );
        let mut barrier = WatchdogCompletionBarrier::new(Some(watchdog));
        barrier.ensure_joined();
        let early_error = Err(CliError::Run(
            commit_ci_preflight::run::RunError::RepositoryNotDirectory,
        ));
        let _ = reconcile_watchdog_outcome(early_error, &mut barrier);
        assert!(barrier.take_join_error().is_none());
        assert!(barrier.watchdog.is_none());
    }
}
