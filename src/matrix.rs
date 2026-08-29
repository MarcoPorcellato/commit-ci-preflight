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

//! Version 2 multi-runtime receipt contract.
//!
//! V2 deliberately composes strict v1 runtime receipts instead of widening a
//! v1 document. A check is therefore inseparable from the named, pinned
//! runtime that executed it, while v1 inputs and historical evidence retain
//! their exact parser and verifier behaviour.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::cache::ManagedCache;
use crate::config::ConfigError;
use crate::process::{CancellationToken, SupervisorPort};
use crate::receipt::{
    EvidenceStatus, ProducerEvidence, ReceiptError, RunEvidence, canonical_digest,
};
use crate::run::{
    Clock, CompletionBarrier, NoopCompletionBarrier, NoopRunLifecycleObserver, RunError,
    RunRequest, execute_local_receipt_with_barrier_and_lifecycle,
    write_canonical_receipt_bytes_atomic,
};
use crate::runtime::runtime_for;
use crate::source_snapshot::SourceSnapshot;
use crate::verify::VerificationReportV1;

pub use ccp_core::matrix::{
    MATRIX_CONFIG_SCHEMA_VERSION, MATRIX_POLICY_SCHEMA_VERSION, MATRIX_RECEIPT_SCHEMA_VERSION,
    MatrixCheckConfigV2, MatrixConfigV2, MatrixEnvironmentConfigV2, MatrixPlanEnvelopeV2,
    MatrixPlanProfile, MatrixPlanV2, MatrixReceiptEnvelopeV2, MatrixReceiptV2,
    MatrixRequiredCheckV2, MatrixRuntimeConfigV2, MatrixRuntimePlanV2, MatrixRuntimePolicyV2,
    MatrixRuntimeReceiptV2, MatrixVerificationPolicyV2,
};

pub fn build_matrix_plan(
    config: MatrixConfigV2,
    profile: MatrixPlanProfile,
) -> Result<MatrixPlanEnvelopeV2, MatrixError> {
    ccp_core::matrix::build_matrix_plan_with_profile(config, profile).map_err(MatrixError::from)
}

pub fn prepare_source_snapshot_overlay(
    envelope: &MatrixPlanEnvelopeV2,
    snapshot: &mut SourceSnapshot,
) -> Result<(), MatrixError> {
    envelope
        .validate_profile_binding()
        .map_err(MatrixError::from)?;
    let runtime_envelopes = envelope.runtime_envelopes().map_err(MatrixError::from)?;
    let (_, first) = runtime_envelopes
        .first()
        .ok_or(MatrixError::InvalidReceipt)?;
    let mut overlay = first.clone();
    overlay.plan.checks.clear();
    for (_, runtime) in runtime_envelopes {
        if runtime.plan.caches != overlay.plan.caches
            || runtime.plan.environment != overlay.plan.environment
            || runtime.plan.storage != overlay.plan.storage
            || runtime.fixed_environment != overlay.fixed_environment
        {
            return Err(MatrixError::PlanDigestMismatch);
        }
        overlay.plan.checks.extend(runtime.plan.checks);
    }
    snapshot
        .prepare_mount_overlay(&overlay)
        .map_err(RunError::SourceSnapshot)
        .map_err(MatrixError::Run)
}

pub struct MatrixRunOutcomeV2 {
    pub receipt: MatrixReceiptEnvelopeV2,
    pub receipt_path: PathBuf,
}

/// Unsealed, unpublished Matrix receipt material collected while the caller
/// owns admission and source-snapshot lifecycle. It must be revalidated and
/// sealed only after terminal admission finalization and snapshot cleanup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixRunMaterialV2 {
    receipt: MatrixReceiptV2,
}

/// Inputs that are fixed for the complete matrix execution. Grouping these
/// immutable values keeps the executor below Clippy's argument-count limit
/// without obscuring the owned admission/watchdog dependencies.
pub struct MatrixRunRequestV2<'a> {
    pub envelope: &'a MatrixPlanEnvelopeV2,
    pub repository: &'a Path,
    pub cache: &'a ManagedCache,
    pub generation: u64,
    pub source_snapshot: &'a SourceSnapshot,
}

