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

use std::{
    fs,
    path::{Component, Path},
};

use saphyr::{LoadableYamlNode, MappingOwned, YamlOwned};

const BUG_FORM: &str = include_str!("../.github/ISSUE_TEMPLATE/bug_report.yml");
const FEATURE_FORM: &str = include_str!("../.github/ISSUE_TEMPLATE/feature_request.yml");
const ADOPTION_FORM: &str = include_str!("../.github/ISSUE_TEMPLATE/adoption_report.yml");
const ISSUE_CONFIG: &str = include_str!("../.github/ISSUE_TEMPLATE/config.yml");
const PR_TEMPLATE: &str = include_str!("../.github/PULL_REQUEST_TEMPLATE.md");
const ROADMAP: &str = include_str!("../ROADMAP.md");
const SOCIAL_PREVIEW: &str = include_str!("../docs/assets/social-preview.svg");
const CACHE_AND_WORKSPACE: &str = include_str!("../docs/CACHE_AND_WORKSPACE.md");
const TESTING_AND_FAULT_INJECTION: &str = include_str!("../docs/TESTING_AND_FAULT_INJECTION.md");

#[test]
fn cache_payload_documentation_contract() {
    for phrase in [
        "control plane",
        "payload plane",
        "never follows a payload link target on the host",
        "relative, absolute, broken, recursive, and outside-root",
        "Windows link-bearing payload reuse remains unsupported",
        "one node and one non-directory object",
        "control-plane and payload-root links still fail closed",
    ] {
        assert!(CACHE_AND_WORKSPACE.contains(phrase), "missing {phrase}");
    }
    for reference in [
        "src/cache.rs::complete_payload_symlinks_are_preserved_across_generation_reuse",
        "src/cache.rs::failed_payload_preflight_removes_the_new_staging_generation",
        "src/cache.rs::staging_cleanup_unlinks_payload_links_without_touching_targets",
    ] {
        assert!(
            TESTING_AND_FAULT_INJECTION.contains(reference),
            "missing {reference}"
        );
    }
}

