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

use std::process::ExitCode;

use commit_ci_preflight::economic_evaluation::{
    EvaluationReport, EvaluationWorksheet, evaluate_worksheet,
};

fn main() -> ExitCode {
    match run() {
        Ok(report) => match serde_json::to_string_pretty(&report) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(_) => fail("could not serialize evaluation report"),
        },
        Err(error) => fail(&error),
    }
}

fn run() -> Result<EvaluationReport, String> {
    let mut arguments = std::env::args_os().skip(1);
    let input_path = arguments
        .next()
        .ok_or_else(|| "usage: evaluate_economic_case_study <worksheet.json>".to_owned())?;
    if arguments.next().is_some() {
        return Err("usage: evaluate_economic_case_study <worksheet.json>".into());
    }

    let input =
        std::fs::read_to_string(input_path).map_err(|_| "could not read worksheet".to_owned())?;
    let worksheet: EvaluationWorksheet =
        serde_json::from_str(&input).map_err(|_| "worksheet is not valid JSON".to_owned())?;
    evaluate_worksheet(&worksheet).map_err(|error| error.to_string())
}

fn fail(error: &str) -> ExitCode {
    eprintln!("economic evaluation failed: {error}");
    ExitCode::FAILURE
}
