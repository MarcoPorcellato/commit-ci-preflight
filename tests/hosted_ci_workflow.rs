// Copyright 2026 Marco Porcellato
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

use std::path::PathBuf;

use saphyr::LoadableYamlNode;

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
