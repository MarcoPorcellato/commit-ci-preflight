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

use commit_ci_preflight::config::ConfigV1;
use commit_ci_preflight::verify::VerificationPolicyV1;

const WORKFLOW: &str = include_str!("../.github/workflows/receipt-gate.yml");
const GATE_SCRIPT: &str = include_str!("../scripts/github-receipt-gate.sh");
const CHECKOUT_SHA: &str = "de0fac2e4500dabe0009e67214ff5f5447ce83dd";
const FIXTURE_COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

#[test]
fn workflow_uses_a_trusted_minimal_fail_closed_boundary() {
    assert!(WORKFLOW.contains("pull_request_target:"));
    assert!(WORKFLOW.contains("contents: read"));
    assert!(WORKFLOW.contains("statuses: write"));
    assert!(WORKFLOW.contains("cancel-in-progress: true"));
    assert!(WORKFLOW.contains("timeout-minutes: 6"));
    assert!(WORKFLOW.contains("github.event.pull_request.head.sha"));
    assert!(WORKFLOW.contains("github.event.pull_request.base.sha"));
    assert!(WORKFLOW.contains("ccp-evidence/"));
    assert!(WORKFLOW.contains(&format!("actions/checkout@{CHECKOUT_SHA}")));
    assert!(WORKFLOW.contains("persist-credentials: false"));
    assert!(WORKFLOW.contains("cargo build --locked --release --bin commit-ci-preflight"));
    assert!(WORKFLOW.contains("commit-ci-preflight/receipt"));

    for forbidden in [
        "pull_request:\n",
        "pull_request.head.ref",
        "pull_request.head.repo",
        "actions/cache",
        "cargo test",
        "docker run",
        "github.step_summary",
        "secrets.",
        "permissions: write-all",
    ] {
        assert!(
            !WORKFLOW.contains(forbidden),
            "forbidden workflow surface: {forbidden}"
        );
    }
}

#[test]
fn repository_policy_matches_the_normalized_local_plan() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let plan = ConfigV1::load(&root.join(".commit-ci-preflight.toml"))
        .and_then(ConfigV1::into_plan)
        .expect("repository plan");
    let policy = VerificationPolicyV1::load(&root.join(".commit-ci-policy.toml")).expect("policy");

    assert_eq!(policy.project, plan.plan.project);
    assert_eq!(policy.configuration_digest, plan.plan_digest);
    assert_eq!(policy.image_reference, plan.plan.runtime.image);

    let mut required: Vec<_> = plan
        .plan
        .checks
        .iter()
        .filter(|check| check.required)
        .map(|check| check.id.clone())
        .collect();
    required.sort();
    assert_eq!(policy.required_checks, required);

    let test = plan
        .plan
        .checks
        .iter()
        .find(|check| check.id == "test")
        .expect("test check");
    assert!(
        test.argv
            .iter()
            .any(|argument| argument == "CCP_TEST_ROOT=/workspace/.ccp-mounts/test-work")
    );
    assert!(
        plan.plan
            .caches
            .iter()
            .any(|cache| cache.id == "test-work" && cache.mount_path == ".ccp-mounts/test-work")
    );
}

#[cfg(unix)]
#[test]
fn gate_script_renders_a_deterministic_passing_summary() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let summary = std::env::temp_dir().join(format!(
        "commit-ci-preflight-gate-summary-{}.md",
        std::process::id()
    ));
    let _ = fs::remove_file(&summary);
    let output = Command::new("sh")
        .arg(root.join("scripts/github-receipt-gate.sh"))
        .arg(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .arg(root.join("tests/fixtures/receipt-v1-pass.json"))
        .arg(root.join("tests/fixtures/policy-v1.toml"))
        .arg(FIXTURE_COMMIT)
        .env("GITHUB_STEP_SUMMARY", &summary)
        .env("CCP_EVALUATED_AT_UTC", "2026-08-08T12:30:00Z")
        .output()
        .expect("gate script");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let rendered = fs::read_to_string(&summary).expect("summary");
    fs::remove_file(&summary).expect("remove test summary");
    assert!(rendered.contains("### Commit CI Preflight"));
    assert!(rendered.contains("Integrity: Pass"));
    assert!(rendered.contains("Policy: Pass"));
    assert!(rendered.contains("Decision: Pass"));
}

#[cfg(unix)]
#[test]
fn gate_script_rejects_missing_symlinked_and_oversized_receipts() {
    use std::os::unix::fs::symlink;

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let prefix = std::env::temp_dir().join(format!(
        "commit-ci-preflight-gate-negative-{}",
        std::process::id()
    ));
    let summary = prefix.with_extension("md");
    let missing = prefix.with_extension("missing");
    let linked = prefix.with_extension("link");
    let oversized = prefix.with_extension("json");
    let _ = fs::remove_file(&summary);
    let _ = fs::remove_file(&linked);
    let _ = fs::remove_file(&oversized);

    let invoke = |receipt: &std::path::Path| {
        Command::new("sh")
            .arg(root.join("scripts/github-receipt-gate.sh"))
            .arg(env!("CARGO_BIN_EXE_commit-ci-preflight"))
            .arg(receipt)
            .arg(root.join("tests/fixtures/policy-v1.toml"))
            .arg(FIXTURE_COMMIT)
            .env("GITHUB_STEP_SUMMARY", &summary)
            .env("CCP_EVALUATED_AT_UTC", "2026-08-08T12:30:00Z")
            .output()
            .expect("negative gate script")
    };

    let missing_output = invoke(&missing);
    assert_eq!(missing_output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&missing_output.stdout).contains("receipt is missing"));

    symlink(root.join("tests/fixtures/receipt-v1-pass.json"), &linked).expect("receipt symlink");
    let linked_output = invoke(&linked);
    assert_eq!(linked_output.status.code(), Some(3));

    fs::write(&oversized, vec![b'x'; 1_048_577]).expect("oversized receipt");
    let oversized_output = invoke(&oversized);
    assert_eq!(oversized_output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&oversized_output.stdout).contains("one MiB"));

    fs::remove_file(&linked).expect("remove receipt symlink");
    fs::remove_file(&oversized).expect("remove oversized receipt");
    let _ = fs::remove_file(&summary);
}

#[test]
fn gate_script_has_no_network_or_project_execution_surface() {
    assert!(GATE_SCRIPT.starts_with("#!/bin/sh\n"));
    assert!(GATE_SCRIPT.contains("receipt_size\" -gt 1048576"));
    assert!(GATE_SCRIPT.contains("\"$verifier\" verify"));
    for forbidden in [
        "curl ", "gh api", "git ", "cargo ", "docker ", "eval ", "source ",
    ] {
        assert!(
            !GATE_SCRIPT.contains(forbidden),
            "forbidden gate command: {forbidden}"
        );
    }
}
