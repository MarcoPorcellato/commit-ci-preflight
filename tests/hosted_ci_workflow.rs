// Copyright 2026 Marco Porcellato
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

use std::{fs, path::PathBuf};

use saphyr::{LoadableYamlNode, MappingOwned, YamlOwned};

const WORKFLOW: &str = include_str!("../.github/workflows/rust-ci.yml");
const README: &str = include_str!("../README.md");
const GITHUB_GATE: &str = include_str!("../docs/GITHUB_GATE.md");
const CHANGELOG: &str = include_str!("../CHANGELOG.md");
const RUST_TOOLCHAIN: &str = include_str!("../rust-toolchain.toml");
const CHECKOUT_SHA: &str = "3d3c42e5aac5ba805825da76410c181273ba90b1";

#[test]
fn public_repository_uses_full_standard_hosted_rust_ci() {
    let documents = saphyr::YamlOwned::load_from_str(WORKFLOW).expect("hosted CI YAML");
    assert_eq!(documents.len(), 1);

    for required in [
        "pull_request:",
        "push:",
        "branches: [main]",
        "workflow_dispatch:",
        "permissions:\n  contents: read",
        "cancel-in-progress: true",
        "ubuntu-24.04",
        "macos-15",
        "windows-2025",
        &format!("actions/checkout@{CHECKOUT_SHA}"),
        "github.event.pull_request.head.repo.full_name || github.repository",
        "github.event.pull_request.head.sha || github.sha",
        "persist-credentials: false",
        "rustup show active-toolchain",
        "cargo fmt --all -- --check",
        "cargo clippy --locked --workspace --all-targets --all-features -- -D warnings",
        "cargo test --locked --workspace --all-targets --all-features",
        "cargo doc --locked --workspace --all-features --no-deps",
        "cargo run --locked --quiet --example generate_release_metadata -- --check",
        "CCP_TEST_ROOT: ${{ runner.temp }}/ccp-tests",
        "needs: [quality, test]",
        "QUALITY_RESULT: ${{ needs.quality.result }}",
        "TEST_RESULT: ${{ needs.test.result }}",
    ] {
        assert!(
            WORKFLOW.contains(required),
            "missing hosted CI boundary: {required}"
        );
    }

    for forbidden in [
        "pull_request_target:",
        "statuses: write",
        "ccp-evidence/",
        "commit-ci-preflight/receipt",
        "commit-ci-preflight run",
        "guard exec",
        "docker ",
        "self-hosted",
        "actions/cache",
        "cache:",
        "secrets.",
    ] {
        assert!(
            !WORKFLOW.contains(forbidden),
            "forbidden public hosted CI surface: {forbidden}"
        );
    }

    assert!(RUST_TOOLCHAIN.contains("channel = \"1.96.0\""));
    assert!(RUST_TOOLCHAIN.contains("components = [\"clippy\", \"rustfmt\"]"));
    assert!(RUST_TOOLCHAIN.contains("profile = \"minimal\""));
}

#[test]
fn repository_has_no_ordinary_per_pr_receipt_workflow() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    assert!(
        !root.join(".github/workflows/receipt-gate.yml").exists(),
        "public repository must not require local CCP receipts for ordinary pull requests"
    );
}

#[test]
fn runner_temp_is_resolved_inside_the_test_step() {
    let documents = saphyr::YamlOwned::load_from_str(WORKFLOW).expect("hosted CI YAML");
    let root = documents[0].as_mapping().expect("workflow mapping");
    let jobs = mapping_get(root, "jobs")
        .and_then(YamlOwned::as_mapping)
        .expect("jobs mapping");
    let test_job = mapping_get(jobs, "test")
        .and_then(YamlOwned::as_mapping)
        .expect("test job mapping");

    let job_env_has_test_root = mapping_get(test_job, "env")
        .and_then(YamlOwned::as_mapping)
        .and_then(|env| mapping_get(env, "CCP_TEST_ROOT"))
        .is_some();
    assert!(
        !job_env_has_test_root,
        "runner context is unavailable in job-level env"
    );

    let steps = mapping_get(test_job, "steps")
        .and_then(YamlOwned::as_sequence)
        .expect("test steps");
    let suite_step = steps
        .iter()
        .filter_map(YamlOwned::as_mapping)
        .find(|step| {
            mapping_get(step, "name").and_then(YamlOwned::as_str)
                == Some("Run the complete deterministic suite")
        })
        .expect("complete deterministic suite step");
    let test_root = mapping_get(suite_step, "env")
        .and_then(YamlOwned::as_mapping)
        .and_then(|env| mapping_get(env, "CCP_TEST_ROOT"))
        .and_then(YamlOwned::as_str);
    assert_eq!(test_root, Some("${{ runner.temp }}/ccp-tests"));
}

