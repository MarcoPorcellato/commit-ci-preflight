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

const WORKFLOW: &str = include_str!("../.github/workflows/native-benchmark.yml");
const CHECKOUT_SHA: &str = "3d3c42e5aac5ba805825da76410c181273ba90b1";
const UPLOAD_SHA: &str = "043fb46d1a93c77aae656e7c1c64a875d1fc6a0a";

#[test]
fn native_benchmark_workflow_is_valid_yaml() {
    use saphyr::{LoadableYamlNode, YamlOwned};

    let documents = YamlOwned::load_from_str(WORKFLOW).expect("workflow YAML");
    assert_eq!(documents.len(), 1);
    assert!(documents[0].is_mapping());
}

#[test]
fn native_benchmark_is_opt_in_pinned_and_least_privilege() {
    for required in [
        "workflow_dispatch:",
        "types: [labeled]",
        "contents: read",
        "cancel-in-progress: false",
        "github.event.label.name == 'native-benchmark'",
        "github.event.pull_request.head.repo.full_name == github.repository",
        "github.event_name == 'workflow_dispatch'",
        "github.event.pull_request.head.sha || github.sha",
        "ref: ${{ github.event.pull_request.head.sha || github.sha }}",
        "persist-credentials: false",
        "fail-fast: false",
        "runner: ubuntu-24.04",
        "runner: windows-2025",
        "timeout-minutes: 15",
        "retention-days: 1",
        "compression-level: 0",
        &format!("actions/checkout@{CHECKOUT_SHA}"),
        &format!("actions/upload-artifact@{UPLOAD_SHA}"),
    ] {
        assert!(
            WORKFLOW.contains(required),
            "missing workflow boundary: {required}"
        );
    }
    for forbidden in [
        "pull_request_target:",
        "permissions: write-all",
        "contents: write",
        "secrets.",
        "actions/cache",
        "ubuntu-latest",
        "windows-latest",
    ] {
        assert!(
            !WORKFLOW.contains(forbidden),
            "forbidden workflow surface: {forbidden}"
        );
    }
}

#[test]
fn workflow_emits_platform_specific_receipts_without_claiming_other_platforms() {
    assert!(WORKFLOW.contains("linux-x86_64-github"));
    assert!(WORKFLOW.contains("windows-x86_64-github"));
    assert!(!WORKFLOW.contains("macos"));
    assert!(WORKFLOW.contains("if-no-files-found: error"));
    assert!(WORKFLOW.contains("scripts/run_native_benchmark.sh"));
    assert!(WORKFLOW.contains("./scripts/run_native_benchmark.ps1"));
}