/// Execute every independently pinned runtime sequentially under one caller
/// owned admission/watchdog session and return unsealed receipt material. The
/// caller must finalize admission, clean the source snapshot, then seal and
/// publish the outer receipt. The inner v1 receipts stay in memory: no
/// intermediate source-tree mutation can make a later runtime observe a
/// different Git state.
pub fn execute_matrix_run_v2(
    request: &MatrixRunRequestV2<'_>,
    supervisor: &dyn SupervisorPort,
    cancellation: &CancellationToken,
    clock: &dyn Clock,
    barrier: &mut dyn CompletionBarrier,
) -> Result<MatrixRunMaterialV2, MatrixError> {
    request.envelope.validate_profile_binding()?;
    let started_at_utc = clock.now_utc().map_err(MatrixError::Run)?;
    let mut runtime_receipts = Vec::new();
    let mut checks = Vec::new();
    for (runtime_id, runtime_envelope) in request.envelope.runtime_envelopes()? {
        request.envelope.validate_profile_binding()?;
        let expected_configuration_digest = request
            .envelope
            .runtime_configuration_digest(&runtime_id)?
            .to_owned();
        if runtime_envelope.plan_digest != expected_configuration_digest {
            return Err(MatrixError::PlanDigestMismatch);
        }
        let runtime =
            runtime_for(runtime_envelope.plan.runtime.kind).map_err(MatrixError::Runtime)?;
        let mut inner_barrier = NoopCompletionBarrier;
        let mut lifecycle = NoopRunLifecycleObserver;
        let receipt = execute_local_receipt_with_barrier_and_lifecycle(
            &RunRequest {
                envelope: &runtime_envelope,
                repository: request.repository,
                cache: request.cache,
                producer_version: request.envelope.profile().producer_version(),
                generation: request.generation,
                source_snapshot: Some(request.source_snapshot),
            },
            runtime.as_ref(),
            supervisor,
            cancellation,
            clock,
            &mut inner_barrier,
            &mut lifecycle,
        )
        .map_err(MatrixError::Run)?;
        checks.extend(receipt.receipt.checks.iter().cloned());
        runtime_receipts.push(MatrixRuntimeReceiptV2 {
            runtime_id,
            receipt,
        });
    }
    barrier.finalize(&checks).map_err(MatrixError::Run)?;
    let first = runtime_receipts
        .first()
        .ok_or(MatrixError::InvalidReceipt)?;
    request.envelope.validate_profile_binding()?;
    let configuration_digest = request.envelope.plan_digest()?.to_owned();
    let repository_evidence = first.receipt.receipt.repository.clone();
    let redaction_policy_version = first.receipt.receipt.redaction_policy_version.clone();
    let statuses: Vec<_> = checks
        .iter()
        .filter(|check| check.required)
        .map(|check| check.status)
        .collect();
    let overall_status = derive_status(&statuses);
    let finished_at_utc = clock.now_utc().map_err(MatrixError::Run)?;
    let run_id = canonical_digest(&MatrixRunIdInput {
        schema_version: MATRIX_RECEIPT_SCHEMA_VERSION,
        project: &request.envelope.plan.project,
        commit: &repository_evidence.commit_sha,
        configuration_digest: &configuration_digest,
        generation: request.generation,
        started_at_utc: &started_at_utc,
    })
    .map_err(MatrixError::Receipt)?;
    let receipt = MatrixReceiptV2 {
        schema_version: MATRIX_RECEIPT_SCHEMA_VERSION.to_owned(),
        producer: first.receipt.receipt.producer.clone(),
        repository: repository_evidence,
        run: RunEvidence {
            run_id,
            generation: request.generation,
            started_at_utc,
            finished_at_utc,
        },
        configuration_digest,
        runtime_receipts,
        overall_status,
        incomplete_reason: (overall_status == EvidenceStatus::Pending)
            .then(|| "one or more required checks were not run".to_owned()),
        redaction_policy_version,
    };
    Ok(MatrixRunMaterialV2 { receipt })
}

/// Revalidate the selected profile and every inner receipt immediately before
/// sealing the outer Matrix receipt.
pub fn seal_matrix_run_material(
    envelope: &MatrixPlanEnvelopeV2,
    material: MatrixRunMaterialV2,
) -> Result<MatrixReceiptEnvelopeV2, MatrixError> {
    validate_matrix_receipts_for_seal(envelope, &material.receipt.runtime_receipts)?;
    MatrixReceiptEnvelopeV2::seal(material.receipt).map_err(MatrixError::from)
}

