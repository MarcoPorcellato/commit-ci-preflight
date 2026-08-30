#![allow(dead_code)]

use commit_ci_preflight::{config, matrix, receipt, run, runtime, verify};

fn main() {
    let _schema: fn() -> Result<String, config::ConfigError> = config::config_schema_json;
    let _canonical = canonical_u8;
}

fn canonical_u8(value: &u8) -> Result<Vec<u8>, receipt::ReceiptError> {
    receipt::canonical_json(value)
}

fn verify_v1(
    bytes: &[u8],
    policy: &verify::VerificationPolicyV1,
    commit: &str,
    evaluated_at: &str,
) -> Result<verify::VerificationReportV1, verify::VerificationError> {
    verify::verify_receipt_document(bytes, policy, commit, evaluated_at)
}

fn verify_v2(
    bytes: &[u8],
    policy: &matrix::MatrixVerificationPolicyV2,
    commit: &str,
    evaluated_at: &str,
) -> Result<verify::VerificationReportV1, matrix::MatrixError> {
    matrix::verify_matrix_receipt_document(bytes, policy, commit, evaluated_at)
}

fn execute<'a>(
    request: &run::RunRequest<'a>,
    runtime_port: &dyn runtime::RuntimePort,
    supervisor: &dyn commit_ci_preflight::process::SupervisorPort,
    cancellation: &commit_ci_preflight::process::CancellationToken,
    clock: &dyn run::Clock,
) -> Result<run::RunOutcome, run::RunError> {
    run::execute_local_run(request, runtime_port, supervisor, cancellation, clock)
}

fn select_runtime(
    kind: config::RuntimeKind,
) -> Result<Box<dyn runtime::RuntimePort>, runtime::RuntimeError> {
    runtime::runtime_for(kind)
}
