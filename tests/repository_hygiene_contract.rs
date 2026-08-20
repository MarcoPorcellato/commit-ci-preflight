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

use saphyr::{LoadableYamlNode, MappingOwned, YamlOwned};

const BUG_FORM: &str = include_str!("../.github/ISSUE_TEMPLATE/bug_report.yml");
const FEATURE_FORM: &str = include_str!("../.github/ISSUE_TEMPLATE/feature_request.yml");
const ADOPTION_FORM: &str = include_str!("../.github/ISSUE_TEMPLATE/adoption_report.yml");
const ISSUE_CONFIG: &str = include_str!("../.github/ISSUE_TEMPLATE/config.yml");
const PR_TEMPLATE: &str = include_str!("../.github/PULL_REQUEST_TEMPLATE.md");
const ROADMAP: &str = include_str!("../ROADMAP.md");
const SOCIAL_PREVIEW: &str = include_str!("../docs/assets/social-preview.svg");

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