/// Atomically publish a previously sealed Matrix receipt. Callers must invoke
/// this only after caller-owned terminal and source-snapshot lifecycle steps.
pub fn write_matrix_receipt(
    repository: &Path,
    output: &str,
    receipt: &MatrixReceiptEnvelopeV2,
) -> Result<PathBuf, MatrixError> {
    let bytes = receipt.canonical_bytes()?;
    write_canonical_receipt_bytes_atomic(repository, output, &bytes).map_err(MatrixError::Run)
}

fn validate_matrix_receipts_for_seal(
    envelope: &MatrixPlanEnvelopeV2,
    runtime_receipts: &[MatrixRuntimeReceiptV2],
) -> Result<ProducerEvidence, MatrixError> {
    envelope.validate_profile_binding()?;
    let expected_producer = ProducerEvidence {
        name: env!("CARGO_PKG_NAME").to_owned(),
        version: envelope.profile().producer_version().to_owned(),
    };
    let first = runtime_receipts
        .first()
        .ok_or(MatrixError::InvalidReceipt)?;
    if first.receipt.receipt.producer != expected_producer {
        return Err(MatrixError::InvalidReceipt);
    }
    for runtime in runtime_receipts {
        let expected_configuration_digest =
            envelope.runtime_configuration_digest(&runtime.runtime_id)?;
        if runtime.receipt.receipt.producer != first.receipt.receipt.producer
            || runtime.receipt.receipt.producer != expected_producer
            || runtime.receipt.receipt.configuration_digest != expected_configuration_digest
        {
            return Err(MatrixError::InvalidReceipt);
        }
    }
    Ok(expected_producer)
}

fn derive_status(required: &[EvidenceStatus]) -> EvidenceStatus {
    if required.contains(&EvidenceStatus::Fail) {
        EvidenceStatus::Fail
    } else if required
        .iter()
        .any(|status| matches!(status, EvidenceStatus::Pending | EvidenceStatus::NotRun))
    {
        EvidenceStatus::Pending
    } else {
        EvidenceStatus::Pass
    }
}

#[derive(Serialize)]
struct MatrixRunIdInput<'a> {
    schema_version: &'static str,
    project: &'a str,
    commit: &'a str,
    configuration_digest: &'a str,
    generation: u64,
    started_at_utc: &'a str,
}

pub fn matrix_config_schema_json() -> Result<String, MatrixError> {
    Ok(include_str!("../schema/config-v2.schema.json").to_owned())
}

pub fn matrix_receipt_schema_json() -> Result<String, MatrixError> {
    Ok(include_str!("../schema/receipt-v2.schema.json").to_owned())
}

pub fn matrix_policy_schema_json() -> Result<String, MatrixError> {
    Ok(include_str!("../schema/policy-v2.schema.json").to_owned())
}

