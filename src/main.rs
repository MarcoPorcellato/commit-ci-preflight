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
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clap::{Args, CommandFactory, Parser, Subcommand};
use commit_ci_preflight::admission::{
    ADMISSION_SCHEMA_VERSION, AdmissionCoordinator, AdmissionError, AdmissionGuard,
    DEFAULT_QUEUE_TIMEOUT,
};
use commit_ci_preflight::benchmark::{
    BenchmarkError, run_benchmark, verify_benchmark_document, write_new_receipt,
};
use commit_ci_preflight::cache::{CacheError, CacheRootOptions, ManagedCache, ResolvedCacheRoot};
use commit_ci_preflight::config::{ConfigV1, ExecutionPlanEnvelopeV1};
use commit_ci_preflight::github_actions::{
    GithubActionsError, MigrationReadiness, analyze_workflow_file,
};
use commit_ci_preflight::process::{
    CancellationToken, GenerationGuard, OutputMode, ProcessRequest, ProcessResult,
    ProcessSupervisor, ProcessTermination, RunIdentity, SupervisorPort,
};
use commit_ci_preflight::receipt::{CheckEvidence, EvidenceStatus};
use commit_ci_preflight::resource::{
    ResourceGuardError, ResourcePlatform, ResourceProbe, ResourceProbeError, ResourceWatchdog,
    SupervisorResourceRunner, WatchdogTripReason, evaluate_pre_start, status_from_snapshot,
    unknown_status, unsupported_status,
};
use commit_ci_preflight::run::{
    CompletionBarrier, RunError, RunRequest, SystemClock, execute_local_run_with_barrier,
};
use commit_ci_preflight::runtime::{
    DryRunPlan, RuntimeError, RuntimeProbe, doctor_guard, runtime_for,
};
use commit_ci_preflight::verify::{
    VerificationDecision, VerificationError, VerificationPolicyV1, receipt_input_failure_report,
    system_evaluated_at_utc, verify_receipt_document,
};
use commit_ci_preflight::workspace::{WorkspaceError, WorkspacePlanV1};

const GUARD_EXEC_DEFAULT_TIMEOUT: Duration = Duration::from_secs(6 * 60 * 60);
const GUARD_EXEC_MAX_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);
const GUARD_EXEC_CAPTURE_BYTES: usize = 1_048_576;

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
}

