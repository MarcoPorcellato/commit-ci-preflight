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
use std::fs;
use std::path::{Path, PathBuf};

use clap::{Args, CommandFactory, Parser, Subcommand};
use commit_ci_preflight::cache::{CacheError, CacheRootOptions, ManagedCache, ResolvedCacheRoot};
use commit_ci_preflight::config::{ConfigV1, ExecutionPlanEnvelopeV1};
use commit_ci_preflight::process::{CancellationToken, ProcessSupervisor};
use commit_ci_preflight::receipt::EvidenceStatus;
use commit_ci_preflight::run::{RunError, RunRequest, SystemClock, execute_local_run};
use commit_ci_preflight::runtime::{
    DryRunPlan, RuntimeError, RuntimeProbe, doctor_guard, runtime_for,
};
use commit_ci_preflight::verify::{
    VerificationDecision, VerificationError, VerificationPolicyV1, receipt_input_failure_report,
    system_evaluated_at_utc, verify_receipt_document,
};
use commit_ci_preflight::workspace::{WorkspaceError, WorkspacePlanV1};

#[derive(Debug, Parser)]
#[command(name = "commit-ci-preflight", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
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
    /// Inspect or initialize the persistent managed cache.
    Cache {
        #[command(subcommand)]
        action: CacheCommand,
    },
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

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
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
        }) => print_run(&config, &location, generation, json),
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
    json: bool,
) -> Result<(), CliError> {
    let envelope = load_plan(path)?;
    let root = resolve_cache_root(location)?;
    let cache = ManagedCache::initialize(root).map_err(CliError::Cache)?;
    let runtime = runtime_for(envelope.plan.runtime.kind).map_err(CliError::Runtime)?;
    let supervisor = ProcessSupervisor::standard();
    let cancellation = CancellationToken::default();
    install_cancellation_handler(&cancellation)?;
    let outcome = execute_local_run(
        &RunRequest {
            envelope: &envelope,
            repository: &location.repository,
            cache: &cache,
            generation,
        },
        runtime.as_ref(),
        &supervisor,
        &cancellation,
        &SystemClock,
    )
    .map_err(CliError::Run)?;
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
enum CliError {
    Usage(Box<dyn std::error::Error>),
    Cache(CacheError),
    Workspace(WorkspaceError),
    Runtime(RuntimeError),
    Run(RunError),
    RunOutcome(EvidenceStatus),
    Verification(VerificationError),
    VerifyOutcome(VerificationDecision),
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
            Self::Run(error) => error.exit_code(),
            Self::RunOutcome(EvidenceStatus::Fail) => 1,
            Self::RunOutcome(EvidenceStatus::Pending | EvidenceStatus::NotRun) => 5,
            Self::RunOutcome(EvidenceStatus::Pass) => 0,
            Self::VerifyOutcome(_) => 3,
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
            Self::Run(error) => write!(formatter, "{error}"),
            Self::RunOutcome(status) => write!(formatter, "run completed with {status:?}"),
            Self::Verification(error) => write!(formatter, "{error}"),
            Self::VerifyOutcome(decision) => {
                write!(formatter, "verification completed with {decision:?}")
            }
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
            Self::Run(error) => Some(error),
            Self::RunOutcome(_) => None,
            Self::Verification(error) => Some(error),
            Self::VerifyOutcome(_) => None,
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
    use super::{Cli, CliError};
    use clap::CommandFactory;

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
}
