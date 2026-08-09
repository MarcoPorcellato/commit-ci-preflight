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
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};

use crate::config::RuntimeKind;
use crate::receipt::{EvidenceStatus, ReceiptError, canonical_digest, canonical_json};
use crate::runtime::{ContainmentMechanism, GracefulStopCapability, RuntimeFlavor, RuntimeProbe};

pub const BENCHMARK_SCHEMA_VERSION: &str = "1.0";
pub const BENCHMARK_WORKLOAD_NAME: &str = "canonical-sha256-chain";
pub const BENCHMARK_WORKLOAD_VERSION: &str = "1";
pub const BENCHMARK_ITERATIONS: u32 = 4096;
pub const BENCHMARK_SAMPLES: usize = 5;
pub const MAX_BENCHMARK_RECEIPT_BYTES: usize = 1_048_576;
pub const MAX_SAMPLE_NS: u64 = 3_600_000_000_000;
pub const EXPECTED_WORKLOAD_DIGEST: &str =
    "sha256:29ac09de518a019bd8c663b411f77bbab466c7cf7236b2f56c2dbb6b105c69dc";
const PAYLOAD: &str = "commit-ci-preflight benchmark contract v1: deterministic canonical JSON and SHA-256 chain; timings are observations, never correctness inputs";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkEnvelopeV1 {
    pub receipt: BenchmarkReceiptV1,
    pub benchmark_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkReceiptV1 {
    pub schema_version: String,
    pub benchmark_commit: String,
    pub workload: BenchmarkWorkloadV1,
    pub platform: BenchmarkPlatformV1,
    pub runtime_probe: Option<BenchmarkRuntimeProbeV1>,
    pub samples_ns: Vec<u64>,
    pub median_ns: u64,
    pub result_digest: String,
    pub correctness_status: EvidenceStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkWorkloadV1 {
    pub name: String,
    pub version: String,
    pub iterations_per_sample: u32,
    pub samples: usize,
    pub payload_bytes: usize,
    pub expected_result_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkPlatformV1 {
    pub host_os: String,
    pub host_arch: String,
    pub execution_kind: String,
    pub ci_environment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkRuntimeProbeV1 {
    pub runtime_kind: String,
    pub runtime_flavor: String,
    pub server_version: Option<String>,
    pub operating_system: Option<String>,
    pub os_type: Option<String>,
    pub containment: String,
    pub graceful_stop: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct WorkloadInput<'a> {
    workload_version: &'static str,
    iteration: u32,
    previous_digest: &'a str,
    payload: &'static str,
}

impl BenchmarkEnvelopeV1 {
    pub fn new(receipt: BenchmarkReceiptV1) -> Result<Self, BenchmarkError> {
        validate_receipt(&receipt)?;
        let benchmark_id = canonical_digest(&receipt).map_err(BenchmarkError::Receipt)?;
        Ok(Self {
            receipt,
            benchmark_id,
        })
    }

    pub fn validate(&self) -> Result<(), BenchmarkError> {
        validate_receipt(&self.receipt)?;
        let expected = canonical_digest(&self.receipt).map_err(BenchmarkError::Receipt)?;
        if self.benchmark_id != expected {
            return Err(BenchmarkError::IntegrityMismatch);
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, BenchmarkError> {
        self.validate()?;
        canonical_json(self).map_err(BenchmarkError::Receipt)
    }
}

pub fn run_benchmark(
    benchmark_commit: &str,
    runtime_probe: Option<&RuntimeProbe>,
) -> Result<BenchmarkEnvelopeV1, BenchmarkError> {
    validate_commit(benchmark_commit)?;
    let mut samples_ns = Vec::with_capacity(BENCHMARK_SAMPLES);
    let mut result_digest = None;
    for _ in 0..BENCHMARK_SAMPLES {
        let started = Instant::now();
        let sample_result = execute_workload()?;
        let elapsed = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        if sample_result != EXPECTED_WORKLOAD_DIGEST {
            return Err(BenchmarkError::WorkloadMismatch(sample_result));
        }
        if let Some(previous) = &result_digest
            && previous != &sample_result
        {
            return Err(BenchmarkError::NonDeterministicWorkload);
        }
        result_digest = Some(sample_result);
        samples_ns.push(elapsed.max(1));
    }
    let median_ns = median(&samples_ns)?;
    let receipt = BenchmarkReceiptV1 {
        schema_version: BENCHMARK_SCHEMA_VERSION.to_owned(),
        benchmark_commit: benchmark_commit.to_owned(),
        workload: BenchmarkWorkloadV1 {
            name: BENCHMARK_WORKLOAD_NAME.to_owned(),
            version: BENCHMARK_WORKLOAD_VERSION.to_owned(),
            iterations_per_sample: BENCHMARK_ITERATIONS,
            samples: BENCHMARK_SAMPLES,
            payload_bytes: PAYLOAD.len(),
            expected_result_digest: EXPECTED_WORKLOAD_DIGEST.to_owned(),
        },
        platform: BenchmarkPlatformV1 {
            host_os: std::env::consts::OS.to_owned(),
            host_arch: std::env::consts::ARCH.to_owned(),
            execution_kind: "native_process".to_owned(),
            ci_environment: github_actions_environment(),
        },
        runtime_probe: runtime_probe.map(BenchmarkRuntimeProbeV1::from),
        samples_ns,
        median_ns,
        result_digest: result_digest.ok_or(BenchmarkError::NoSamples)?,
        correctness_status: EvidenceStatus::Pass,
    };
    BenchmarkEnvelopeV1::new(receipt)
}

pub fn verify_benchmark_document(
    input: &[u8],
    expected_commit: &str,
    expected_os: &str,
    expected_arch: &str,
    expected_ci_environment: Option<&str>,
    expected_runtime_flavor: Option<&str>,
) -> Result<BenchmarkEnvelopeV1, BenchmarkError> {
    if input.len() > MAX_BENCHMARK_RECEIPT_BYTES {
        return Err(BenchmarkError::ReceiptTooLarge(input.len()));
    }
    validate_commit(expected_commit)?;
    let envelope: BenchmarkEnvelopeV1 =
        serde_json::from_slice(input).map_err(BenchmarkError::Json)?;
    envelope.validate()?;
    if envelope.receipt.benchmark_commit != expected_commit {
        return Err(BenchmarkError::CommitMismatch);
    }
    if envelope.receipt.platform.host_os != expected_os {
        return Err(BenchmarkError::PlatformMismatch("host_os"));
    }
    if envelope.receipt.platform.host_arch != expected_arch {
        return Err(BenchmarkError::PlatformMismatch("host_arch"));
    }
    if envelope.receipt.platform.ci_environment.as_deref() != expected_ci_environment {
        return Err(BenchmarkError::PlatformMismatch("ci_environment"));
    }
    if envelope
        .receipt
        .runtime_probe
        .as_ref()
        .map(|probe| probe.runtime_flavor.as_str())
        != expected_runtime_flavor
    {
        return Err(BenchmarkError::PlatformMismatch("runtime_flavor"));
    }
    Ok(envelope)
}

pub fn write_new_receipt(
    path: &Path,
    envelope: &BenchmarkEnvelopeV1,
) -> Result<(), BenchmarkError> {
    let bytes = envelope.canonical_bytes()?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|source| BenchmarkError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(BenchmarkError::InvalidOutputPath)?;
    let temporary = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|source| BenchmarkError::Io {
            path: temporary.clone(),
            source,
        })?;
    file.write_all(&bytes)
        .map_err(|source| BenchmarkError::Io {
            path: temporary.clone(),
            source,
        })?;
    file.write_all(b"\n").map_err(|source| BenchmarkError::Io {
        path: temporary.clone(),
        source,
    })?;
    file.sync_all().map_err(|source| BenchmarkError::Io {
        path: temporary.clone(),
        source,
    })?;
    if let Err(source) = fs::hard_link(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        if source.kind() == std::io::ErrorKind::AlreadyExists {
            return Err(BenchmarkError::OutputExists(path.to_path_buf()));
        }
        return Err(BenchmarkError::Io {
            path: path.to_path_buf(),
            source,
        });
    }
    fs::remove_file(&temporary).map_err(|source| BenchmarkError::Io {
        path: temporary,
        source,
    })?;
    Ok(())
}

pub fn benchmark_schema_json() -> Result<String, BenchmarkError> {
    let mut json = serde_json::to_string_pretty(&schema_for!(BenchmarkEnvelopeV1))
        .map_err(BenchmarkError::Json)?;
    json.push('\n');
    Ok(json)
}

pub fn execute_workload() -> Result<String, BenchmarkError> {
    let mut digest = format!("sha256:{}", "0".repeat(64));
    for iteration in 0..BENCHMARK_ITERATIONS {
        digest = canonical_digest(&WorkloadInput {
            workload_version: BENCHMARK_WORKLOAD_VERSION,
            iteration,
            previous_digest: &digest,
            payload: PAYLOAD,
        })
        .map_err(BenchmarkError::Receipt)?;
    }
    Ok(digest)
}

fn validate_receipt(receipt: &BenchmarkReceiptV1) -> Result<(), BenchmarkError> {
    if receipt.schema_version != BENCHMARK_SCHEMA_VERSION {
        return Err(BenchmarkError::UnsupportedSchema);
    }
    validate_commit(&receipt.benchmark_commit)?;
    if receipt.workload.name != BENCHMARK_WORKLOAD_NAME
        || receipt.workload.version != BENCHMARK_WORKLOAD_VERSION
        || receipt.workload.iterations_per_sample != BENCHMARK_ITERATIONS
        || receipt.workload.samples != BENCHMARK_SAMPLES
        || receipt.workload.payload_bytes != PAYLOAD.len()
        || receipt.workload.expected_result_digest != EXPECTED_WORKLOAD_DIGEST
        || receipt.result_digest != EXPECTED_WORKLOAD_DIGEST
        || receipt.correctness_status != EvidenceStatus::Pass
    {
        return Err(BenchmarkError::InvalidWorkloadContract);
    }
    if receipt.samples_ns.len() != BENCHMARK_SAMPLES
        || receipt
            .samples_ns
            .iter()
            .any(|sample| *sample == 0 || *sample > MAX_SAMPLE_NS)
        || receipt.median_ns != median(&receipt.samples_ns)?
    {
        return Err(BenchmarkError::InvalidSamples);
    }
    if !matches!(
        receipt.platform.host_os.as_str(),
        "linux" | "macos" | "windows"
    ) || !matches!(receipt.platform.host_arch.as_str(), "aarch64" | "x86_64")
        || receipt.platform.execution_kind != "native_process"
        || receipt
            .platform
            .ci_environment
            .as_deref()
            .is_some_and(|value| value != "github_actions")
    {
        return Err(BenchmarkError::InvalidPlatform);
    }
    if let Some(probe) = &receipt.runtime_probe
        && (probe.runtime_kind != "docker_compatible"
            || !matches!(
                probe.runtime_flavor.as_str(),
                "docker_compatible" | "orbstack"
            )
            || probe.containment.is_empty()
            || probe.graceful_stop.is_empty())
    {
        return Err(BenchmarkError::InvalidRuntimeProbe);
    }
    Ok(())
}

fn validate_commit(value: &str) -> Result<(), BenchmarkError> {
    if value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(BenchmarkError::InvalidCommit)
    }
}

fn median(samples: &[u64]) -> Result<u64, BenchmarkError> {
    if samples.len() != BENCHMARK_SAMPLES || BENCHMARK_SAMPLES.is_multiple_of(2) {
        return Err(BenchmarkError::NoSamples);
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    Ok(sorted[sorted.len() / 2])
}

fn github_actions_environment() -> Option<String> {
    (std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")).then(|| "github_actions".to_owned())
}

impl From<&RuntimeProbe> for BenchmarkRuntimeProbeV1 {
    fn from(probe: &RuntimeProbe) -> Self {
        Self {
            runtime_kind: match probe.runtime {
                RuntimeKind::DockerCompatible => "docker_compatible",
                RuntimeKind::Host => "host",
            }
            .to_owned(),
            runtime_flavor: match probe.flavor {
                RuntimeFlavor::DockerCompatible => "docker_compatible",
                RuntimeFlavor::OrbStack => "orbstack",
            }
            .to_owned(),
            server_version: probe.server_version.clone(),
            operating_system: probe.operating_system.clone(),
            os_type: probe.os_type.clone(),
            containment: match probe.containment {
                ContainmentMechanism::ProcessGroup => "process_group",
                ContainmentMechanism::JobObject => "job_object",
            }
            .to_owned(),
            graceful_stop: match probe.graceful_stop {
                GracefulStopCapability::ProcessGroupSignal => "process_group_signal",
                GracefulStopCapability::HardStopOnly => "hard_stop_only",
            }
            .to_owned(),
        }
    }
}

#[derive(Debug)]
pub enum BenchmarkError {
    Receipt(ReceiptError),
    Json(serde_json::Error),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    InvalidCommit,
    UnsupportedSchema,
    InvalidWorkloadContract,
    InvalidSamples,
    InvalidPlatform,
    InvalidRuntimeProbe,
    IntegrityMismatch,
    CommitMismatch,
    PlatformMismatch(&'static str),
    WorkloadMismatch(String),
    NonDeterministicWorkload,
    ReceiptTooLarge(usize),
    InvalidOutputPath,
    OutputExists(PathBuf),
    NoSamples,
}

impl fmt::Display for BenchmarkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Receipt(error) => write!(formatter, "benchmark receipt error: {error}"),
            Self::Json(_) => formatter.write_str("invalid benchmark receipt JSON"),
            Self::Io { path, .. } => {
                write!(formatter, "benchmark I/O failed at {}", path.display())
            }
            Self::InvalidCommit => {
                formatter.write_str("benchmark commit must be a lowercase 40-character Git SHA")
            }
            Self::UnsupportedSchema => formatter.write_str("unsupported benchmark schema version"),
            Self::InvalidWorkloadContract => {
                formatter.write_str("benchmark workload contract is invalid")
            }
            Self::InvalidSamples => formatter.write_str("benchmark timing samples are invalid"),
            Self::InvalidPlatform => formatter.write_str("benchmark platform evidence is invalid"),
            Self::InvalidRuntimeProbe => formatter.write_str("benchmark runtime probe is invalid"),
            Self::IntegrityMismatch => formatter.write_str("benchmark receipt integrity mismatch"),
            Self::CommitMismatch => {
                formatter.write_str("benchmark commit does not match the expected commit")
            }
            Self::PlatformMismatch(field) => {
                write!(formatter, "benchmark platform mismatch for {field}")
            }
            Self::WorkloadMismatch(actual) => {
                write!(formatter, "benchmark workload digest mismatch: {actual}")
            }
            Self::NonDeterministicWorkload => {
                formatter.write_str("benchmark workload produced inconsistent sample digests")
            }
            Self::ReceiptTooLarge(actual) => write!(
                formatter,
                "benchmark receipt is {actual} bytes; maximum is {MAX_BENCHMARK_RECEIPT_BYTES}"
            ),
            Self::InvalidOutputPath => formatter.write_str("benchmark output path is invalid"),
            Self::OutputExists(path) => write!(
                formatter,
                "benchmark output already exists: {}",
                path.display()
            ),
            Self::NoSamples => formatter.write_str("benchmark sample contract is invalid"),
        }
    }
}

impl std::error::Error for BenchmarkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Receipt(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BENCHMARK_SAMPLES, BenchmarkEnvelopeV1, BenchmarkPlatformV1, BenchmarkReceiptV1,
        BenchmarkWorkloadV1, EXPECTED_WORKLOAD_DIGEST, execute_workload,
    };
    use crate::receipt::EvidenceStatus;

    #[test]
    fn workload_result_is_pinned() {
        assert_eq!(
            execute_workload().expect("workload"),
            EXPECTED_WORKLOAD_DIGEST
        );
    }

    #[test]
    fn envelope_detects_timing_and_identity_tampering() {
        let receipt = BenchmarkReceiptV1 {
            schema_version: "1.0".to_owned(),
            benchmark_commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            workload: BenchmarkWorkloadV1 {
                name: "canonical-sha256-chain".to_owned(),
                version: "1".to_owned(),
                iterations_per_sample: 4096,
                samples: BENCHMARK_SAMPLES,
                payload_bytes: super::PAYLOAD.len(),
                expected_result_digest: EXPECTED_WORKLOAD_DIGEST.to_owned(),
            },
            platform: BenchmarkPlatformV1 {
                host_os: "linux".to_owned(),
                host_arch: "x86_64".to_owned(),
                execution_kind: "native_process".to_owned(),
                ci_environment: Some("github_actions".to_owned()),
            },
            runtime_probe: None,
            samples_ns: vec![10, 20, 30, 40, 50],
            median_ns: 30,
            result_digest: EXPECTED_WORKLOAD_DIGEST.to_owned(),
            correctness_status: EvidenceStatus::Pass,
        };
        let mut envelope = BenchmarkEnvelopeV1::new(receipt).expect("envelope");
        envelope.receipt.samples_ns[0] = 11;
        assert!(envelope.validate().is_err());
    }
}