pub fn verify_matrix_receipt_document(
    bytes: &[u8],
    policy: &MatrixVerificationPolicyV2,
    expected_commit: &str,
    evaluated_at_utc: &str,
) -> Result<VerificationReportV1, MatrixError> {
    ccp_core::matrix::verify_matrix_receipt_document(
        bytes,
        policy,
        expected_commit,
        evaluated_at_utc,
    )
    .map_err(MatrixError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    use crate::cache::{CacheRootOptions, PlatformFamily, ResolvedCacheRoot};
    use crate::process::{
        CapturedStream, CleanupStatus, ExitOutcome, GenerationGuard, ProcessError, ProcessRequest,
        ProcessResult, ProcessTermination, RunIdentity,
    };
    use crate::receipt::{
        CheckEvidence, PlatformEvidence, ReceiptEnvelopeV1, ReceiptV1, RepositoryEvidence,
    };
    use crate::run::SystemClock;
    use crate::source_snapshot::SourceSnapshot;

    const IMAGE_311: &str = "example.invalid/python311@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const IMAGE_312: &str = "example.invalid/python312@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const OUTPUT_DIGEST: &str =
        "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    #[test]
    fn matrix_fixture_root_is_nested_under_selected_test_root() {
        assert_eq!(
            matrix_fixture_root(Path::new("/private-test-root"), "snapshot", 7),
            PathBuf::from("/private-test-root/.ccp-matrix-snapshot-7")
        );
    }

    fn matrix_fixture_root(base: &Path, name: &str, process_id: u32) -> PathBuf {
        base.join(format!(".ccp-matrix-{name}-{process_id}"))
    }

    fn matrix_test_root(name: &str) -> PathBuf {
        let base = std::env::var_os("CCP_TEST_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .expect("current directory")
                    .parent()
                    .expect("repository parent")
                    .to_path_buf()
            });
        matrix_fixture_root(&base, name, std::process::id())
    }

    fn envelope(profile: MatrixPlanProfile) -> MatrixPlanEnvelopeV2 {
        build_matrix_plan(
            MatrixConfigV2::parse(&format!(
                r#"
schema_version = "2.0"
project = "owner/repository"

[receipt]
output = ".ccp/receipt.json"

[[checks]]
id = "python311-check"
runtime_id = "python311"
required = true
argv = ["python", "-V"]
working_directory = "."
timeout_seconds = 60

[[checks]]
id = "python312-check"
runtime_id = "python312"
required = true
argv = ["python", "-V"]
working_directory = "."
timeout_seconds = 60

[[runtimes]]
id = "python311"
kind = "docker_compatible"
image = "{IMAGE_311}"
cpu_count = 1
memory_mib = 128
pids_limit = 16

[[runtimes]]
id = "python312"
kind = "docker_compatible"
image = "{IMAGE_312}"
cpu_count = 1
memory_mib = 128
pids_limit = 16
"#
            ))
            .expect("matrix config"),
            profile,
        )
        .expect("matrix plan")
    }

    fn synthetic_runtime_receipts(envelope: &MatrixPlanEnvelopeV2) -> Vec<MatrixRuntimeReceiptV2> {
        let producer = ProducerEvidence {
            name: env!("CARGO_PKG_NAME").to_owned(),
            version: envelope.profile().producer_version().to_owned(),
        };
        envelope
            .plan
            .runtimes
            .iter()
            .map(|runtime| {
                let image_digest = runtime
                    .runtime
                    .image
                    .rsplit_once('@')
                    .expect("pinned image")
                    .1
                    .to_owned();
                let receipt = ReceiptEnvelopeV1::seal(ReceiptV1 {
                    schema_version: crate::receipt::RECEIPT_SCHEMA_VERSION.to_owned(),
                    producer: producer.clone(),
                    repository: RepositoryEvidence {
                        repository: envelope.plan.project.clone(),
                        commit_sha: "a".repeat(40),
                        dirty: false,
                    },
                    run: RunEvidence {
                        run_id: format!("run-{}", runtime.id),
                        generation: 7,
                        started_at_utc: "2026-08-25T01:00:00Z".to_owned(),
                        finished_at_utc: "2026-08-25T01:00:01Z".to_owned(),
                    },
                    platform: PlatformEvidence {
                        host_os: "macos".to_owned(),
                        host_arch: "aarch64".to_owned(),
                        runtime_kind: "docker_compatible".to_owned(),
                        runtime_version: "test".to_owned(),
                        image_reference: runtime.runtime.image.clone(),
                        image_digest,
                    },
                    configuration_digest: envelope
                        .runtime_configuration_digest(&runtime.id)
                        .expect("runtime digest")
                        .to_owned(),
                    checks: vec![CheckEvidence {
                        id: format!("{}-check", runtime.id),
                        required: true,
                        argv: vec!["python".to_owned(), "-V".to_owned()],
                        working_directory: ".".to_owned(),
                        status: EvidenceStatus::Pass,
                        exit_code: Some(0),
                        duration_ms: 1,
                        timed_out: false,
                        cancelled: false,
                        output_digest: Some(OUTPUT_DIGEST.to_owned()),
                        incomplete_reason: None,
                    }],
                    overall_status: EvidenceStatus::Pass,
                    incomplete_reason: None,
                    redaction_policy_version: "ccp-redaction-v1".to_owned(),
                })
                .expect("seal inner receipt");
                MatrixRuntimeReceiptV2 {
                    runtime_id: runtime.id.clone(),
                    receipt,
                }
            })
            .collect()
    }

    fn reseal(runtime: &mut MatrixRuntimeReceiptV2) {
        let inner = runtime.receipt.receipt.clone();
        runtime.receipt = ReceiptEnvelopeV1::seal(inner).expect("reseal inner receipt");
    }

    #[test]
    fn preseal_receipt_validation_accepts_current_and_legacy_profile_provenance() {
        for profile in [MatrixPlanProfile::CurrentV2, MatrixPlanProfile::LegacyV1] {
            let envelope = envelope(profile);
            let receipts = synthetic_runtime_receipts(&envelope);

            let producer = validate_matrix_receipts_for_seal(&envelope, &receipts)
                .expect("profile-bound inner receipts");
            assert_eq!(producer.version, profile.producer_version());
        }
    }

    #[test]
    fn preseal_receipt_validation_rejects_mutated_producer_and_runtime_digest() {
        let envelope = envelope(MatrixPlanProfile::LegacyV1);

        let mut mixed_producer = synthetic_runtime_receipts(&envelope);
        mixed_producer[1].receipt.receipt.producer.version = env!("CARGO_PKG_VERSION").to_owned();
        reseal(&mut mixed_producer[1]);
        assert!(matches!(
            validate_matrix_receipts_for_seal(&envelope, &mixed_producer),
            Err(MatrixError::InvalidReceipt)
        ));

        let mut changed_digest = synthetic_runtime_receipts(&envelope);
        changed_digest[1].receipt.receipt.configuration_digest = OUTPUT_DIGEST.to_owned();
        reseal(&mut changed_digest[1]);
        assert!(matches!(
            validate_matrix_receipts_for_seal(&envelope, &changed_digest),
            Err(MatrixError::InvalidReceipt)
        ));
    }

    #[derive(Default)]
    struct CountingSupervisor {
        calls: AtomicUsize,
    }

    impl SupervisorPort for CountingSupervisor {
        fn execute(
            &self,
            _request: &ProcessRequest,
            _cancellation: &CancellationToken,
            _generation: &GenerationGuard,
        ) -> Result<ProcessResult, ProcessError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            panic!("tampered Matrix plan must fail before supervisor execution")
        }
    }

    struct SnapshotMatrixSupervisor {
        repository: PathBuf,
        execution_roots: Mutex<Vec<PathBuf>>,
        containers: Mutex<BTreeMap<String, BTreeMap<String, String>>>,
        removals: AtomicU64,
    }

    impl SnapshotMatrixSupervisor {
        fn new(repository: PathBuf) -> Self {
            Self {
                repository,
                execution_roots: Mutex::new(Vec::new()),
                containers: Mutex::new(BTreeMap::new()),
                removals: AtomicU64::new(0),
            }
        }

        fn completed(request: &ProcessRequest, stdout: Vec<u8>, success: bool) -> ProcessResult {
            ProcessResult {
                identity: request.identity.clone(),
                termination: ProcessTermination::Completed,
                cleanup: CleanupStatus::Verified,
                exit: Some(ExitOutcome {
                    success,
                    code: Some(if success { 0 } else { 1 }),
                }),
                stdout: CapturedStream::from_captured(stdout, false),
                stderr: CapturedStream::from_captured(Vec::new(), false),
                elapsed_millis: 1,
            }
        }

        fn workspace_source(request: &ProcessRequest) -> Option<PathBuf> {
            request.argv.iter().find_map(|argument| {
                let argument = argument.to_str()?;
                let source = argument.strip_prefix("type=bind,src=")?;
                let (source, target) = source.split_once(",dst=")?;
                (target == "/workspace,readonly").then(|| PathBuf::from(source))
            })
        }
    }

    impl SupervisorPort for SnapshotMatrixSupervisor {
        fn execute(
            &self,
            request: &ProcessRequest,
            _cancellation: &CancellationToken,
            generation: &GenerationGuard,
        ) -> Result<ProcessResult, ProcessError> {
            generation.ensure_current(&request.identity)?;
            if request.program == "git" {
                let command = request.argv.first().and_then(|argument| argument.to_str());
                let stdout = match command {
                    Some("ls-tree") => {
                        format!("100644 blob {}\tREADME.md\0", "c".repeat(40)).into_bytes()
                    }
                    Some("cat-file")
                        if request.argv.get(1).is_some_and(|argument| argument == "-s") =>
                    {
                        b"16\n".to_vec()
                    }
                    Some("cat-file") => b"snapshot source\n".to_vec(),
                    Some("hash-object") => format!("{}\n", "c".repeat(40)).into_bytes(),
                    Some("status") => Vec::new(),
                    Some("rev-parse") => format!("{}\n", "a".repeat(40)).into_bytes(),
                    _ => Vec::new(),
                };
                return Ok(Self::completed(request, stdout, true));
            }

            let command = request.argv.first().and_then(|argument| argument.to_str());
            match command {
                Some("info") => Ok(Self::completed(
                    request,
                    br#"{"ServerVersion":"29.4.0","OperatingSystem":"fixture","OSType":"linux","Name":"private"}"#.to_vec(),
                    true,
                )),
                Some("create") => {
                    let source = Self::workspace_source(request).expect("read-only workspace mount");
                    assert_eq!(
                        fs::read(source.join("README.md")).expect("snapshot source"),
                        b"snapshot source\n"
                    );
                    assert!(
                        !source.join("ignored-live.txt").exists(),
                        "live-only mutation leaked into snapshot execution root"
                    );
                    self.execution_roots.lock().expect("execution roots").push(source);
                    let mut name = None;
                    let mut labels = BTreeMap::new();
                    let mut arguments = request.argv.iter();
                    while let Some(argument) = arguments.next() {
                        match argument.to_str() {
                            Some("--name") => name = arguments.next().and_then(|value| value.to_str()),
                            Some("--label") => {
                                if let Some((key, value)) = arguments
                                    .next()
                                    .and_then(|value| value.to_str())
                                    .and_then(|value| value.split_once('='))
                                {
                                    labels.insert(key.to_owned(), value.to_owned());
                                }
                            }
                            _ => {}
                        }
                    }
                    self.containers
                        .lock()
                        .expect("containers")
                        .insert(name.expect("container name").to_owned(), labels);
                    Ok(Self::completed(
                        request,
                        format!("{}\n", "d".repeat(64)).into_bytes(),
                        true,
                    ))
                }
                Some("inspect") => {
                    let name = request
                        .argv
                        .last()
                        .and_then(|argument| argument.to_str())
                        .expect("container name");
                    if let Some(labels) = self
                        .containers
                        .lock()
                        .expect("containers")
                        .get(name)
                        .cloned()
                    {
                        Ok(Self::completed(
                            request,
                            serde_json::to_vec(&serde_json::json!({
                                "Name": format!("/{name}"),
                                "Config": {"Labels": labels},
                                "State": {"Status": "running"},
                            }))
                            .expect("inspect JSON"),
                            true,
                        ))
                    } else {
                        Ok(Self::completed(request, b"No such container\n".to_vec(), false))
                    }
                }
                Some("rm") => {
                    let name = request
                        .argv
                        .last()
                        .and_then(|argument| argument.to_str())
                        .expect("container name");
                    self.containers.lock().expect("containers").remove(name);
                    if self.removals.fetch_add(1, Ordering::SeqCst) == 0 {
                        fs::write(self.repository.join("ignored-live.txt"), b"live mutation\n")
                            .expect("mutate live repository between runtimes");
                    }
                    Ok(Self::completed(request, Vec::new(), true))
                }
                Some("wait") => Ok(Self::completed(request, b"0\n".to_vec(), true)),
                Some("attach" | "start" | "stop" | "kill") => {
                    Ok(Self::completed(request, b"fixture\n".to_vec(), true))
                }
                _ => Ok(Self::completed(request, b"fixture\n".to_vec(), true)),
            }
        }
    }

    #[test]
    fn matrix_runtimes_share_one_immutable_source_snapshot() {
        let root = matrix_test_root("source-snapshot");
        if root.exists() {
            fs::remove_dir_all(&root).expect("clean fixture root");
        }
        let repository = root.join("repository");
        fs::create_dir_all(&repository).expect("repository root");
        fs::write(repository.join("README.md"), b"live source\n").expect("live source");
        let cache = ManagedCache::initialize(
            ResolvedCacheRoot::resolve(
                &repository,
                &CacheRootOptions {
                    explicit: Some(root.join("cache")),
                    environment: None,
                    home: None,
                    xdg_cache_home: None,
                    local_app_data: None,
                    platform: PlatformFamily::Unix,
                },
            )
            .expect("cache root"),
        )
        .expect("cache");
        let envelope = envelope(MatrixPlanProfile::LegacyV1);
        let supervisor = SnapshotMatrixSupervisor::new(repository.clone());
        let commit = "a".repeat(40);
        let identity = RunIdentity {
            project: envelope.plan.project.clone(),
            commit: Some(commit.clone()),
            config_digest: envelope.plan_digest.clone(),
            generation: "7".to_owned(),
        };
        let generation = GenerationGuard::new(identity.clone());
        let mut snapshot = SourceSnapshot::materialize(
            &repository,
            &commit,
            &root.join("source-snapshot"),
            &supervisor,
            &CancellationToken::default(),
            &generation,
            &identity,
        )
        .expect("source snapshot");
        prepare_source_snapshot_overlay(&envelope, &mut snapshot).expect("matrix snapshot overlay");
        let mut barrier = NoopCompletionBarrier;

        let outcome = execute_matrix_run_v2(
            &MatrixRunRequestV2 {
                envelope: &envelope,
                repository: &repository,
                cache: &cache,
                generation: 7,
                source_snapshot: &snapshot,
            },
            &supervisor,
            &CancellationToken::default(),
            &SystemClock,
            &mut barrier,
        )
        .expect("matrix run");

        assert_eq!(outcome.receipt.runtime_receipts.len(), 2);
        assert!(
            !repository.join(&envelope.plan.receipt.output).exists(),
            "matrix execution must return unsealed, unpublished material"
        );
        let roots = supervisor.execution_roots.lock().expect("execution roots");
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0], roots[1]);
        assert_eq!(roots[0], snapshot.root());
        assert!(repository.join("ignored-live.txt").is_file());
        assert!(!snapshot.root().join("ignored-live.txt").exists());
        drop(roots);
        snapshot.cleanup().expect("snapshot cleanup");
        drop(cache);
        fs::remove_dir_all(root).expect("remove fixture root");
    }

    #[test]
    fn executor_rejects_tampered_legacy_plan_before_supervisor_execution() {
        let root = matrix_test_root("provenance-test");
        if root.exists() {
            fs::remove_dir_all(&root).expect("clean fixture root");
        }
        let repository = root.join("repository");
        fs::create_dir_all(&repository).expect("repository root");
        let cache = ManagedCache::initialize(
            ResolvedCacheRoot::resolve(
                &repository,
                &CacheRootOptions {
                    explicit: Some(root.join("cache")),
                    environment: None,
                    home: None,
                    xdg_cache_home: None,
                    local_app_data: None,
                    platform: PlatformFamily::Unix,
                },
            )
            .expect("cache root"),
        )
        .expect("cache");
        let mut envelope = envelope(MatrixPlanProfile::LegacyV1);
        let snapshot_supervisor = SnapshotMatrixSupervisor::new(repository.clone());
        let commit = "a".repeat(40);
        let identity = RunIdentity {
            project: envelope.plan.project.clone(),
            commit: Some(commit.clone()),
            config_digest: envelope.plan_digest.clone(),
            generation: "7".to_owned(),
        };
        let generation = GenerationGuard::new(identity.clone());
        let mut snapshot = SourceSnapshot::materialize(
            &repository,
            &commit,
            &root.join("source-snapshot"),
            &snapshot_supervisor,
            &CancellationToken::default(),
            &generation,
            &identity,
        )
        .expect("source snapshot");
        prepare_source_snapshot_overlay(&envelope, &mut snapshot).expect("matrix snapshot overlay");
        envelope.plan.project = "owner/tampered".to_owned();
        let supervisor = CountingSupervisor::default();
        let mut barrier = NoopCompletionBarrier;

        let result = execute_matrix_run_v2(
            &MatrixRunRequestV2 {
                envelope: &envelope,
                repository: &repository,
                cache: &cache,
                generation: 7,
                source_snapshot: &snapshot,
            },
            &supervisor,
            &CancellationToken::default(),
            &SystemClock,
            &mut barrier,
        );

        assert!(matches!(result, Err(MatrixError::PlanDigestMismatch)));
        assert_eq!(supervisor.calls.load(Ordering::SeqCst), 0);
        snapshot.cleanup().expect("snapshot cleanup");
        drop(cache);
        fs::remove_dir_all(root).expect("remove fixture root");
    }
}

