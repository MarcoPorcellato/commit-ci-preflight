use std::error::Error;
use commit_ci_preflight::{config::{ConfigError, ExecutionPlanV1, NormalizedRuntime}, matrix::{MatrixError, MatrixPlanV2}, receipt::{ReceiptEnvelopeV1, ReceiptError}, verify::{PolicyError, VerificationError, VerificationPolicyDocument, VerificationPolicyV1, VerificationPolicyV1_1, VerificationReportV1}};

fn assert_error<E: Error + Send + Sync + 'static>(error: E, expected: &str, has_source: bool) {
    assert_eq!(error.to_string(), expected);
    assert_eq!(error.source().is_some(), has_source);
}

#[test]
fn root_public_api_and_error_contract_is_compile_checked() {
    let _: Option<(ReceiptEnvelopeV1, ExecutionPlanV1, NormalizedRuntime, VerificationPolicyDocument, VerificationPolicyV1, VerificationPolicyV1_1, MatrixPlanV2, VerificationReportV1, ReceiptError, ConfigError, PolicyError, VerificationError, MatrixError)> = None;
    assert_error(ReceiptError::NoChecks, "receipt contains no checks", false);
    assert_error(PolicyError::TooLarge, "verification policy exceeds size limit", false);
    assert_error(VerificationError::TrustedPolicyPathRequired, "trusted-plan policy verification requires the policy file path", false);
    assert_error(ConfigError::NoChecks, "configuration contains no checks", false);
    assert_error(MatrixError::PlanDigestMismatch, "matrix plan digest mismatch", false);
    let io = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
    assert_error(PolicyError::Io(io), "cannot read verification policy", true);
    let parse = toml::from_str::<toml::Value>("[").unwrap_err();
    assert_error(PolicyError::Parse(parse), "verification policy is not valid strict TOML", true);
    assert_error(VerificationError::Policy(PolicyError::TooLarge), "verification policy exceeds size limit", true);
    assert_error(VerificationError::TrustedPlan(commit_ci_preflight::verify::TrustedPlanError::PolicyPath), "trusted policy path has no parent directory", true);
    assert_error(VerificationError::Receipt(ReceiptError::NoChecks), "verification report serialization failed", true);
}
