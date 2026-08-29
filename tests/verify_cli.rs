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

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

const RECEIPT: &str = "tests/fixtures/receipt-v1-pass.json";
const POLICY: &str = "tests/fixtures/policy-v1.toml";
const RECEIPT_V2: &str = "tests/fixtures/receipt-v2-pass.json";
const TRUSTED_PLAN_POLICY: &str = "tests/fixtures/policy-v1_1-trusted-plan.toml";
const INVALID_TRUSTED_PLAN_POLICY: &str = "tests/fixtures/policy-v1_1-missing-config.toml";
const LEGACY_MATRIX_POLICY: &str = "tests/fixtures/policy-v2-legacy-compatible.toml";
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const EVALUATED_AT: &str = "2026-08-08T12:30:00Z";

fn build_verifier_candidate() -> PathBuf {
    let output = Command::new(env!("CARGO"))
        .args([
            "build",
            "--locked",
            "-p",
            "ccp-verifier",
            "--bin",
            "ccp-verifier",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("build independent verifier candidate");
    assert!(
        output.status.success(),
        "ccp-verifier candidate build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let path =
        PathBuf::from(env!("CARGO_BIN_EXE_commit-ci-preflight")).with_file_name("ccp-verifier");
    assert!(path.is_file(), "Cargo did not produce {}", path.display());
    path
}

fn verifier_parity(verifier_path: &Path, args: &[&str]) {
    let root = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args(args)
        .output()
        .expect("root verify");
    let verifier = Command::new(verifier_path)
        .args(args)
        .output()
        .expect("independent verifier");
    assert_eq!(root.status.code(), verifier.status.code());
    assert_eq!(root.stdout, verifier.stdout);
    if root.stderr != verifier.stderr {
        let root_text = String::from_utf8_lossy(&root.stderr);
        let verifier_text = String::from_utf8_lossy(&verifier.stderr);
        assert!(root_text.contains("Usage:") && verifier_text.contains("Usage:"));
        let root_stderr = root_text.replace("commit-ci-preflight", "VERIFIER");
        let verifier_stderr = verifier_text.replace("ccp-verifier", "VERIFIER");
        assert_eq!(root_stderr, verifier_stderr);
    }
}

#[test]
fn independent_verifier_matches_root_for_representative_outcomes() {
    let verifier = build_verifier_candidate();
    verifier_parity(
        &verifier,
        &[
            "verify",
            "--receipt",
            RECEIPT,
            "--policy",
            POLICY,
            "--expected-commit",
            COMMIT,
            "--evaluated-at-utc",
            EVALUATED_AT,
            "--json",
        ],
    );
    verifier_parity(
        &verifier,
        &[
            "verify",
            "--receipt",
            RECEIPT,
            "--policy",
            POLICY,
            "--expected-commit",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "--evaluated-at-utc",
            EVALUATED_AT,
            "--json",
        ],
    );
    verifier_parity(
        &verifier,
        &[
            "verify",
            "--receipt",
            "tests/fixtures/does-not-exist.json",
            "--policy",
            POLICY,
            "--expected-commit",
            COMMIT,
            "--evaluated-at-utc",
            EVALUATED_AT,
            "--json",
        ],
    );
    verifier_parity(
        &verifier,
        &[
            "verify",
            "--receipt",
            RECEIPT,
            "--policy",
            POLICY,
            "--expected-commit",
            "HEAD",
            "--evaluated-at-utc",
            EVALUATED_AT,
        ],
    );
    verifier_parity(
        &verifier,
        &[
            "verify",
            "--receipt",
            RECEIPT_V2,
            "--policy",
            TRUSTED_PLAN_POLICY,
            "--expected-commit",
            COMMIT,
            "--evaluated-at-utc",
            EVALUATED_AT,
            "--json",
        ],
    );
    verifier_parity(
        &verifier,
        &[
            "verify",
            "--receipt",
            "tests/fixtures/does-not-exist.json",
            "--policy",
            LEGACY_MATRIX_POLICY,
            "--expected-commit",
            COMMIT,
            "--evaluated-at-utc",
            EVALUATED_AT,
            "--json",
        ],
    );
    verifier_parity(
        &verifier,
        &[
            "verify",
            "--receipt",
            "tests/fixtures/does-not-exist.json",
            "--policy",
            INVALID_TRUSTED_PLAN_POLICY,
            "--expected-commit",
            COMMIT,
            "--evaluated-at-utc",
            EVALUATED_AT,
            "--json",
        ],
    );

    let malformed = std::env::temp_dir().join(format!(
        "ccp-verifier-malformed-receipt-{}.json",
        std::process::id()
    ));
    std::fs::write(&malformed, b"{").expect("write malformed receipt fixture");
    let malformed_path = malformed.to_str().expect("temporary path UTF-8");
    verifier_parity(
        &verifier,
        &[
            "verify",
            "--receipt",
            malformed_path,
            "--policy",
            POLICY,
            "--expected-commit",
            COMMIT,
            "--evaluated-at-utc",
            EVALUATED_AT,
            "--json",
        ],
    );
    std::fs::remove_file(&malformed).expect("remove malformed receipt fixture");
}

fn verify_command(expected_commit: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"));
    command.args([
        "verify",
        "--receipt",
        RECEIPT,
        "--policy",
        POLICY,
        "--expected-commit",
        expected_commit,
        "--evaluated-at-utc",
        EVALUATED_AT,
        "--json",
    ]);
    command
}

#[test]
fn verify_cli_emits_machine_report_and_zero_only_for_pass() {
    let output = verify_command(COMMIT).output().expect("verify CLI");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("report JSON");
    assert_eq!(report["integrity_status"], "PASS");
    assert_eq!(report["policy_status"], "PASS");
    assert_eq!(report["decision"], "PASS");
}

#[test]
fn verify_cli_resolves_trusted_plan_policy_relative_to_the_policy_file() {
    let output = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args([
            "verify",
            "--receipt",
            RECEIPT_V2,
            "--policy",
            TRUSTED_PLAN_POLICY,
            "--expected-commit",
            COMMIT,
            "--evaluated-at-utc",
            EVALUATED_AT,
            "--json",
        ])
        .output()
        .expect("trusted-plan verify CLI");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("report JSON");
    assert_eq!(
        report["assurance_scope"],
        "integrity_and_trusted_plan_policy"
    );
    assert_eq!(report["decision"], "PASS");
}

#[test]
fn valid_but_wrong_external_commit_exits_three_with_policy_report() {
    let output = verify_command(&"b".repeat(40))
        .output()
        .expect("policy failure CLI");
    assert_eq!(output.status.code(), Some(3));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("report JSON");
    assert_eq!(report["integrity_status"], "PASS");
    assert_eq!(report["policy_status"], "FAIL");
    assert_eq!(report["decision"], "FAIL");
    assert!(String::from_utf8_lossy(&output.stderr).contains("verification completed with Fail"));
}

#[test]
fn missing_receipt_is_verification_failure_and_invalid_commit_is_usage() {
    let missing = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args([
            "verify",
            "--receipt",
            "tests/fixtures/does-not-exist.json",
            "--policy",
            POLICY,
            "--expected-commit",
            COMMIT,
            "--json",
        ])
        .output()
        .expect("missing receipt CLI");
    assert_eq!(missing.status.code(), Some(3));
    let missing_report: serde_json::Value =
        serde_json::from_slice(&missing.stdout).expect("missing receipt report");
    assert_eq!(missing_report["integrity_status"], "FAIL");
    assert_eq!(missing_report["policy_status"], "NOT_RUN");
    assert_eq!(missing_report["findings"][0]["code"], "receipt.read_failed");

    let invalid = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args([
            "verify",
            "--receipt",
            RECEIPT,
            "--policy",
            POLICY,
            "--expected-commit",
            "HEAD",
            "--evaluated-at-utc",
            EVALUATED_AT,
        ])
        .output()
        .expect("invalid commit CLI");
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
}

#[test]
fn missing_receipt_does_not_bypass_trusted_policy_configuration_validation() {
    let output = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args([
            "verify",
            "--receipt",
            "tests/fixtures/does-not-exist.json",
            "--policy",
            INVALID_TRUSTED_PLAN_POLICY,
            "--expected-commit",
            COMMIT,
            "--json",
        ])
        .output()
        .expect("invalid trusted policy CLI");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read trusted configuration"));
}

#[test]
fn verify_cli_accepts_the_legacy_matrix_policy_before_receipt_evaluation() {
    let output = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args([
            "verify",
            "--receipt",
            "tests/fixtures/does-not-exist.json",
            "--policy",
            LEGACY_MATRIX_POLICY,
            "--expected-commit",
            COMMIT,
            "--evaluated-at-utc",
            EVALUATED_AT,
            "--json",
        ])
        .output()
        .expect("legacy Matrix policy CLI");
    assert_eq!(output.status.code(), Some(3));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("report JSON");
    assert_eq!(report["integrity_status"], "FAIL");
    assert_eq!(report["policy_status"], "NOT_RUN");
    assert_eq!(report["findings"][0]["code"], "receipt.read_failed");
}
