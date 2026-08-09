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
use std::path::{Path, PathBuf};

use clap::{CommandFactory, Parser, Subcommand};
use commit_ci_preflight::config::{ConfigV1, ExecutionPlanEnvelopeV1};
use commit_ci_preflight::process::{CancellationToken, ProcessSupervisor};
use commit_ci_preflight::runtime::{
    DryRunPlan, RuntimeError, RuntimeProbe, doctor_guard, runtime_for,
};

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
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Command::Plan { config, json }) => print_plan(&config, json),
        Some(Command::Doctor { config, json }) => print_doctor(&config, json),
        Some(Command::DryRun { config, json }) => print_dry_run(&config, json),
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

fn print_dry_run(path: &Path, json: bool) -> Result<(), CliError> {
    let envelope = load_plan(path)?;
    let runtime = runtime_for(envelope.plan.runtime.kind).map_err(CliError::Runtime)?;
    let dry_run = runtime.dry_run(&envelope).map_err(CliError::Runtime)?;
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
    println!("Workspace mounts: deferred to PR 04");
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
    Runtime(RuntimeError),
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
            Self::Runtime(error) => error.exit_code(),
            Self::Internal(_) => 70,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(error) => write!(formatter, "{error}"),
            Self::Runtime(error) => write!(formatter, "{error}"),
            Self::Internal(_) => formatter.write_str("internal command failure"),
        }
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Usage(error) | Self::Internal(error) => Some(error.as_ref()),
            Self::Runtime(error) => Some(error),
        }
    }
}

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