#[test]
fn matrix_legacy_profile_is_documented_without_production_digest_constants() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for path in ["docs/CONFIGURATION.md", "docs/LOCAL_RUN.md"] {
        let text = fs::read_to_string(root.join(path)).expect("read operator docs");
        for command in [
            "plan --matrix-plan-profile matrix-v2-legacy-v1 --json",
            "doctor --matrix-plan-profile matrix-v2-legacy-v1 --json",
            "dry-run --matrix-plan-profile matrix-v2-legacy-v1 --json",
            "run --matrix-plan-profile matrix-v2-legacy-v1 --generation N --json",
        ] {
            assert!(text.contains(command), "{path} missing {command}");
        }
    }
    for (path, snippets) in [
        (
            "docs/RECEIPT_SPEC.md",
            &[
                "0.1.0+matrix-v2-legacy-v1",
                "outer Matrix schema 2.0",
                "inner runtime schema 1.0",
                "never from a completed receipt",
            ] as &[&str],
        ),
        (
            "docs/MULTI_RUNTIME_RECEIPTS.md",
            &[
                "outer-v2",
                "inner-v1",
                "producer suffix",
                "historical verifier",
                "tests/verification_contract.rs::historical_matrix_verifier_accepts_legacy_profile_receipt_and_rejects_mutations",
                "#[ignore]",
                "--ignored",
                "CCP_HISTORICAL_VERIFIER_044697",
                "provenance-pinned",
            ],
        ),
        (
            "docs/GITHUB_GATE.md",
            &["append-once", "`verify` has no profile flag", "current-v2"],
        ),
    ] {
        let text = fs::read_to_string(root.join(path)).expect("read contract docs");
        for snippet in snippets {
            assert!(text.contains(snippet), "{path} missing {snippet}");
        }
    }
    for (path, snippets) in [
        (
            "docs/ADOPTION_GUIDE.md",
            &["Matrix-only", "policy inference", "cache namespaces"] as &[&str],
        ),
        (
            "docs/CACHE_AND_WORKSPACE.md",
            &["Matrix-only", "separate legacy cache", "policy inference"],
        ),
        (
            "docs/TROUBLESHOOTING.md",
            &["Matrix-only", "policy migration", "policy inference"],
        ),
        (
            "docs/INVARIANT_EVIDENCE_MATRIX.md",
            &[
                "tests/matrix_contract.rs::legacy_profile_reproduces_historical_plan",
                "current-v2",
            ],
        ),
        (
            "docs/TESTING_AND_FAULT_INJECTION.md",
            &[
                "CCP_HISTORICAL_VERIFIER_044697",
                "ordinary suite does not prove",
                "tests/plan_cli.rs::matrix_plan_profile_flag_is_exposed_only_by_configuration_commands",
                "tests/runtime_cli.rs::legacy_profile_rejection_precedes_shared_state",
                "tests/runtime_cli.rs::legacy_profile_rejects_current_only_matrix_syntax_before_shared_state",
            ],
        ),
    ] {
        let text = fs::read_to_string(root.join(path)).expect("read boundary docs");
        for snippet in snippets {
            assert!(text.contains(snippet), "{path} missing {snippet}");
        }
    }
    assert_documented_test_references_are_valid(
        root,
        [
            "docs/INVARIANT_EVIDENCE_MATRIX.md",
            "docs/TESTING_AND_FAULT_INJECTION.md",
        ],
    );
    let mut sources = Vec::new();
    collect_rust_sources(&root.join("src"), &mut sources);
    for path in sources {
        let text = fs::read_to_string(path).expect("read source");
        for digest in [
            "25b35b942a6ff9b6237ebed7cefbdbc96b968bbe8954a38b606942f36b8df4b2",
            "b3d8beef1542566d9d925bfee77d2244995dc74adcd879128ef65e82ed1d354b",
            "d446c4ca0602c09eee61c796ad2972f58ab0eebe84a39f928fd90aac5bfb535c",
            "13f4cb39b7e1a8ed31cae64502cc8e4d80d040230d3fb410a6afc3bad3b76178",
            "eff5b7d55bb0220890dbfb050bb68a1e0fbba8f9a30a69e2f66085354fcc8562",
            "7afb3e6dd435d9d5a317e4d9d85e80527431044312bbe299e9a70b6ba9e994c8",
        ] {
            assert!(
                !text.contains(digest),
                "production source embeds adopter digest {digest}"
            );
        }
    }
}

fn assert_documented_test_references_are_valid<'a>(
    root: &Path,
    documents: impl IntoIterator<Item = &'a str>,
) {
    for document in documents {
        let text = fs::read_to_string(root.join(document)).expect("read evidence document");
        for reference in backticked_test_references(&text) {
            assert_documented_test_reference_is_safe_and_defined(root, document, reference);
        }
    }
}

fn backticked_test_references(document: &str) -> impl Iterator<Item = &str> {
    document
        .split('`')
        .enumerate()
        .filter_map(|(index, reference)| {
            (index % 2 == 1 && reference.starts_with("tests/") && reference.contains(".rs::"))
                .then_some(reference)
        })
}

fn assert_documented_test_reference_is_safe_and_defined(
    root: &Path,
    document: &str,
    reference: &str,
) {
    let (relative_path, name) = reference
        .split_once("::")
        .unwrap_or_else(|| panic!("{document} has invalid test reference {reference}"));
    assert!(
        !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte == b'_' || byte.is_ascii_alphanumeric()),
        "{document} has unsafe test function name {name}"
    );

    let path = Path::new(relative_path);
    let components: Vec<_> = path.components().collect();
    assert!(
        matches!(
            components.as_slice(),
            [Component::Normal(directory), Component::Normal(file)]
                if *directory == "tests" && file.to_string_lossy().ends_with(".rs")
        ),
        "{document} has unsafe or non-tests path {relative_path}"
    );

    let source = fs::read_to_string(root.join(path))
        .unwrap_or_else(|_| panic!("{document} references missing test source {relative_path}"));
    assert!(
        source.contains(&format!("fn {name}(")),
        "{document} references missing test {reference}"
    );
}

