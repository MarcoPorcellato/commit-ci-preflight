use std::error::Error;
use commit_ci_preflight::{config::{ConfigError, ExecutionPlanV1, NormalizedRuntime}, matrix::{MatrixError, MatrixPlanV2}, receipt::{ReceiptEnvelopeV1, ReceiptError}, verify::{PolicyError, VerificationError, VerificationReportV1}};

fn assert_error<E: Error + Send + Sync + 'static>(error: E, expected: &str, has_source: bool) {
    assert_eq!(error.to_string(), expected);
    assert_eq!(error.source().is_some(), has_source);
}

#[test]
fn root_public_api_and_error_contract_is_compile_checked() {
    let _: Option<(ReceiptEnvelopeV1, ExecutionPlanV1, NormalizedRuntime, MatrixPlanV2, VerificationReportV1)> = None;
    assert_error(ReceiptError::NoChecks, "receipt contains no checks", false);
    assert_error(PolicyError::TooLarge, "verification policy exceeds size limit", false);
    assert_error(VerificationError::TrustedPolicyPathRequired, "trusted-plan policy verification requires the policy file path", false);
    assert_error(ConfigError::NoChecks, "configuration contains no checks", false);
    assert_error(MatrixError::PlanDigestMismatch, "matrix plan digest mismatch", false);
}
