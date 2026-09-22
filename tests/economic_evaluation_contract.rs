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

use commit_ci_preflight::economic_evaluation::{
    EvaluationClass, EvaluationWorksheet, evaluate_worksheet,
};

fn fixture(name: &str) -> EvaluationWorksheet {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/economic-evaluation-v1")
        .join(name);
    let bytes = std::fs::read_to_string(path).expect("read fixture");
    serde_json::from_str(&bytes).expect("parse fixture")
}

#[test]
fn ten_equivalent_successes_produce_measured_time_and_cost_fields() {
    let worksheet: EvaluationWorksheet = serde_json::from_str(include_str!(
        "fixtures/economic-evaluation-v1/valid-ten-events.json"
    ))
    .expect("valid fixture");

    let report = evaluate_worksheet(&worksheet).expect("evaluate fixture");

    assert_eq!(report.classification, EvaluationClass::Measured);
    assert_eq!(report.comparable_success_count, 10);
    assert_eq!(report.excluded_event_count, 0);
    assert_eq!(report.hosted_median_end_to_end_seconds, 180);
    assert_eq!(report.local_median_end_to_end_seconds, 72);
    assert_eq!(report.avoided_rounded_hosted_minutes, 20);
    assert_eq!(report.avoided_github_charge_microusd, 120_000);
}

#[test]
fn even_sample_medians_use_the_arithmetic_midpoint() {
    let mut worksheet = fixture("valid-ten-events.json");
    worksheet.events.truncate(2);
    worksheet.events[0].hosted_completed_at_seconds = Some(1010);
    worksheet.events[0].local_completed_at_seconds = Some(2005);
    worksheet.events[0].local_verified_at_seconds = Some(2010);
    worksheet.events[1].hosted_completed_at_seconds = Some(3020);
    worksheet.events[1].local_completed_at_seconds = Some(4010);
    worksheet.events[1].local_verified_at_seconds = Some(4020);

    let report = evaluate_worksheet(&worksheet).expect("evaluate even sample");

    assert_eq!(report.hosted_median_end_to_end_seconds, 15);
    assert_eq!(report.local_median_end_to_end_seconds, 15);
}

#[test]
fn mismatched_test_scopes_fail_before_aggregation() {
    let worksheet = fixture("invalid-scope-mismatch.json");
    assert!(
        evaluate_worksheet(&worksheet)
            .expect_err("scope mismatch must fail")
            .to_string()
            .contains("hosted_scope_id must equal local_scope_id")
    );
}

#[test]
fn non_monotonic_timestamps_fail_before_aggregation() {
    let worksheet = fixture("invalid-timestamp-order.json");
    assert!(
        evaluate_worksheet(&worksheet)
            .expect_err("time order must fail")
            .to_string()
            .contains("timestamps must be monotonic")
    );
}

#[test]
fn missing_success_timestamp_fails_before_aggregation() {
    let mut worksheet = fixture("valid-ten-events.json");
    worksheet.events[0].local_verified_at_seconds = None;

    assert!(
        evaluate_worksheet(&worksheet)
            .expect_err("missing timestamp must fail")
            .to_string()
            .contains("successful event is missing local verification")
    );
}

#[test]
fn empty_success_scope_fails_before_aggregation() {
    let mut worksheet = fixture("valid-ten-events.json");
    worksheet.events[0].hosted_scope_id.clear();

    assert!(
        evaluate_worksheet(&worksheet)
            .expect_err("empty scope must fail")
            .to_string()
            .contains("successful event scope IDs must be non-empty")
    );
}

#[test]
fn zero_duration_job_fails_before_aggregation() {
    let mut worksheet = fixture("valid-ten-events.json");
    worksheet.events[0].hosted_job_seconds[0] = 0;

    assert!(
        evaluate_worksheet(&worksheet)
            .expect_err("zero-duration job must fail")
            .to_string()
            .contains("job durations must be non-zero")
    );
}

#[test]
fn non_positive_avoided_minutes_fail_before_aggregation() {
    let mut worksheet = fixture("valid-ten-events.json");
    worksheet.events[0].retained_hosted_job_seconds =
        worksheet.events[0].hosted_job_seconds.clone();

    assert!(
        evaluate_worksheet(&worksheet)
            .expect_err("non-positive saving must fail")
            .to_string()
            .contains("successful event must avoid rounded hosted minutes")
    );
}

#[test]
fn nine_successes_and_one_cancelled_event_are_exploratory_and_visible() {
    let report = evaluate_worksheet(&fixture("nine-successes-one-cancelled.json"))
        .expect("valid exploratory worksheet");
    assert_eq!(report.classification, EvaluationClass::Exploratory);
    assert_eq!(report.comparable_success_count, 9);
    assert_eq!(report.excluded_event_count, 1);
    assert_eq!(report.outcome_counts.cancelled, 1);
}

#[test]
fn zero_rate_reports_no_github_charge_avoided() {
    let report = evaluate_worksheet(&fixture("zero-rate-public-runner.json"))
        .expect("valid zero-rate worksheet");
    assert_eq!(report.avoided_github_charge_microusd, 0);
}

#[test]
fn example_emits_a_privacy_preserving_json_report() {
    let output = std::process::Command::new(env!("CARGO"))
        .args([
            "run",
            "--locked",
            "--quiet",
            "--example",
            "evaluate_economic_case_study",
            "--",
            "tests/fixtures/economic-evaluation-v1/valid-ten-events.json",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run example");
    assert!(
        output.status.success(),
        "example stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert_eq!(report["classification"], "measured");
    assert_eq!(report["hosted_median_end_to_end_seconds"], 180);
    assert_eq!(report["local_median_end_to_end_seconds"], 72);
    assert!(report.get("events").is_none());
}