fn collect_rust_sources(dir: &Path, output: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).expect("read src").flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_sources(&path, output);
        } else if path.extension().and_then(|v| v.to_str()) == Some("rs") {
            output.push(path);
        }
    }
}

#[test]
fn issue_template_yaml_is_present_and_safe() {
    assert_yaml_mapping("issue template config", ISSUE_CONFIG, true);
    assert_yaml_mapping("bug report form", BUG_FORM, true);
    assert_yaml_mapping("feature request form", FEATURE_FORM, true);
    assert_yaml_mapping("adoption report form", ADOPTION_FORM, true);

    assert!(contains_field(ISSUE_CONFIG, "blank_issues_enabled", false));

    assert!(contains_field(BUG_FORM, "name", true));
    assert!(contains_field(BUG_FORM, "description", true));
    assert!(contains_field(BUG_FORM, "title", true));
    assert!(contains_field(BUG_FORM, "labels", true));
    assert!(contains_field(BUG_FORM, "body", true));

    assert!(contains_field(FEATURE_FORM, "name", true));
    assert!(contains_field(FEATURE_FORM, "description", true));
    assert!(contains_field(FEATURE_FORM, "title", true));
    assert!(contains_field(FEATURE_FORM, "labels", true));
    assert!(contains_field(FEATURE_FORM, "body", true));

    assert!(contains_field(ADOPTION_FORM, "name", true));
    assert!(contains_field(ADOPTION_FORM, "description", true));
    assert!(contains_field(ADOPTION_FORM, "title", true));
    assert!(contains_field(ADOPTION_FORM, "labels", true));
    assert!(contains_field(ADOPTION_FORM, "body", true));
}

#[test]
fn templates_do_not_include_unsafe_claim_phrases() {
    const FORBIDDEN: &[&str] = &[
        "always works",
        "eliminates all",
        "fully replaced",
        "full replacement",
        "guarantee",
        "guarantees",
        "100% ",
        "zero risk",
    ];

    for forbidden in FORBIDDEN {
        for (name, template) in [
            ("bug form", BUG_FORM),
            ("feature form", FEATURE_FORM),
            ("adoption form", ADOPTION_FORM),
            ("PR template", PR_TEMPLATE),
            ("roadmap", ROADMAP),
        ] {
            assert!(
                !template.to_lowercase().contains(forbidden),
                "forbidden claim phrase '{forbidden}' in {name}"
            );
        }
    }
}

#[test]
fn pr_template_contains_trust_claim_and_evidence_sections() {
    assert!(PR_TEMPLATE.contains("## Trust claim checklist"));
    assert!(PR_TEMPLATE.contains("## Impact and rollback"));
    assert!(PR_TEMPLATE.contains("## Evidence checklist"));
    assert!(PR_TEMPLATE.contains("A0"));
    assert!(PR_TEMPLATE.contains("Roadmap text is presented as planned work"));
    assert!(PR_TEMPLATE.contains("PENDING"));
    assert!(PR_TEMPLATE.contains("NOT-RUN"));
}

