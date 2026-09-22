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

use std::fmt;

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: &str = "economic-evaluation-v1";
const MEASURED_MINIMUM: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EvaluationWorksheet {
    pub schema_version: String,
    pub runner_rate_microusd_per_minute: u64,
    pub events: Vec<EvaluationEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EvaluationEvent {
    pub outcome: EventOutcome,
    pub hosted_scope_id: String,
    pub local_scope_id: String,
    pub hosted_dispatched_at_seconds: Option<u64>,
    pub hosted_runner_started_at_seconds: Option<u64>,
    pub hosted_completed_at_seconds: Option<u64>,
    pub hosted_job_seconds: Vec<u64>,
    pub retained_hosted_job_seconds: Vec<u64>,
    pub local_started_at_seconds: Option<u64>,
    pub local_completed_at_seconds: Option<u64>,
    pub local_verified_at_seconds: Option<u64>,
    pub local_preparation_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventOutcome {
    Success,
    Failure,
    Cancelled,
    Incomplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationClass {
    Measured,
    Exploratory,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct OutcomeCounts {
    pub success: u64,
    pub failure: u64,
    pub cancelled: u64,
    pub incomplete: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationError(pub String);

impl fmt::Display for EvaluationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for EvaluationError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvaluationReport {
    pub classification: EvaluationClass,
    pub comparable_success_count: u64,
    pub excluded_event_count: u64,
    pub hosted_median_queue_seconds: u64,
    pub hosted_median_end_to_end_seconds: u64,
    pub local_median_execution_seconds: u64,
    pub local_median_end_to_end_seconds: u64,
    pub local_median_preparation_seconds: u64,
    pub hosted_range_end_to_end_seconds: u64,
    pub local_range_end_to_end_seconds: u64,
    pub avoided_rounded_hosted_minutes: u64,
    pub avoided_github_charge_microusd: u64,
    pub outcome_counts: OutcomeCounts,
}

pub fn evaluate_worksheet(
    worksheet: &EvaluationWorksheet,
) -> Result<EvaluationReport, EvaluationError> {
    if worksheet.schema_version != SCHEMA_VERSION {
        return Err(EvaluationError(format!(
            "unsupported worksheet schema version: {}",
            worksheet.schema_version
        )));
    }
    if worksheet.events.is_empty() {
        return Err(EvaluationError("worksheet must contain events".into()));
    }

    let mut outcome_counts = OutcomeCounts::default();
    let mut hosted_queues = Vec::new();
    let mut hosted_end_to_end = Vec::new();
    let mut local_execution = Vec::new();
    let mut local_end_to_end = Vec::new();
    let mut local_preparation = Vec::new();
    let mut avoided_minutes = 0_u64;

    for event in &worksheet.events {
        match event.outcome {
            EventOutcome::Success => {
                outcome_counts.success += 1;
                if event.hosted_scope_id.is_empty() || event.local_scope_id.is_empty() {
                    return Err(EvaluationError(
                        "successful event scope IDs must be non-empty".into(),
                    ));
                }
                if event.hosted_scope_id != event.local_scope_id {
                    return Err(EvaluationError(
                        "hosted_scope_id must equal local_scope_id".into(),
                    ));
                }
                let hosted_dispatched =
                    required(event.hosted_dispatched_at_seconds, "hosted dispatch")?;
                let hosted_started = required(
                    event.hosted_runner_started_at_seconds,
                    "hosted runner start",
                )?;
                let hosted_completed =
                    required(event.hosted_completed_at_seconds, "hosted completion")?;
                let local_started = required(event.local_started_at_seconds, "local start")?;
                let local_completed =
                    required(event.local_completed_at_seconds, "local completion")?;
                let local_verified =
                    required(event.local_verified_at_seconds, "local verification")?;
                let preparation = required(event.local_preparation_seconds, "local preparation")?;

                if hosted_dispatched > hosted_started
                    || hosted_started > hosted_completed
                    || local_started > local_completed
                    || local_completed > local_verified
                {
                    return Err(EvaluationError("timestamps must be monotonic".into()));
                }

                hosted_queues.push(hosted_started - hosted_dispatched);
                hosted_end_to_end.push(hosted_completed - hosted_dispatched);
                local_execution.push(local_completed - local_started);
                local_end_to_end.push(local_verified - local_started);
                local_preparation.push(preparation);

                let baseline = rounded_sum(&event.hosted_job_seconds)?;
                let retained = rounded_sum(&event.retained_hosted_job_seconds)?;
                let event_avoided_minutes = baseline
                    .checked_sub(retained)
                    .filter(|value| *value > 0)
                    .ok_or_else(|| {
                        EvaluationError("successful event must avoid rounded hosted minutes".into())
                    })?;
                avoided_minutes = avoided_minutes
                    .checked_add(event_avoided_minutes)
                    .ok_or_else(|| EvaluationError("avoided minute total overflowed".into()))?;
            }
            EventOutcome::Failure => outcome_counts.failure += 1,
            EventOutcome::Cancelled => outcome_counts.cancelled += 1,
            EventOutcome::Incomplete => outcome_counts.incomplete += 1,
        }
    }

    if hosted_end_to_end.is_empty() {
        return Err(EvaluationError("worksheet has no successful events".into()));
    }

    let comparable_success_count = hosted_end_to_end.len() as u64;
    let excluded_event_count = worksheet.events.len() as u64 - comparable_success_count;
    let avoided_github_charge_microusd = avoided_minutes
        .checked_mul(worksheet.runner_rate_microusd_per_minute)
        .ok_or_else(|| EvaluationError("avoided GitHub charge overflowed".into()))?;

    Ok(EvaluationReport {
        classification: if hosted_end_to_end.len() >= MEASURED_MINIMUM {
            EvaluationClass::Measured
        } else {
            EvaluationClass::Exploratory
        },
        comparable_success_count,
        excluded_event_count,
        hosted_median_queue_seconds: median(hosted_queues),
        hosted_median_end_to_end_seconds: median(hosted_end_to_end.clone()),
        local_median_execution_seconds: median(local_execution),
        local_median_end_to_end_seconds: median(local_end_to_end.clone()),
        local_median_preparation_seconds: median(local_preparation),
        hosted_range_end_to_end_seconds: range(&hosted_end_to_end),
        local_range_end_to_end_seconds: range(&local_end_to_end),
        avoided_rounded_hosted_minutes: avoided_minutes,
        avoided_github_charge_microusd,
        outcome_counts,
    })
}

fn required(value: Option<u64>, name: &str) -> Result<u64, EvaluationError> {
    value.ok_or_else(|| EvaluationError(format!("successful event is missing {name}")))
}

fn rounded_sum(durations: &[u64]) -> Result<u64, EvaluationError> {
    if durations.is_empty() || durations.contains(&0) {
        return Err(EvaluationError("job durations must be non-zero".into()));
    }
    durations.iter().try_fold(0_u64, |total, seconds| {
        total
            .checked_add(seconds.div_ceil(60))
            .ok_or_else(|| EvaluationError("rounded minute total overflowed".into()))
    })
}

fn median(mut values: Vec<u64>) -> u64 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn range(values: &[u64]) -> u64 {
    let minimum = values.iter().min().expect("non-empty values");
    let maximum = values.iter().max().expect("non-empty values");
    maximum - minimum
}