#[test]
fn public_hosted_policy_and_optional_receipt_product_are_documented_separately() {
    let normalized_readme = README.split_whitespace().collect::<Vec<_>>().join(" ");
    for required in [
        "This public repository uses standard GitHub-hosted CI",
        "no billable public-runner savings",
        "historical CCP receipts remain valid",
    ] {
        assert!(
            normalized_readme.contains(required),
            "README boundary missing: {required}"
        );
    }
    for required in [
        "not active for this public repository's ordinary pull requests",
        "cross-repository template",
        "economically qualified",
    ] {
        assert!(
            GITHUB_GATE.contains(required),
            "GitHub gate boundary missing: {required}"
        );
    }
    assert!(CHANGELOG.contains("Replaced this public repository's ordinary per-PR receipt gate"));
}

#[test]
fn economic_case_studies_recompute_remote_savings_from_observed_inputs() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let evidence_path = root.join("docs/evidence/economic-case-studies-2026-08.json");
    assert!(
        evidence_path.is_file(),
        "public economic evidence must exist at {}",
        evidence_path.display()
    );

    let evidence: serde_json::Value =
        serde_json::from_slice(&fs::read(&evidence_path).expect("read public economic evidence"))
            .expect("parse public economic evidence");

    let knowledge = &evidence["case_studies"]["matryca_knowledge"];
    assert_eq!(knowledge["observed_linux_minutes"], 111);
    assert_eq!(knowledge["observed_gross_compute_usd"], 0.666);
    assert_eq!(knowledge["observed_billed_usd"], 0.570);
    assert_eq!(knowledge["baseline_rounded_runner_minutes"], 60);
    assert_eq!(knowledge["retained_rounded_runner_minutes"], 32);
    assert_eq!(knowledge["avoided_rounded_runner_minutes"], 28);
    assert_eq!(knowledge["estimated_avoided_github_compute_usd"], 0.168);

    let brain = &evidence["case_studies"]["matryca_brain"];
    assert_eq!(brain["public_name"], "Matryca-Brain");
    assert_eq!(brain["observed_linux_minutes"], 5_600);
    assert_eq!(brain["observed_gross_compute_usd"], 33.60);
    assert_eq!(brain["observed_gross_storage_usd"], 0.12);
    assert_eq!(brain["observed_gross_total_usd"], 33.72);
    assert_eq!(brain["observed_billed_total_usd"], 15.38);
    assert_eq!(brain["hosted_pr_executions"], 194);
    assert_eq!(brain["hosted_dependency_update_executions"], 22);
    assert_eq!(brain["ccp_guarded_self_hosted_attempts"], 22);
    assert_eq!(brain["ccp_guarded_distinct_commits"], 19);
    assert_eq!(brain["ccp_guarded_outcomes"]["success"], 17);
    assert_eq!(brain["ccp_guarded_outcomes"]["failure"], 1);
    assert_eq!(brain["ccp_guarded_outcomes"]["cancelled"], 4);
    assert_eq!(brain["post_cutover_gross_usd"], 0.00);

    let comparable_hosted_executions = brain["hosted_pr_executions"]
        .as_u64()
        .expect("hosted PR executions");
    let average_minutes = brain["observed_linux_minutes"]
        .as_u64()
        .expect("observed Linux minutes") as f64
        / comparable_hosted_executions as f64;
    let avoided_minutes = average_minutes
        * brain["ccp_guarded_self_hosted_attempts"]
            .as_u64()
            .expect("CCP guarded attempts") as f64;
    let avoided_usd = avoided_minutes
        * evidence["github_pricing"]["linux_2_core_usd_per_minute"]
            .as_f64()
            .expect("Linux runner rate");

    assert!((average_minutes - 28.865_979_381_4).abs() < 1e-9);
    assert!((avoided_minutes - 635.051_546_391_8).abs() < 1e-9);
    assert!((avoided_usd - 3.810_309_278_4).abs() < 1e-9);
    assert_eq!(brain["estimated_avoided_github_compute_minutes"], 635.1);
    assert_eq!(brain["estimated_avoided_github_compute_usd"], 3.81);
    assert_eq!(evidence["claim_boundary"]["net_savings_certified"], false);
}

fn mapping_get<'a>(mapping: &'a MappingOwned, key: &str) -> Option<&'a YamlOwned> {
    mapping
        .iter()
        .find_map(|(candidate, value)| (candidate.as_str() == Some(key)).then_some(value))
}
