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

use commit_ci_preflight::benchmark::{
    BenchmarkEnvelopeV1, benchmark_schema_json, run_benchmark, verify_benchmark_document,
    write_new_receipt,
};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const PINNED_SCHEMA: &str = include_str!("../schema/benchmark-v1.schema.json");

#[test]
fn native_benchmark_replays_the_fixed_result_and_verifies_current_platform() {
    let first = run_benchmark(COMMIT, None).expect("first benchmark");
    let second = run_benchmark(COMMIT, None).expect("second benchmark");

    assert_eq!(first.receipt.result_digest, second.receipt.result_digest);
    assert_eq!(first.receipt.workload, second.receipt.workload);
    assert_eq!(first.receipt.platform, second.receipt.platform);
    assert_ne!(first.receipt.samples_ns, vec![0; 5]);
    verify_benchmark_document(
        &first.canonical_bytes().expect("canonical benchmark"),
        COMMIT,
        std::env::consts::OS,
        std::env::consts::ARCH,
        None,
        None,
    )
    .expect("verify native benchmark");
}

#[test]
fn verifier_rejects_tampering_commit_platform_and_unknown_fields() {
    let envelope = run_benchmark(COMMIT, None).expect("benchmark");
    let bytes = envelope.canonical_bytes().expect("canonical benchmark");
    assert!(
        verify_benchmark_document(
            &bytes,
            "1123456789abcdef0123456789abcdef01234567",
            std::env::consts::OS,
            std::env::consts::ARCH,
            None,
            None,
        )
        .is_err()
    );
    assert!(
        verify_benchmark_document(&bytes, COMMIT, "other", std::env::consts::ARCH, None, None)
            .is_err()
    );

    let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("benchmark JSON");
    value["receipt"]["unexpected"] = serde_json::json!(true);
    assert!(
        verify_benchmark_document(
            &serde_json::to_vec(&value).expect("tampered JSON"),
            COMMIT,
            std::env::consts::OS,
            std::env::consts::ARCH,
            None,
            None,
        )
        .is_err()
    );
}

#[test]
fn output_is_create_new_and_never_overwrites_evidence() {
    let root = test_root("create-new");
    fs::create_dir_all(&root).expect("create benchmark test root");
    let output = root.join("receipt.json");
    let envelope = run_benchmark(COMMIT, None).expect("benchmark");
    write_new_receipt(&output, &envelope).expect("first output");
    let original = fs::read(&output).expect("original receipt");
    assert!(write_new_receipt(&output, &envelope).is_err());
    assert_eq!(fs::read(&output).expect("preserved receipt"), original);
    fs::remove_dir_all(root).expect("remove benchmark test root");
}

#[test]
fn cli_run_and_independent_verify_use_stable_exit_codes() {
    let output = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args(["benchmark", "--commit", COMMIT, "--json"])
        .output()
        .expect("run benchmark CLI");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let envelope: BenchmarkEnvelopeV1 =
        serde_json::from_slice(&output.stdout).expect("benchmark CLI JSON");
    envelope.validate().expect("benchmark integrity");

    let root = test_root("cli-verify");
    fs::create_dir_all(&root).expect("create CLI root");
    let receipt = root.join("receipt.json");
    fs::write(
        &receipt,
        envelope.canonical_bytes().expect("canonical receipt"),
    )
    .expect("write CLI receipt");
    let verify = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args([
            "verify-benchmark",
            "--receipt",
            receipt.to_str().expect("UTF-8 receipt path"),
            "--expected-commit",
            COMMIT,
            "--expected-os",
            std::env::consts::OS,
            "--expected-arch",
            std::env::consts::ARCH,
            "--json",
        ])
        .output()
        .expect("verify benchmark CLI");
    assert!(verify.status.success());
    assert!(verify.stderr.is_empty());
    let report: serde_json::Value =
        serde_json::from_slice(&verify.stdout).expect("verification report");
    assert_eq!(report["decision"], "PASS");

    let mismatch = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args([
            "verify-benchmark",
            "--receipt",
            receipt.to_str().expect("UTF-8 receipt path"),
            "--expected-commit",
            COMMIT,
            "--expected-os",
            "other",
            "--expected-arch",
            std::env::consts::ARCH,
        ])
        .output()
        .expect("mismatched benchmark CLI");
    assert_eq!(mismatch.status.code(), Some(3));

    let missing = Command::new(env!("CARGO_BIN_EXE_commit-ci-preflight"))
        .args([
            "verify-benchmark",
            "--receipt",
            root.join("missing.json")
                .to_str()
                .expect("UTF-8 missing receipt path"),
            "--expected-commit",
            COMMIT,
            "--expected-os",
            std::env::consts::OS,
            "--expected-arch",
            std::env::consts::ARCH,
        ])
        .output()
        .expect("missing benchmark CLI");
    assert_eq!(missing.status.code(), Some(3));
    fs::remove_dir_all(root).expect("remove CLI root");
}

#[test]
fn schema_is_machine_readable_and_covers_native_evidence() {
    let schema = benchmark_schema_json().expect("benchmark schema");
    assert_eq!(schema, PINNED_SCHEMA);
    let value: serde_json::Value = serde_json::from_str(&schema).expect("schema JSON");
    assert_eq!(value["title"], "BenchmarkEnvelopeV1");
    assert!(value["$defs"]["BenchmarkPlatformV1"].is_object());
    assert!(value["$defs"]["BenchmarkRuntimeProbeV1"].is_object());
}

fn test_root(name: &str) -> PathBuf {
    std::env::var_os("CCP_TEST_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(format!(
            "commit-ci-preflight-benchmark-{name}-{}",
            std::process::id()
        ))
}
