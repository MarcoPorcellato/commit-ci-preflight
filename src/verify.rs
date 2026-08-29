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

//! Compatibility facade for the physically independent verifier core.

use std::fmt;
use std::io;
use std::path::Path;

pub use ccp_core::errors::{PolicyError, TrustedPlanError, VerificationError};
pub use ccp_core::verification_model::{
    AcceptedPlatformV1, VerificationDecision, VerificationFindingV1, VerificationReportV1,
    VerificationStatus,
};
pub(crate) use ccp_core::verification_model::{finding, parse_utc_seconds, validate_commit};
pub use ccp_core::verify::{
    POLICY_SCHEMA_VERSION, ProducerContractV1_1, TRUSTED_PLAN_POLICY_SCHEMA_VERSION,
    VERIFICATION_REPORT_SCHEMA_VERSION, VerificationPolicyDocument, VerificationPolicyV1,
    VerificationPolicyV1_1, receipt_input_failure_report, system_evaluated_at_utc,
    trusted_plan_policy_schema_json, validate_verification_policy_path,
    verification_policy_schema_json, verification_report_schema_json, verify_receipt_document,
    verify_receipt_document_for_policy, verify_receipt_document_for_policy_path,
};

/// Compatibility error preserving the adopted root `V2(MatrixError)` payload.
#[derive(Debug)]
pub enum VerificationPolicyDocumentError {
    Io(io::Error),
    TooLarge,
    InvalidUtf8,
    Parse(toml::de::Error),
    UnsupportedSchemaVersion,
    V1(PolicyError),
    V1_1(PolicyError),
    V2(crate::matrix::MatrixError),
}

impl fmt::Display for VerificationPolicyDocumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(_) => formatter.write_str("cannot read verification policy"),
            Self::TooLarge => formatter.write_str("verification policy exceeds size limit"),
            Self::InvalidUtf8 => formatter.write_str("verification policy is not UTF-8"),
            Self::Parse(_) => formatter.write_str("verification policy is not valid TOML"),
            Self::UnsupportedSchemaVersion => {
                formatter.write_str("verification policy schema version is unsupported")
            }
            Self::V1(error) => write!(formatter, "{error}"),
            Self::V1_1(error) => write!(formatter, "{error}"),
            Self::V2(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for VerificationPolicyDocumentError {}

pub fn load_verification_policy_document(
    path: &Path,
) -> Result<VerificationPolicyDocument, VerificationPolicyDocumentError> {
    ccp_core::verify::load_verification_policy_document(path).map_err(|error| match error {
        ccp_core::verify::VerificationPolicyDocumentError::Io(error) => {
            VerificationPolicyDocumentError::Io(error)
        }
        ccp_core::verify::VerificationPolicyDocumentError::TooLarge => {
            VerificationPolicyDocumentError::TooLarge
        }
        ccp_core::verify::VerificationPolicyDocumentError::InvalidUtf8 => {
            VerificationPolicyDocumentError::InvalidUtf8
        }
        ccp_core::verify::VerificationPolicyDocumentError::Parse(error) => {
            VerificationPolicyDocumentError::Parse(error)
        }
        ccp_core::verify::VerificationPolicyDocumentError::UnsupportedSchemaVersion => {
            VerificationPolicyDocumentError::UnsupportedSchemaVersion
        }
        ccp_core::verify::VerificationPolicyDocumentError::V1(error) => {
            VerificationPolicyDocumentError::V1(error)
        }
        ccp_core::verify::VerificationPolicyDocumentError::V1_1(error) => {
            VerificationPolicyDocumentError::V1_1(error)
        }
        ccp_core::verify::VerificationPolicyDocumentError::V2(error) => {
            VerificationPolicyDocumentError::V2(crate::matrix::MatrixError::from(error))
        }
    })
}
