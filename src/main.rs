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

use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};
use commit_ci_preflight::config::ConfigV1;

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
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Command::Plan { config, json }) => print_plan(&config, json),
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
        std::process::exit(2);
    }
}

fn print_plan(path: &std::path::Path, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let envelope = ConfigV1::load(path)?.into_plan()?;
    if json {
        let bytes = envelope.canonical_bytes()?;
        println!("{}", String::from_utf8(bytes)?);
    } else {
        println!("Plan: {}", envelope.plan_digest);
        println!("Project: {}", envelope.plan.project);
        println!("Runtime: {:?}", envelope.plan.runtime.kind);
        println!("Checks: {}", envelope.plan.checks.len());
        for check in envelope.plan.checks {
            println!("  - {}", check.id);
        }
        println!("Read-only: no command was executed.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
