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

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use commit_ci_preflight::github_actions::{
    FeatureDisposition, MigrationReadiness, analyze_workflow, compatibility_report_schema_json,
};

const SUPPORTED: &str = include_str!("fixtures/github-actions/supported.yml");
const MIXED: &str = include_str!("fixtures/github-actions/mixed.yml");
const REUSABLE: &str = include_str!("fixtures/github-actions/reusable.yml");

#[test]
fn supported_fixture_is_deterministic_and_never_claims_executable_output() {
    let first = analyze_workflow(SUPPORTED).expect("supported fixture report");
    let second = analyze_workflow(SUPPORTED).expect("deterministic rerun");

    assert_eq!(first, second);
    assert_eq!(first.schema_version, "1.0");
    assert_eq!(first.readiness, MigrationReadiness::ManualReviewRequired);
    assert!(!first.executable_config_emitted);
    assert_eq!(first.proposed_checks.len(), 2);
    assert_eq!(first.proposed_checks[0].shell, "bash");
    assert_eq!(first.proposed_checks[1].shell, "sh");
    assert_eq!(
        first.environment_names,
        ["CARGO_TERM_COLOR", "RUST_BACKTRACE"]
    );
    assert!(first.findings.iter().any(|finding| {
        finding.feature == "checkout" && finding.disposition == FeatureDisposition::Translated
    }));
    assert!(first.findings.iter().all(|finding| {
        finding.feature != "marketplace_or_local_action"
            && finding.disposition != FeatureDisposition::Unsupported
    }));
}

#[test]
fn unsupported_fixture_reports_every_high_risk_surface_without_proposals() {
    let report = analyze_workflow(MIXED).expect("mixed fixture report");

    assert_eq!(report.readiness, MigrationReadiness::Blocked);
    assert!(report.proposed_checks.is_empty());
    for feature in [
        "unknown_key",
        "permissions",
        "runner_expression",
        "strategy",
        "services",
        "setup_metadata",
        "action_inputs",
        "action_input_expression",
        "marketplace_or_local_action",
        "expression",
    ] {
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.feature == feature),
            "missing compatibility finding for {feature}"
        );
    }
}

#[test]
fn reusable_workflow_is_blocked_and_never_loaded() {
    let report = analyze_workflow(REUSABLE).expect("reusable fixture report");
    assert_eq!(report.readiness, MigrationReadiness::Blocked);
    assert!(report.proposed_checks.is_empty());
    assert!(report.findings.iter().any(|finding| {
        finding.feature == "reusable_workflow"
            && finding.disposition == FeatureDisposition::Unsupported
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.feature == "secrets" && finding.disposition == FeatureDisposition::Unsupported
    }));
}

#[test]
fn report_schema_is_versioned_and_machine_readable() {
    let schema = compatibility_report_schema_json().expect("compatibility schema");
    let parsed: serde_json::Value = serde_json::from_str(&schema).expect("schema JSON");
    assert_eq!(parsed["title"], "GithubActionsCompatibilityReportV1");
    assert!(parsed["properties"]["findings"].is_object());
    assert!(parsed["properties"]["executable_config_emitted"].is_object());
}

#[test]
fn duplicate_keys_and_oversized_input_fail_before_compatibility_analysis() {
    let duplicate = "name: one\nname: two\non: push\njobs: {}\n";
    assert!(analyze_workflow(duplicate).is_err());
    let oversized = "x".repeat(commit_ci_preflight::github_actions::MAX_WORKFLOW_BYTES + 1);
    assert!(analyze_workflow(&oversized).is_err());
}

#[test]
fn cli_is_read_only_and_uses_distinct_blocked_exit_code() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let marker = root.join("must-not-be-created-by-migration-analysis");
    let _ = fs::remove_file(&marker);
    let supported = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args([
            "migrate-github-actions",
            "--workflow",
            "tests/fixtures/github-actions/supported.yml",
            "--json",
        ])
        .output()
        .expect("run supported migration analysis");
    assert!(supported.status.success());
    assert!(supported.stderr.is_empty());
    let report: serde_json::Value =
        serde_json::from_slice(&supported.stdout).expect("CLI report JSON");
    assert_eq!(report["executable_config_emitted"], false);
    assert!(!marker.exists());

    let mixed = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args([
            "migrate-github-actions",
            "--workflow",
            "tests/fixtures/github-actions/mixed.yml",
            "--json",
        ])
        .output()
        .expect("run blocked migration analysis");
    assert_eq!(mixed.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&mixed.stderr).contains("blocked"));
    assert!(!marker.exists());
}
