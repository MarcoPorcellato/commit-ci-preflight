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

use commit_ci_preflight::receipt::{EvidenceStatus, ReceiptEnvelopeV1};
use sha2::{Digest, Sha256};

const README: &str = include_str!("../README.md");
const SBOM: &str = include_str!("../SBOM.spdx.json");
const NOTICES: &str = include_str!("../THIRD_PARTY_NOTICES.md");
const CARGO_LOCK: &str = include_str!("../Cargo.lock");
const RELEASE_SCRIPT: &str = include_str!("../scripts/build_release_candidate.sh");
const INSTALLATION: &str = include_str!("../docs/INSTALLATION.md");
const ROLLBACK: &str = include_str!("../docs/UPGRADE_AND_ROLLBACK.md");
const THREAT_MODEL: &str = include_str!("../docs/THREAT_MODEL.md");
const BETA_SUPPORT: &str = include_str!("../docs/BETA_SUPPORT.md");
const TUTORIAL: &str = include_str!("../docs/TUTORIAL.md");
const DEMO_RECEIPT: &str = include_str!("../docs/evidence/pr10/demo-rust-receipt.json");

#[test]
fn public_readme_is_human_first_and_truthfully_differentiated() {
    for heading in [
        "## The problem",
        "## How it works",
        "## Quick start",
        "## What makes it different",
        "## When to use it",
        "## When not to use it",
        "## Evidence and limitations",
    ] {
        assert!(
            README.contains(heading),
            "missing README heading: {heading}"
        );
    }
    for official_source in [
        "https://github.com/nektos/act",
        "https://docs.dagger.io/",
        "https://docs.earthly.dev/",
        "https://docs.github.com/en/actions/concepts/runners/self-hosted-runners",
    ] {
        assert!(
            README.contains(official_source),
            "missing official comparison source: {official_source}"
        );
    }
    assert!(!README.to_ascii_lowercase().contains("matryca"));
    assert!(README.contains("not an identity attestation"));
    assert!(README.contains("does not execute marketplace actions"));
}