#[derive(Debug, Subcommand)]
enum ResourceAction {
    /// Report the bounded resource-guard capability and current decision.
    Status {
        /// Emit the versioned machine-readable status.
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
    /// Program and arguments. The `--` separator is required.
    #[arg(last = true, required = true, allow_hyphen_values = true)]
    argv: Vec<OsString>,
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
enum AdmissionCommand {
    /// Report only bounded coordinator state and ticket identifiers.
    Status {
        /// Emit the versioned machine-readable status.
        #[arg(long)]
        json: bool,
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
        .and_then(|()| run_benchmark(commit, runtime_probe.as_ref()).map_err(CliError::Benchmark));
    let envelope = release_admission(guard, result)?;
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
    let current_dir = fs::canonicalize(".")
        .map_err(|_| CliError::Guard(GuardExecError::InvalidCurrentDirectory))?;
    if !current_dir.is_dir() {
        return Err(CliError::Guard(GuardExecError::InvalidCurrentDirectory));
    }

    let cancellation = CancellationToken::default();
    install_cancellation_handler(&cancellation)?;
    let supervisor = Arc::new(ProcessSupervisor::standard());
    let mut session = GuardExecSession::acquire(&cancellation, admission_timeout)?;

    if let Err(error) = resource_pre_start(supervisor.clone(), &cancellation) {
        let result = session.finish(Err(GuardExecError::from_cli(error)), &cancellation);
        return result.map(|_| ()).map_err(CliError::Guard);
    }

    session.start_watchdog(
        Arc::clone(&supervisor),
        current_dir.clone(),
        cancellation.clone(),
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
    let process = supervisor
        .execute_with_output(
            &request,
            &cancellation,
            &GenerationGuard::new(request.identity.clone()),
            OutputMode::Tee,
        )
        .map_err(GuardExecError::Process);
    let result = session
        .finish(process, &cancellation)
        .map_err(CliError::Guard)?;
    classify_guard_result(result, &cancellation).map_err(CliError::Guard)
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
    let policy = VerificationPolicyV1::load(policy_path).map_err(CliError::usage)?;
    let evaluated_at = evaluated_at_utc
        .map(str::to_owned)
        .map(Ok)
        .unwrap_or_else(system_evaluated_at_utc)
        .map_err(CliError::Verification)?;
    let report = match fs::read(receipt_path) {
        Ok(receipt) => verify_receipt_document(&receipt, &policy, expected_commit, &evaluated_at),
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
    let envelope = load_plan(path)?;
    let root = resolve_cache_root(location)?;
    let cache = ManagedCache::initialize(root).map_err(CliError::Cache)?;
    let runtime = runtime_for(envelope.plan.runtime.kind).map_err(CliError::Runtime)?;
    let supervisor = Arc::new(ProcessSupervisor::standard());
    let cancellation = CancellationToken::default();
    install_cancellation_handler(&cancellation)?;
    let admission =
        AdmissionCoordinator::platform_for(&location.repository).map_err(CliError::Admission)?;
    let guard = admission
        .acquire(
            Duration::from_secs(admission_timeout_seconds),
            &cancellation,
        )
        .map_err(CliError::Admission)?;
    if let Err(error) = resource_pre_start(supervisor.clone(), &cancellation) {
        return match guard.release() {
            Ok(()) => Err(error),
            Err(release_error) => Err(CliError::Admission(release_error)),
        };
    }
    let watchdog = if ResourcePlatform::current() == ResourcePlatform::MacOs {
        let current_dir = match std::env::current_dir() {
            Ok(path) => path,
            Err(error) => {
                return match guard.release() {
                    Ok(()) => Err(CliError::internal(error)),
                    Err(release_error) => Err(CliError::Admission(release_error)),
                };
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
    let outcome = execute_local_run_with_barrier(
        &RunRequest {
            envelope: &envelope,
            repository: &location.repository,
            cache: &cache,
            generation,
        },
        runtime.as_ref(),
        supervisor.as_ref(),
        &cancellation,
        &SystemClock,
        &mut completion_barrier,
    )
    .map_err(CliError::Run);
    completion_barrier.ensure_joined();
    let outcome = reconcile_watchdog_outcome(outcome, &mut completion_barrier);
    let outcome = release_admission(guard, outcome)?;
    if json {
        let bytes = outcome
            .receipt
            .canonical_bytes()
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

fn load_plan(path: &Path) -> Result<ExecutionPlanEnvelopeV1, CliError> {
    ConfigV1::load(path)
        .map_err(CliError::usage)?
        .into_plan()
        .map_err(CliError::usage)
}

fn print_plan(path: &Path, json: bool) -> Result<(), CliError> {
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

fn run_admission_command(action: AdmissionCommand) -> Result<(), CliError> {
    match action {
        AdmissionCommand::Status { json } => {
            let status = AdmissionCoordinator::platform()
                .map_err(CliError::Admission)?
                .status()
                .map_err(CliError::Admission)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string(&status).map_err(CliError::internal)?
                );
            } else {
                println!("Admission schema: {ADMISSION_SCHEMA_VERSION}");
                println!("Active: {}", status.active);
                println!("Queued: {}", status.queue_count);
                for ticket in status.ticket_ids {
                    println!("  - {ticket}");
                }
            }
            Ok(())
        }
    }
}

fn run_resource_command(action: ResourceAction) -> Result<(), CliError> {
    match action {
        ResourceAction::Status { json } => print_resource_status(json),
    }
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
) -> Result<(), CliError> {
    if ResourcePlatform::current() != ResourcePlatform::MacOs {
        return Ok(());
    }
    let current_dir = std::env::current_dir().map_err(CliError::internal)?;
    let runner = SupervisorResourceRunner::new(supervisor, current_dir, cancellation.clone());
    let snapshot = ResourceProbe::new(runner)
        .sample()
        .map_err(|error| CliError::Resource(ResourceGuardError::Probe(error)))?;
    match evaluate_pre_start(&snapshot)
        .map_err(|error| CliError::Resource(ResourceGuardError::Probe(error)))?
    {
        commit_ci_preflight::resource::PreStartDecision::Admit => Ok(()),
        commit_ci_preflight::resource::PreStartDecision::Deny => {
            Err(CliError::Resource(ResourceGuardError::PreStartDenied))
        }
    }
}

fn release_admission<T>(guard: AdmissionGuard, result: Result<T, CliError>) -> Result<T, CliError> {
    let release = guard.release().map_err(CliError::Admission);
    match release {
        Ok(()) => result,
        Err(error) => Err(error),
    }
}

struct GuardExecSession {
    admission: Option<AdmissionGuard>,
    watchdog: WatchdogCompletionBarrier,
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
        })
    }

    fn start_watchdog(
        &mut self,
        supervisor: Arc<ProcessSupervisor>,
        current_dir: PathBuf,
        cancellation: CancellationToken,
    ) {
        if ResourcePlatform::current() == ResourcePlatform::MacOs {
            self.watchdog = WatchdogCompletionBarrier::new(Some(ResourceWatchdog::start(
                ResourceProbe::new(SupervisorResourceRunner::new(
                    supervisor,
                    current_dir,
                    cancellation.clone(),
                )),
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
        let guard = self
            .admission
            .take()
            .ok_or(GuardExecError::InternalFailure)?;
        finalize_guard_exec_result(
            result,
            cancellation,
            self.watchdog.take_join_error(),
            self.watchdog.trip(),
            || guard.release().map_err(GuardExecError::Admission),
        )
    }
}

fn finalize_guard_exec_result(
    result: Result<ProcessResult, GuardExecError>,
    cancellation: &CancellationToken,
    join_error: Option<ResourceProbeError>,
    trip: Option<WatchdogTripReason>,
    release: impl FnOnce() -> Result<(), GuardExecError>,
) -> Result<ProcessResult, GuardExecError> {
    let result = if let Some(error) = join_error {
        Err(GuardExecError::Resource(ResourceGuardError::Watchdog(
            error,
        )))
    } else if let Some(reason) = trip {
        Err(GuardExecError::Resource(
            ResourceGuardError::WatchdogTripped(reason),
        ))
    } else if cancellation.reason()
        == Some(commit_ci_preflight::process::CancellationReason::ResourcePressure)
    {
        Err(GuardExecError::ResourcePressure)
    } else {
        result
    };
    match release() {
        Ok(()) => result,
        Err(error) => Err(error),
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
    InvalidCurrentDirectory,
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
            | Self::InvalidCurrentDirectory => 2,
            Self::Admission(error) => error.exit_code(),
            Self::Resource(_) | Self::ResourcePressure => 6,
            Self::Process(_) | Self::InternalFailure => 70,
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
            Self::InvalidCurrentDirectory => {
                formatter.write_str("guard exec current directory could not be canonicalized")
            }
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
    RunOutcome(EvidenceStatus),
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
            Self::RunOutcome(EvidenceStatus::Fail) => 1,
            Self::RunOutcome(EvidenceStatus::Pending | EvidenceStatus::NotRun) => 5,
            Self::RunOutcome(EvidenceStatus::Pass) => 0,
            Self::VerifyOutcome(_) => 3,
            Self::GithubActions(_) => 2,
            Self::MigrationBlocked => 4,
            Self::Benchmark(_) => 2,
            Self::BenchmarkVerification(_) => 3,
            Self::Guard(error) => error.exit_code(),
            Self::Verification(VerificationError::Policy(_))
            | Self::Verification(VerificationError::InvalidExpectedCommit)
            | Self::Verification(VerificationError::InvalidEvaluationTime) => 2,
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
            Self::RunOutcome(status) => write!(formatter, "run completed with {status:?}"),
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
            Self::RunOutcome(_) => None,
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
        Cli, CliError, GuardCommand, GuardExecArgs, GuardExecError, WatchdogCompletionBarrier,
        finalize_guard_exec_result, reconcile_watchdog_outcome,
    };
    use clap::{CommandFactory, Parser};
    use commit_ci_preflight::process::CancellationToken;
    use commit_ci_preflight::process::{
        CleanupStatus, ExitOutcome, ProcessResult, ProcessTermination, RunIdentity,
    };
    use commit_ci_preflight::resource::{
        ResourceCommand, ResourceCommandRunner, ResourceProbe, ResourceProbeError, ResourceWatchdog,
    };
    use std::ffi::OsString;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn stable_cli_exit_codes_are_distinct() {
        let usage = CliError::usage(std::io::Error::other("usage"));
        let internal = CliError::internal(std::io::Error::other("internal"));
        assert_eq!(usage.exit_code(), 2);
        assert_eq!(internal.exit_code(), 70);
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
            stdout: commit_ci_preflight::process::CapturedStream {
                bytes: Vec::new(),
                truncated: false,
            },
            stderr: commit_ci_preflight::process::CapturedStream {
                bytes: Vec::new(),
                truncated: false,
            },
            elapsed_millis: 0,
        }
    }

    fn guard_exec_args() -> GuardExecArgs {
        GuardExecArgs {
            admission_timeout_seconds: 1,
            timeout_seconds: 1,
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
