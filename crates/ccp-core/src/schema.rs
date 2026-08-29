//! Stable schema entry points for independent consumers.
use crate::matrix::{MatrixConfigV2, MatrixReceiptEnvelopeV2, MatrixVerificationPolicyV2};
use schemars::schema_for;
use serde_json::Value;

pub fn matrix_config_schema() -> Value {
    serde_json::to_value(schema_for!(MatrixConfigV2)).expect("schema serialization")
}
pub fn matrix_receipt_schema() -> Value {
    serde_json::to_value(schema_for!(MatrixReceiptEnvelopeV2)).expect("schema serialization")
}
pub fn matrix_policy_schema() -> Value {
    serde_json::to_value(schema_for!(MatrixVerificationPolicyV2)).expect("schema serialization")
}