#[test]
fn checked_in_spdx_sbom_matches_the_locked_package_set() {
    let document: serde_json::Value = serde_json::from_str(SBOM).expect("valid SPDX JSON");
    assert_eq!(document["spdxVersion"], "SPDX-2.3");
    assert_eq!(document["dataLicense"], "CC0-1.0");
    assert_eq!(
        document
            .pointer("/creationInfo/created")
            .and_then(|value| value.as_str()),
        Some("2026-08-10T00:00:00Z")
    );

    let packages = document["packages"].as_array().expect("SPDX packages");
    let lock: toml::Value = toml::from_str(CARGO_LOCK).expect("valid Cargo.lock");
    let locked_packages = lock["package"].as_array().expect("locked packages");
    assert_eq!(packages.len(), locked_packages.len());
    assert!(packages.iter().any(|package| {
        package["name"] == "commit-ci-preflight" && package["versionInfo"] == "0.1.0"
    }));
    assert!(packages.iter().all(|package| {
        package["licenseDeclared"]
            .as_str()
            .is_some_and(|license| !license.is_empty())
    }));
    assert!(
        document["relationships"]
            .as_array()
            .is_some_and(|relationships| relationships.len() > packages.len())
    );

    let lock_digest = Sha256::digest(CARGO_LOCK.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert!(
        document["documentNamespace"]
            .as_str()
            .is_some_and(|namespace| namespace.ends_with(&lock_digest))
    );
    assert!(!SBOM.contains("/Users/"));
    assert!(!SBOM.contains("/private/tmp"));
}

#[test]
fn third_party_notices_include_inventory_and_deduplicated_texts() {
    for dependency in ["clap", "ctrlc", "process-wrap", "serde", "sha2", "toml"] {
        assert!(
            NOTICES.contains(dependency),
            "missing dependency {dependency}"
        );
    }
    assert!(NOTICES.contains("## Deduplicated license and notice texts"));
    assert!(NOTICES.contains("### SHA-256"));
    assert!(NOTICES.contains("Apache License"));
    assert!(!NOTICES.contains("/Users/"));
    assert!(!NOTICES.contains("/private/tmp"));
}

#[test]
fn release_candidate_builder_is_local_bounded_and_non_publishing() {
    for required in [
        "git status --porcelain --untracked-files=all",
        "generate_release_metadata -- --check",
        "cargo test --locked --quiet --test release_hardening_contract",
        "cargo build --locked --release --bin commit-ci-preflight",
        "SBOM.spdx.json",
        "THIRD_PARTY_NOTICES.md",
        "docs/ADOPTION_GUIDE.md",
        "docs/COORDINATION_RUNBOOK.md",
        "docs/INSTALLATION.md",
        "docs/TROUBLESHOOTING.md",
        "docs/UPGRADE_AND_ROLLBACK.md",
        "docs/THREAT_MODEL.md",
        "docs/BETA_SUPPORT.md",
        "docs/TUTORIAL.md",
        "examples/github/receipt-gate.yml.example",
        "mktemp -d",
        "rm -rf -- \"$stage_root\"",
        "SHA256SUMS",
    ] {
        assert!(
            RELEASE_SCRIPT.contains(required),
            "release script is missing: {required}"
        );
    }
    for forbidden in [
        "git push",
        "git tag",
        "cargo publish",
        "gh release",
        "curl ",
        "wget ",
        "docker ",
    ] {
        assert!(
            !RELEASE_SCRIPT.contains(forbidden),
            "release script must not contain: {forbidden}"
        );
    }
}

#[test]
fn beta_documents_keep_release_and_security_boundaries_explicit() {
    assert!(INSTALLATION.starts_with("# Installation and artifact verification"));
    assert!(INSTALLATION.contains("published as a GitHub prerelease"));
    assert!(INSTALLATION.contains("unsigned macOS arm64 archive"));
    assert!(INSTALLATION.contains("There is no crate, Homebrew"));
    assert!(INSTALLATION.contains("or signed artifact"));
    assert!(ROLLBACK.starts_with("# Upgrade, rollback, and uninstall"));
    assert!(ROLLBACK.contains("does not remove"));
    assert!(THREAT_MODEL.starts_with("# Threat model and review closure"));
    assert!(THREAT_MODEL.contains("does not treat a container as a complete sandbox"));
    assert!(THREAT_MODEL.contains("Identity overclaim"));
    assert!(THREAT_MODEL.contains("never executes\npull-request-controlled code"));
    assert!(!THREAT_MODEL.contains("No `pull_request_target` execution"));
    assert!(BETA_SUPPORT.starts_with("# Beta limitations and support policy"));
    assert!(BETA_SUPPORT.contains("| `PUBLISHED_RC` |"));
    assert!(
        BETA_SUPPORT.contains("Registry packages and signed release artifacts | `NOT_PUBLISHED`")
    );
    assert!(BETA_SUPPORT.contains("Complete project `run` path on Windows x86_64 | `PENDING`"));
    assert!(TUTORIAL.starts_with("# End-to-end tutorial"));
    assert!(TUTORIAL.contains("does not prove who ran the command"));
}

#[test]
fn release_metadata_generator_is_wired_into_the_local_preflight() {
    let config = include_str!("../.commit-ci-preflight.toml");
    let policy = include_str!("../.commit-ci-policy.toml");
    assert!(config.contains("id = \"release-metadata\""));
    assert!(config.contains("generate_release_metadata"));
    assert!(policy.contains("\"release-metadata\""));
}

#[test]
fn clean_room_demo_receipt_is_valid_and_matches_the_documented_contract() {
    let envelope: ReceiptEnvelopeV1 =
        serde_json::from_str(DEMO_RECEIPT).expect("valid demo receipt JSON");
    envelope.verify().expect("valid canonical receipt");
    assert_eq!(envelope.receipt.overall_status, EvidenceStatus::Pass);
    assert_eq!(
        envelope.receipt.repository.commit_sha,
        "f7efbe230aaad1392f362974d83fc52b889074e9"
    );
    assert_eq!(
        envelope.receipt.configuration_digest,
        "sha256:cff29e073937f7bd51d611fbd407c94e2ce5915f01ee018f7fb3d8f35819ea98"
    );
    assert_eq!(envelope.receipt.checks.len(), 1);
    assert_eq!(envelope.receipt.checks[0].id, "test");
    assert_eq!(envelope.receipt.checks[0].status, EvidenceStatus::Pass);
    assert!(TUTORIAL.contains("mkdir -p target"));
}
