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
use std::path::Path;

use commit_ci_preflight::config::config_schema_json;
use commit_ci_preflight::matrix::{
    matrix_config_schema_json, matrix_policy_schema_json, matrix_receipt_schema_json,
};
use commit_ci_preflight::receipt::{
    CheckEvidence, EvidenceStatus, PlatformEvidence, ProducerEvidence, RECEIPT_SCHEMA_VERSION,
    ReceiptEnvelopeV1, ReceiptV1, RepositoryEvidence, RunEvidence, receipt_schema_json,
};
use commit_ci_preflight::verify::{
    verification_policy_schema_json, verification_report_schema_json,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let envelope = ReceiptEnvelopeV1::seal(passing_receipt())?;
    write_exact(
        Path::new("schema/receipt-v1.schema.json"),
        receipt_schema_json()?.as_bytes(),
    )?;
    write_exact(
        Path::new("schema/config-v1.schema.json"),
        config_schema_json()?.as_bytes(),
    )?;
    write_exact(
        Path::new("schema/policy-v1.schema.json"),
        verification_policy_schema_json()?.as_bytes(),
    )?;
    write_exact(
        Path::new("schema/verification-report-v1.schema.json"),
        verification_report_schema_json()?.as_bytes(),
    )?;
    write_exact(
        Path::new("schema/config-v2.schema.json"),
        matrix_config_schema_json()?.as_bytes(),
    )?;
    write_exact(
        Path::new("schema/receipt-v2.schema.json"),
        matrix_receipt_schema_json()?.as_bytes(),
    )?;
    write_exact(
        Path::new("schema/policy-v2.schema.json"),
        matrix_policy_schema_json()?.as_bytes(),
    )?;
    write_exact(
        Path::new("tests/fixtures/receipt-v1-pass.json"),
        &envelope.canonical_bytes()?,
    )?;
    Ok(())
}

fn write_exact(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    let parent = path.parent().expect("generated contract path has a parent");
    fs::create_dir_all(parent)?;
    fs::write(path, bytes)
}

fn digest(fill: char) -> String {
    format!("sha256:{}", fill.to_string().repeat(64))
}

fn passing_receipt() -> ReceiptV1 {
    ReceiptV1 {
        schema_version: RECEIPT_SCHEMA_VERSION.to_owned(),
        producer: ProducerEvidence {
            name: "commit-ci-preflight".to_owned(),
            version: "0.1.0".to_owned(),
        },
        repository: RepositoryEvidence {
            repository: "example/project".to_owned(),
            commit_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            dirty: false,
        },
        run: RunEvidence {
            run_id: "fixture-run-0001".to_owned(),
            generation: 1,
            started_at_utc: "2026-08-08T12:00:00Z".to_owned(),
            finished_at_utc: "2026-08-08T12:00:01Z".to_owned(),
        },
        platform: PlatformEvidence {
            host_os: "macos".to_owned(),
            host_arch: "aarch64".to_owned(),
            runtime_kind: "orbstack".to_owned(),
            runtime_version: "fixture-1".to_owned(),
            image_reference: format!("example.invalid/ci@{}", digest('a')),
            image_digest: digest('a'),
        },
        configuration_digest: digest('b'),
        checks: vec![CheckEvidence {
            id: "rust-test".to_owned(),
            required: true,
            argv: vec!["cargo".to_owned(), "test".to_owned()],
            working_directory: ".".to_owned(),
            status: EvidenceStatus::Pass,
            exit_code: Some(0),
            duration_ms: 1000,
            timed_out: false,
            cancelled: false,
            output_digest: Some(digest('c')),
            incomplete_reason: None,
        }],
        overall_status: EvidenceStatus::Pass,
        incomplete_reason: None,
        redaction_policy_version: "1".to_owned(),
    }
}