#[derive(Debug)]
pub enum MatrixError {
    Io(std::io::Error),
    Parse(toml::de::Error),
    Json(serde_json::Error),
    Config(ConfigError),
    Receipt(ReceiptError),
    Policy(String),
    Verification(crate::verify::VerificationError),
    Runtime(crate::runtime::RuntimeError),
    Run(RunError),
    UnsupportedSchemaVersion(String),
    ConfigTooLarge,
    InvalidField(&'static str),
    DuplicateValue(&'static str),
    UnknownRuntime(String),
    RuntimeWithoutRequiredCheck(String),
    CrossRuntimeDependency { check: String, dependency: String },
    LegacyPlanNotRepresentable(&'static str),
    PlanDigestMismatch,
    ReceiptIdMismatch,
    InvalidReceipt,
    InvalidEvaluationTime,
}

impl fmt::Display for MatrixError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "matrix I/O failed: {error}"),
            Self::Parse(error) => write!(formatter, "matrix configuration parse failed: {error}"),
            Self::Json(error) => write!(formatter, "matrix JSON serialization failed: {error}"),
            Self::Config(error) => write!(formatter, "matrix configuration invalid: {error}"),
            Self::Receipt(error) => write!(formatter, "matrix receipt invalid: {error}"),
            Self::Policy(error) => write!(formatter, "matrix policy invalid: {error}"),
            Self::Verification(error) => write!(formatter, "matrix verification invalid: {error}"),
            Self::Runtime(error) => write!(formatter, "matrix runtime invalid: {error}"),
            Self::Run(error) => write!(formatter, "matrix run failed: {error}"),
            Self::UnsupportedSchemaVersion(version) => {
                write!(formatter, "unsupported matrix schema version: {version}")
            }
            Self::ConfigTooLarge => write!(
                formatter,
                "matrix configuration exceeds the bounded input size"
            ),
            Self::InvalidField(field) => write!(formatter, "invalid matrix field: {field}"),
            Self::DuplicateValue(field) => write!(formatter, "duplicate matrix value: {field}"),
            Self::UnknownRuntime(id) => {
                write!(formatter, "matrix check references unknown runtime: {id}")
            }
            Self::RuntimeWithoutRequiredCheck(id) => {
                write!(formatter, "matrix runtime has no required check: {id}")
            }
            Self::CrossRuntimeDependency { check, dependency } => write!(
                formatter,
                "matrix cross-runtime dependency is unsupported: {check} -> {dependency}"
            ),
            Self::LegacyPlanNotRepresentable(field) => {
                write!(formatter, "matrix legacy plan cannot represent: {field}")
            }
            Self::PlanDigestMismatch => write!(formatter, "matrix plan digest mismatch"),
            Self::ReceiptIdMismatch => write!(formatter, "matrix receipt identifier mismatch"),
            Self::InvalidReceipt => {
                write!(formatter, "matrix receipt violates semantic invariants")
            }
            Self::InvalidEvaluationTime => write!(formatter, "matrix evaluation time is invalid"),
        }
    }
}

