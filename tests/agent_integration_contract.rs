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

use std::{fs, path::Path};

const HARNESS_SLUGS: &[&str] = &[
    "claude-code",
    "antigravity",
    "codex-app",
    "codex-cli",
    "cursor",
    "devin-cli",
    "factory-droid",
    "gemini-cli",
    "copilot-cli",
    "grok-build-cli",
    "kimi-code",
    "opencode",
    "pi",
    "hermes-agent",
];

const REQUIRED_HEADINGS: &[&str] = &[
    "## Evidence state and scope",
    "## Harness-owned installation surface",
    "## Bootstrap and discovery",
    "## Tool mapping",
    "## CCP activity sequence",
    "## Fresh-session smoke protocol",
    "## Failure and rollback",
    "## Privacy and neutrality",
];

#[test]
fn multi_harness_reference_has_a_complete_truthful_l1_surface() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = read(root, "README.md");
    let common = read(root, "docs/agent-integrations/HARNESS_INTEGRATION.md");
    let matrix = read(root, "docs/agent-integrations/COMPATIBILITY_MATRIX.md");
    let handoff = read(root, "examples/agent/CCP_ACTIVITY_HANDOFF.md");
    let runbook = read(root, "docs/COORDINATION_RUNBOOK.md");
    let roadmap = read(root, "docs/PRODUCT_ROADMAP.md");

    for required in ["exact-head", "admission", "outer guard", "GitHub-hosted CI"] {
        assert!(
            common.contains(required),
            "common contract is missing {required}"
        );
    }
    for required in [
        "exact source SHA",
        "## Terminal handoff",
        "PENDING",
        "GitHub fallback",
    ] {
        assert!(handoff.contains(required), "handoff is missing {required}");
    }
    assert!(!matrix.contains("| VERIFIED"));

    for slug in HARNESS_SLUGS {
        let page = read(
            root,
            &format!("docs/agent-integrations/harnesses/{slug}.md"),
        );
        assert!(matrix.contains(slug), "matrix is missing {slug}");
        for heading in REQUIRED_HEADINGS {
            assert!(page.contains(heading), "{slug} is missing {heading}");
        }
        assert!(page.contains("L0") || page.contains("L1"));
        assert!(page.contains("https://"));
        assert_public_boundary(&page, slug);
    }

    assert_public_boundary(&common, "common contract");
    assert_public_boundary(&matrix, "compatibility matrix");
    assert_public_boundary(&handoff, "activity handoff");
    for required in [
        "Manual quarantine is unsupported.",
        "The owner-approved agent continuation mode is opt-in.",
        "continuation exists solely for orphan prevention.",
        "unknown ownership remains blocking",
        "fail-closed.",
        "explicit claim",
        "no hidden execution",
        "legacy `guard exec`",
        "synchronous and unchanged",
    ] {
        assert!(runbook.contains(required), "runbook is missing {required}");
    }
    for required in [
        "The planned agent admission mode is an owner-approved safety exception",
        "orphan prevention, not a second scheduler.",
        "agent mode opt-in",
        "never bypass unknown ownership",
        "never revive a terminated chat",
        "never auto-execute a command",
        "shell-free wrapper around one explicit program argv",
        "Official launchers must pass through `guard exec` to be covered.",
    ] {
        assert!(readme.contains(required), "README is missing {required}");
    }
    for required in [
        "Owner-authorized safety exception (2026-08-21)",
        "narrow orphan prevention safeguard",
        "single Rust coordinator",
        "fail closed on unknown ownership",
        "explicit claim by a live activity",
        "never execute a stored or hidden command",
        "neither revives a terminated chat nor relaxes any release",
    ] {
        assert!(roadmap.contains(required), "roadmap is missing {required}");
    }
    assert!(readme.contains("docs/agent-integrations/HARNESS_INTEGRATION.md"));
}

#[test]
fn admission_layout_recovery_guidance_is_hash_bound_and_never_manual() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let runbook = read(root, "docs/COORDINATION_RUNBOOK.md");
    let troubleshooting = read(root, "docs/TROUBLESHOOTING.md");
    for required in [
        "admission layout-recovery status --json",
        "admission layout-recovery apply",
        "--expected-plan",
        "recovery_uncertain",
        "does not authorize",
        "Manual deletion",
    ] {
        assert!(
            runbook.contains(required) || troubleshooting.contains(required),
            "missing recovery boundary: {required}"
        );
    }
    assert!(!runbook.contains("ignore `agent-tickets`"));
}

fn read(root: &Path, relative: &str) -> String {
    fs::read_to_string(root.join(relative)).unwrap_or_else(|error| {
        panic!("read {relative}: {error}");
    })
}

fn assert_public_boundary(value: &str, label: &str) {
    for forbidden in [
        "/Users/",
        "/private/tmp",
        "GitNexus",
        "Serena",
        "Bearer ",
        "ghp_",
        "token=",
    ] {
        assert!(
            !value.contains(forbidden),
            "{label} contains a private or vendor-specific boundary: {forbidden}"
        );
    }
}
