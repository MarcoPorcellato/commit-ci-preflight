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