impl std::error::Error for MatrixError {}

impl From<ccp_core::matrix::MatrixContractError> for MatrixError {
    fn from(error: ccp_core::matrix::MatrixContractError) -> Self {
        use ccp_core::matrix::MatrixContractError as Core;
        match error {
            Core::Io(error) => Self::Io(error),
            Core::Json(error) => Self::Json(error),
            Core::Parse(error) => Self::Parse(error),
            Core::Config(error) => Self::Config(error),
            Core::Receipt(error) => Self::Receipt(error),
            Core::Verification(error) => Self::Verification(error),
            Core::UnsupportedSchemaVersion(value) => Self::UnsupportedSchemaVersion(value),
            Core::ConfigTooLarge => Self::ConfigTooLarge,
            Core::InvalidField(value) => Self::InvalidField(value),
            Core::DuplicateValue(value) => Self::DuplicateValue(value),
            Core::UnknownRuntime(value) => Self::UnknownRuntime(value),
            Core::RuntimeWithoutRequiredCheck(value) => Self::RuntimeWithoutRequiredCheck(value),
            Core::CrossRuntimeDependency { check, dependency } => {
                Self::CrossRuntimeDependency { check, dependency }
            }
            Core::LegacyPlanNotRepresentable(field) => Self::LegacyPlanNotRepresentable(field),
            Core::PlanDigestMismatch => Self::PlanDigestMismatch,
            Core::InvalidReceipt => Self::InvalidReceipt,
            Core::ReceiptIdMismatch => Self::ReceiptIdMismatch,
            Core::InvalidEvaluationTime => Self::InvalidEvaluationTime,
        }
    }
}