#[test]
fn roadmap_and_templates_reference_existing_local_docs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));

    for path in [
        ".github/ISSUE_TEMPLATE/bug_report.yml",
        ".github/ISSUE_TEMPLATE/feature_request.yml",
        ".github/ISSUE_TEMPLATE/adoption_report.yml",
        ".github/ISSUE_TEMPLATE/config.yml",
        ".github/PULL_REQUEST_TEMPLATE.md",
        "ROADMAP.md",
        "docs/ADOPTION_GUIDE.md",
        "docs/RECEIPT_SPEC.md",
        "docs/PRODUCT_ROADMAP.md",
        "docs/PROGRAMME_EXECUTION.md",
        "docs/REPOSITORY_PRESENTATION.md",
        "docs/assets/social-preview.svg",
    ] {
        assert!(
            root.join(path).exists(),
            "expected repository-hygiene file exists: {path}"
        );
    }
}

#[test]
fn programme_execution_ledger_is_discoverable_from_the_public_readme() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = fs::read_to_string(root.join("README.md")).expect("read README");
    let ledger = fs::read_to_string(root.join("docs/PROGRAMME_EXECUTION.md"))
        .expect("read programme execution ledger");

    assert!(readme.contains("docs/PROGRAMME_EXECUTION.md"));
    assert!(ledger.contains("## Near-term execution order"));
    assert!(ledger.contains("## Decision and stop rules"));
    assert!(ledger.contains("PENDING"));
}

#[test]
fn social_preview_is_bounded_self_contained_and_readable() {
    assert!(SOCIAL_PREVIEW.contains("width=\"1280\" height=\"640\""));
    assert!(SOCIAL_PREVIEW.contains("Commit CI Preflight"));
    assert!(SOCIAL_PREVIEW.contains("1 · RUN LOCAL"));
    assert!(SOCIAL_PREVIEW.contains("4 · STATUS"));
    assert!(!SOCIAL_PREVIEW.contains("<script"));
    assert!(!SOCIAL_PREVIEW.contains("href="));
    assert!(!SOCIAL_PREVIEW.contains("url("));
}

fn assert_yaml_mapping(name: &str, raw_yaml: &str, check_reject_unsafe: bool) {
    let documents = YamlOwned::load_from_str(raw_yaml).unwrap_or_else(|error| {
        panic!("failed to parse {name} as yaml: {error}");
    });

    assert_eq!(
        documents.len(),
        1,
        "{name} should contain a single yaml document"
    );
    assert!(documents[0].is_mapping(), "{name} should be a yaml mapping");

    if check_reject_unsafe {
        reject_unsafe_yaml(&documents[0], name);
    }
}

fn contains_field(raw_yaml: &str, field: &str, assert_scalar_or_array: bool) -> bool {
    let documents = YamlOwned::load_from_str(raw_yaml).expect("yaml");
    let node = documents[0].as_mapping().expect("mapping");
    match mapping_get(node, field) {
        Some(value) if assert_scalar_or_array => {
            value.is_mapping() || value.is_sequence() || value.as_str().is_some()
        }
        Some(_) => true,
        None => false,
    }
}

fn mapping_get<'a>(mapping: &'a MappingOwned, key: &str) -> Option<&'a YamlOwned> {
    mapping
        .iter()
        .find_map(|(candidate, value)| (candidate.as_str() == Some(key)).then_some(value))
}

fn reject_unsafe_yaml(node: &YamlOwned, path: &str) {
    match node {
        YamlOwned::Alias(_) => panic!("issue templates must not use YAML aliases: {path}"),
        YamlOwned::Tagged(_, _) => panic!("issue templates must not use YAML tags: {path}"),
        YamlOwned::BadValue => panic!("issue templates yaml contains invalid value: {path}"),
        YamlOwned::Representation(_, _, _) => {
            panic!("issue templates yaml contains unsupported representation: {path}");
        }
        YamlOwned::Sequence(sequence) => {
            for (index, child) in sequence.iter().enumerate() {
                reject_unsafe_yaml(child, &format!("{path}[{index}]"));
            }
        }
        YamlOwned::Mapping(mapping) => {
            for (key, value) in mapping {
                reject_unsafe_yaml(key, path);
                reject_unsafe_yaml(value, path);
            }
        }
        YamlOwned::Value(_) => {}
    }
}
