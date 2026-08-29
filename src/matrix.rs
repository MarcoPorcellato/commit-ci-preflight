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

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};

use crate::cache::ManagedCache;
use crate::config::{
    CacheConfig, CheckConfig, ConfigError, ConfigV1, EnvironmentConfig, ExecutionPlanEnvelopeV1,
    ExecutionPlanV1, NormalizedCache, NormalizedEnvironment, NormalizedReceipt, NormalizedRuntime,
    ReceiptConfig, RuntimeConfig, RuntimeKind, validate_identifier,
};
use crate::matrix_legacy::{LegacyMatrixDigestBasisV1, project_legacy_basis};
use crate::process::{CancellationToken, SupervisorPort};
use crate::receipt::{
    EvidenceStatus, ProducerEvidence, ReceiptEnvelopeV1, ReceiptError, ReceiptV1,
    RepositoryEvidence, RunEvidence, canonical_digest, canonical_json,
};
use crate::run::{
    Clock, CompletionBarrier, NoopCompletionBarrier, NoopRunLifecycleObserver, RunError,
    RunRequest, execute_local_receipt_with_barrier_and_lifecycle,
    write_canonical_receipt_bytes_atomic,
};
use crate::runtime::runtime_for;
use crate::source_snapshot::SourceSnapshot;
use crate::verify::{
    AcceptedPlatformV1, VerificationDecision, VerificationFindingV1, VerificationPolicyV1,
    VerificationReportV1, VerificationStatus, finding, parse_utc_seconds, validate_commit,
};

pub const MATRIX_CONFIG_SCHEMA_VERSION: &str = "2.0";
pub const MATRIX_RECEIPT_SCHEMA_VERSION: &str = "2.0";
pub const MATRIX_POLICY_SCHEMA_VERSION: &str = "2.0";
const MAX_MATRIX_RUNTIMES: usize = 32;
const MAX_MATRIX_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatrixConfigV2 {
    pub schema_version: String,
    pub project: String,
    pub runtimes: Vec<MatrixRuntimeConfigV2>,
    #[serde(default)]
    pub receipt: ReceiptConfig,
    #[serde(default)]
    pub environment: MatrixEnvironmentConfigV2,
    #[serde(default)]
    pub caches: Vec<CacheConfig>,
    pub checks: Vec<MatrixCheckConfigV2>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(rename = "EnvironmentConfig")]
pub struct MatrixEnvironmentConfigV2 {
    pub allow: Vec<String>,
}

impl MatrixEnvironmentConfigV2 {
    fn as_v1(&self) -> EnvironmentConfig {
        EnvironmentConfig {
            allow: self.allow.clone(),
            ..EnvironmentConfig::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatrixRuntimeConfigV2 {
    pub id: String,
    pub kind: RuntimeKind,
    pub image: String,
    pub cpu_count: u16,
    pub memory_mib: u64,
    pub pids_limit: u32,
    #[serde(default)]
    pub network: bool,
}

impl MatrixRuntimeConfigV2 {
    fn as_runtime(&self) -> RuntimeConfig {
        RuntimeConfig {
            kind: self.kind,
            image: self.image.clone(),
            cpu_count: self.cpu_count,
            memory_mib: self.memory_mib,
            pids_limit: self.pids_limit,
            network: self.network,
            pull_policy: None,
            swap_mode: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatrixCheckConfigV2 {
    pub id: String,
    pub runtime_id: String,
    pub required: bool,
    pub argv: Vec<String>,
    pub working_directory: String,
    pub timeout_seconds: u64,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
}

impl MatrixCheckConfigV2 {
    fn as_v1(&self) -> CheckConfig {
        CheckConfig {
            id: self.id.clone(),
            required: self.required,
            argv: self.argv.clone(),
            working_directory: self.working_directory.clone(),
            timeout_seconds: self.timeout_seconds,
            depends_on: self.depends_on.clone(),
            artifacts: self.artifacts.clone(),
            artifact_contracts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MatrixPlanEnvelopeV2 {
    pub plan_digest: String,
    pub plan: MatrixPlanV2,
    #[serde(skip)]
    profile: MatrixPlanProfile,
    #[serde(skip)]
    legacy_basis: Option<LegacyMatrixDigestBasisV1>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MatrixPlanProfile {
    #[default]
    CurrentV2,
    LegacyV1,
}

impl MatrixPlanProfile {
    pub const fn producer_version(self) -> &'static str {
        match self {
            Self::CurrentV2 => env!("CARGO_PKG_VERSION"),
            Self::LegacyV1 => concat!(env!("CARGO_PKG_VERSION"), "+matrix-v2-legacy-v1"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MatrixPlanV2 {
    pub schema_version: String,
    pub project: String,
    pub receipt: NormalizedReceipt,
    pub environment: NormalizedEnvironment,
    pub caches: Vec<NormalizedCache>,
    pub runtimes: Vec<MatrixRuntimePlanV2>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MatrixRuntimePlanV2 {
    pub id: String,
    pub configuration_digest: String,
    pub runtime: NormalizedRuntime,
    pub checks: Vec<crate::config::NormalizedCheck>,
}

impl MatrixConfigV2 {
    pub fn parse(input: &str) -> Result<Self, MatrixError> {
        if input.len() > MAX_MATRIX_BYTES {
            return Err(MatrixError::ConfigTooLarge);
        }
        toml::from_str(input).map_err(MatrixError::Parse)
    }

    pub fn load(path: &Path) -> Result<Self, MatrixError> {
        let metadata = fs::metadata(path).map_err(MatrixError::Io)?;
        if metadata.len() > MAX_MATRIX_BYTES as u64 {
            return Err(MatrixError::ConfigTooLarge);
        }
        let source = fs::read_to_string(path).map_err(MatrixError::Io)?;
        Self::parse(&source)
    }

    pub fn into_plan(self) -> Result<MatrixPlanEnvelopeV2, MatrixError> {
        build_matrix_plan(self, MatrixPlanProfile::default())
    }
}

pub fn build_matrix_plan(
    config: MatrixConfigV2,
    profile: MatrixPlanProfile,
) -> Result<MatrixPlanEnvelopeV2, MatrixError> {
    config.build_plan_with_profile(profile)
}

impl MatrixConfigV2 {
    fn build_plan_with_profile(
        self,
        profile: MatrixPlanProfile,
    ) -> Result<MatrixPlanEnvelopeV2, MatrixError> {
        if self.schema_version != MATRIX_CONFIG_SCHEMA_VERSION {
            return Err(MatrixError::UnsupportedSchemaVersion(self.schema_version));
        }
        if !(2..=MAX_MATRIX_RUNTIMES).contains(&self.runtimes.len()) {
            return Err(MatrixError::InvalidField("runtimes"));
        }
        let mut runtime_by_id = BTreeMap::new();
        for runtime in self.runtimes {
            validate_identifier("runtimes.id", &runtime.id).map_err(MatrixError::Config)?;
            if runtime_by_id.insert(runtime.id.clone(), runtime).is_some() {
                return Err(MatrixError::DuplicateValue("runtimes.id"));
            }
        }
        let mut check_runtime = BTreeMap::new();
        let mut checks_by_runtime: BTreeMap<String, Vec<CheckConfig>> = BTreeMap::new();
        for check in &self.checks {
            validate_identifier("checks.runtime_id", &check.runtime_id)
                .map_err(MatrixError::Config)?;
            if !runtime_by_id.contains_key(&check.runtime_id) {
                return Err(MatrixError::UnknownRuntime(check.runtime_id.clone()));
            }
            if check_runtime
                .insert(check.id.clone(), check.runtime_id.clone())
                .is_some()
            {
                return Err(MatrixError::DuplicateValue("checks.id"));
            }
            checks_by_runtime
                .entry(check.runtime_id.clone())
                .or_default()
                .push(check.as_v1());
        }
        for check in &self.checks {
            for dependency in &check.depends_on {
                if let Some(runtime) = check_runtime.get(dependency)
                    && runtime != &check.runtime_id
                {
                    return Err(MatrixError::CrossRuntimeDependency {
                        check: check.id.clone(),
                        dependency: dependency.clone(),
                    });
                }
            }
        }

        let mut runtime_plans = Vec::with_capacity(runtime_by_id.len());
        let mut shared_receipt = None;
        let mut shared_environment = None;
        let mut shared_caches = None;
        for (id, runtime) in runtime_by_id {
            let group_checks = checks_by_runtime.remove(&id).unwrap_or_default();
            if !group_checks.iter().any(|check| check.required) {
                return Err(MatrixError::RuntimeWithoutRequiredCheck(id));
            }
            let group = ConfigV1 {
                schema_version: "1.0".to_owned(),
                project: self.project.clone(),
                runtime: runtime.as_runtime(),
                receipt: self.receipt.clone(),
                environment: self.environment.as_v1(),
                caches: self.caches.clone(),
                storage: None,
                checks: group_checks,
            }
            .into_plan()
            .map_err(MatrixError::Config)?;
            if shared_environment.is_none() {
                shared_receipt = Some(group.plan.receipt.clone());
                shared_environment = Some(group.plan.environment.clone());
                shared_caches = Some(group.plan.caches.clone());
            }
            runtime_plans.push(MatrixRuntimePlanV2 {
                id,
                configuration_digest: group.plan_digest,
                runtime: group.plan.runtime,
                checks: group.plan.checks,
            });
        }
        let mut plan = MatrixPlanV2 {
            schema_version: MATRIX_CONFIG_SCHEMA_VERSION.to_owned(),
            project: self.project,
            receipt: shared_receipt.expect("at least two runtimes"),
            environment: shared_environment.expect("at least two runtimes"),
            caches: shared_caches.expect("at least two runtimes"),
            runtimes: runtime_plans,
        };
        match profile {
            MatrixPlanProfile::CurrentV2 => {
                let plan_digest = canonical_digest(&plan).map_err(MatrixError::Receipt)?;
                Ok(MatrixPlanEnvelopeV2 {
                    plan_digest,
                    plan,
                    profile,
                    legacy_basis: None,
                })
            }
            MatrixPlanProfile::LegacyV1 => {
                let legacy_basis = project_legacy_basis(&plan)?;
                for runtime in &mut plan.runtimes {
                    runtime.configuration_digest =
                        legacy_basis.runtime_digest(&runtime.id)?.to_owned();
                }
                let plan_digest = legacy_basis.outer_digest()?;
                Ok(MatrixPlanEnvelopeV2 {
                    plan_digest,
                    plan,
                    profile,
                    legacy_basis: Some(legacy_basis),
                })
            }
        }
    }
}

impl MatrixPlanEnvelopeV2 {
    pub fn profile(&self) -> MatrixPlanProfile {
        self.profile
    }

    pub fn plan_digest(&self) -> Result<&str, MatrixError> {
        self.validate_profile_binding()?;
        Ok(&self.plan_digest)
    }

    pub fn runtime_configuration_digest(&self, id: &str) -> Result<&str, MatrixError> {
        self.validate_profile_binding()?;
        match self.profile {
            MatrixPlanProfile::LegacyV1 => self
                .legacy_basis
                .as_ref()
                .ok_or(MatrixError::PlanDigestMismatch)?
                .runtime_digest(id),
            MatrixPlanProfile::CurrentV2 => self
                .plan
                .runtimes
                .iter()
                .find(|runtime| runtime.id == id)
                .map(|runtime| runtime.configuration_digest.as_str())
                .ok_or_else(|| MatrixError::UnknownRuntime(id.to_owned())),
        }
    }

    pub fn legacy_digest_basis_value(&self) -> Result<Option<serde_json::Value>, MatrixError> {
        self.validate_profile_binding()?;
        match self.profile {
            MatrixPlanProfile::CurrentV2 => Ok(None),
            MatrixPlanProfile::LegacyV1 => self
                .legacy_basis
                .as_ref()
                .ok_or(MatrixError::PlanDigestMismatch)?
                .report_value()
                .map(Some),
        }
    }

    pub fn validate_profile_binding(&self) -> Result<(), MatrixError> {
        match self.profile {
            MatrixPlanProfile::CurrentV2 => {
                if self.legacy_basis.is_some()
                    || canonical_digest(&self.plan).map_err(MatrixError::Receipt)?
                        != self.plan_digest
                {
                    return Err(MatrixError::PlanDigestMismatch);
                }
            }
            MatrixPlanProfile::LegacyV1 => {
                let basis = self
                    .legacy_basis
                    .as_ref()
                    .ok_or(MatrixError::PlanDigestMismatch)?;
                let projected = project_legacy_basis(&self.plan)?;
                if &projected != basis || projected.outer_digest()? != self.plan_digest {
                    return Err(MatrixError::PlanDigestMismatch);
                }
                for runtime in &self.plan.runtimes {
                    if projected.runtime_digest(&runtime.id)? != runtime.configuration_digest {
                        return Err(MatrixError::PlanDigestMismatch);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, MatrixError> {
        self.validate_profile_binding()?;
        canonical_json(self).map_err(MatrixError::Receipt)
    }

    pub fn runtime_envelopes(&self) -> Result<Vec<(String, ExecutionPlanEnvelopeV1)>, MatrixError> {
        self.validate_profile_binding()?;
        let mut result = Vec::with_capacity(self.plan.runtimes.len());
        for runtime in &self.plan.runtimes {
            let plan = ExecutionPlanV1 {
                schema_version: "1.0".to_owned(),
                project: self.plan.project.clone(),
                runtime: runtime.runtime.clone(),
                receipt: self.plan.receipt.clone(),
                environment: self.plan.environment.clone(),
                caches: self.plan.caches.clone(),
                storage: None,
                checks: runtime.checks.clone(),
            };
            let plan_digest = match self.profile {
                MatrixPlanProfile::CurrentV2 => {
                    let digest = canonical_digest(&plan).map_err(MatrixError::Receipt)?;
                    if digest != runtime.configuration_digest {
                        return Err(MatrixError::PlanDigestMismatch);
                    }
                    digest
                }
                MatrixPlanProfile::LegacyV1 => {
                    self.runtime_configuration_digest(&runtime.id)?.to_owned()
                }
            };
            result.push((
                runtime.id.clone(),
                ExecutionPlanEnvelopeV1 {
                    plan_digest,
                    plan,
                    fixed_environment: BTreeMap::new(),
                },
            ));
        }
        Ok(result)
    }

    /// Prepare the one source-snapshot overlay shared by every Matrix runtime.
    /// Cache, environment, storage, and fixed-environment fields must be
    /// identical because the overlay is materialized once before admission.
    pub fn prepare_source_snapshot_overlay(
        &self,
        snapshot: &mut SourceSnapshot,
    ) -> Result<(), MatrixError> {
        self.validate_profile_binding()?;
        let runtime_envelopes = self.runtime_envelopes()?;
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
    MatrixReceiptEnvelopeV2::seal(material.receipt)
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatrixReceiptEnvelopeV2 {
    pub receipt_id: String,
    pub receipt: MatrixReceiptV2,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatrixReceiptV2 {
    pub schema_version: String,
    pub producer: ProducerEvidence,
    pub repository: RepositoryEvidence,
    pub run: RunEvidence,
    pub configuration_digest: String,
    pub runtime_receipts: Vec<MatrixRuntimeReceiptV2>,
    pub overall_status: EvidenceStatus,
    pub incomplete_reason: Option<String>,
    pub redaction_policy_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatrixRuntimeReceiptV2 {
    pub runtime_id: String,
    pub receipt: ReceiptEnvelopeV1,
}

impl MatrixReceiptEnvelopeV2 {
    pub fn seal(receipt: MatrixReceiptV2) -> Result<Self, MatrixError> {
        receipt.validate()?;
        let receipt_id = canonical_digest(&receipt).map_err(MatrixError::Receipt)?;
        Ok(Self {
            receipt_id,
            receipt,
        })
    }

    pub fn verify(&self) -> Result<(), MatrixError> {
        self.receipt.validate()?;
        let expected = canonical_digest(&self.receipt).map_err(MatrixError::Receipt)?;
        if expected != self.receipt_id {
            return Err(MatrixError::ReceiptIdMismatch);
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, MatrixError> {
        self.verify()?;
        canonical_json(self).map_err(MatrixError::Receipt)
    }
}

impl MatrixReceiptV2 {
    pub fn validate(&self) -> Result<(), MatrixError> {
        if self.schema_version != MATRIX_RECEIPT_SCHEMA_VERSION {
            return Err(MatrixError::UnsupportedSchemaVersion(
                self.schema_version.clone(),
            ));
        }
        if self.repository.dirty || self.runtime_receipts.len() < 2 {
            return Err(MatrixError::InvalidReceipt);
        }
        let mut runtime_ids = BTreeSet::new();
        let mut check_ids = BTreeSet::new();
        let mut all_checks = Vec::new();
        for group in &self.runtime_receipts {
            validate_identifier("runtime_receipts.runtime_id", &group.runtime_id)
                .map_err(MatrixError::Config)?;
            if !runtime_ids.insert(group.runtime_id.as_str()) {
                return Err(MatrixError::DuplicateValue("runtime_receipts.runtime_id"));
            }
            group.receipt.verify().map_err(MatrixError::Receipt)?;
            let receipt = &group.receipt.receipt;
            if receipt.repository != self.repository || receipt.producer != self.producer {
                return Err(MatrixError::InvalidReceipt);
            }
            for check in &receipt.checks {
                if !check_ids.insert(check.id.as_str()) {
                    return Err(MatrixError::DuplicateValue("runtime_receipts.checks.id"));
                }
                all_checks.push(check.clone());
            }
        }
        if all_checks.is_empty() {
            return Err(MatrixError::InvalidReceipt);
        }
        let platform = self.runtime_receipts[0].receipt.receipt.platform.clone();
        ReceiptV1 {
            schema_version: crate::receipt::RECEIPT_SCHEMA_VERSION.to_owned(),
            producer: self.producer.clone(),
            repository: self.repository.clone(),
            run: self.run.clone(),
            platform,
            configuration_digest: self.configuration_digest.clone(),
            checks: all_checks,
            overall_status: self.overall_status,
            incomplete_reason: self.incomplete_reason.clone(),
            redaction_policy_version: self.redaction_policy_version.clone(),
        }
        .validate()
        .map_err(MatrixError::Receipt)
    }
}

pub fn matrix_config_schema_json() -> Result<String, MatrixError> {
    serde_json::to_string_pretty(&schema_for!(MatrixConfigV2)).map_err(MatrixError::Json)
}

pub fn matrix_receipt_schema_json() -> Result<String, MatrixError> {
    ccp_core::schema::combined_receipt_v2_schema_json().map_err(MatrixError::Json)
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatrixVerificationPolicyV2 {
    pub schema_version: String,
    pub project: String,
    pub configuration_digest: String,
    pub required_checks: Vec<MatrixRequiredCheckV2>,
    pub max_age_seconds: u64,
    pub runtimes: Vec<MatrixRuntimePolicyV2>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatrixRequiredCheckV2 {
    pub id: String,
    pub runtime_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatrixRuntimePolicyV2 {
    pub id: String,
    pub configuration_digest: String,
    pub image_reference: String,
    pub platforms: Vec<AcceptedPlatformV1>,
}

impl MatrixVerificationPolicyV2 {
    pub fn load(path: &Path) -> Result<Self, MatrixError> {
        let source = fs::read_to_string(path).map_err(MatrixError::Io)?;
        Self::parse(&source)
    }

    pub fn parse(source: &str) -> Result<Self, MatrixError> {
        if source.len() > MAX_MATRIX_BYTES {
            return Err(MatrixError::ConfigTooLarge);
        }
        let policy: Self = toml::from_str(source).map_err(MatrixError::Parse)?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), MatrixError> {
        if self.schema_version != MATRIX_POLICY_SCHEMA_VERSION {
            return Err(MatrixError::UnsupportedSchemaVersion(
                self.schema_version.clone(),
            ));
        }
        if !(2..=MAX_MATRIX_RUNTIMES).contains(&self.runtimes.len())
            || self.required_checks.is_empty()
        {
            return Err(MatrixError::InvalidField("runtimes_or_required_checks"));
        }
        let mut runtime_ids = BTreeSet::new();
        for runtime in &self.runtimes {
            validate_identifier("runtimes.id", &runtime.id).map_err(MatrixError::Config)?;
            if !runtime_ids.insert(runtime.id.as_str()) {
                return Err(MatrixError::DuplicateValue("runtimes.id"));
            }
            VerificationPolicyV1 {
                schema_version: "1.0".to_owned(),
                project: self.project.clone(),
                configuration_digest: runtime.configuration_digest.clone(),
                required_checks: vec![runtime.id.clone()],
                image_reference: runtime.image_reference.clone(),
                max_age_seconds: self.max_age_seconds,
                platforms: runtime.platforms.clone(),
            }
            .validate()
            .map_err(|error| MatrixError::Policy(error.to_string()))?;
        }
        let mut check_ids = BTreeSet::new();
        let mut coverage = BTreeSet::new();
        for check in &self.required_checks {
            validate_identifier("required_checks.id", &check.id).map_err(MatrixError::Config)?;
            validate_identifier("required_checks.runtime_id", &check.runtime_id)
                .map_err(MatrixError::Config)?;
            if !runtime_ids.contains(check.runtime_id.as_str()) {
                return Err(MatrixError::UnknownRuntime(check.runtime_id.clone()));
            }
            if !check_ids.insert(check.id.as_str()) {
                return Err(MatrixError::DuplicateValue("required_checks.id"));
            }
            coverage.insert(check.runtime_id.as_str());
        }
        if coverage.len() != runtime_ids.len() {
            return Err(MatrixError::InvalidField(
                "required_checks.runtime_coverage",
            ));
        }
        Ok(())
    }
}

pub fn matrix_policy_schema_json() -> Result<String, MatrixError> {
    serde_json::to_string_pretty(&schema_for!(MatrixVerificationPolicyV2))
        .map_err(MatrixError::Json)
}

pub fn verify_matrix_receipt_document(
    bytes: &[u8],
    policy: &MatrixVerificationPolicyV2,
    expected_commit: &str,
    evaluated_at_utc: &str,
) -> Result<VerificationReportV1, MatrixError> {
    policy.validate()?;
    validate_commit(expected_commit).map_err(MatrixError::Verification)?;
    let evaluated_at =
        parse_utc_seconds(evaluated_at_utc).ok_or(MatrixError::InvalidEvaluationTime)?;
    let mut report = VerificationReportV1 {
        schema_version: crate::verify::VERIFICATION_REPORT_SCHEMA_VERSION.to_owned(),
        assurance_scope: "integrity_and_repository_policy_only".to_owned(),
        evaluated_at_utc: evaluated_at_utc.to_owned(),
        expected_commit: expected_commit.to_owned(),
        receipt_id: None,
        integrity_status: VerificationStatus::Fail,
        policy_status: VerificationStatus::NotRun,
        decision: VerificationDecision::Fail,
        findings: Vec::new(),
    };
    let envelope: MatrixReceiptEnvelopeV2 = match serde_json::from_slice(bytes) {
        Ok(value) => value,
        Err(_) => {
            report.findings.push(finding(
                "receipt.parse_or_shape",
                "receipt",
                "receipt is not valid strict schema v2 JSON",
            ));
            return Ok(report);
        }
    };
    if envelope.verify().is_err() {
        report.findings.push(finding(
            "receipt.semantic_or_digest_invalid",
            "receipt",
            "receipt violates v2 integrity invariants",
        ));
        return Ok(report);
    }
    report.receipt_id = Some(envelope.receipt_id.clone());
    report.integrity_status = VerificationStatus::Pass;
    report.policy_status = VerificationStatus::Pass;
    evaluate_matrix_policy(
        &envelope,
        policy,
        expected_commit,
        evaluated_at,
        &mut report.findings,
    );
    if !report.findings.is_empty() {
        report.policy_status = VerificationStatus::Fail;
    }
    if report.policy_status == VerificationStatus::Pass {
        report.decision = VerificationDecision::Pass;
    }
    Ok(report)
}

fn evaluate_matrix_policy(
    envelope: &MatrixReceiptEnvelopeV2,
    policy: &MatrixVerificationPolicyV2,
    expected_commit: &str,
    evaluated_at: i64,
    findings: &mut Vec<VerificationFindingV1>,
) {
    let receipt = &envelope.receipt;
    equal(
        &receipt.repository.repository,
        &policy.project,
        "policy.repository",
        "repository.repository",
        "receipt project does not match repository policy",
        findings,
    );
    equal(
        &receipt.repository.commit_sha,
        expected_commit,
        "policy.commit",
        "repository.commit_sha",
        "receipt commit does not match the externally supplied commit",
        findings,
    );
    if receipt.repository.dirty {
        findings.push(finding(
            "policy.dirty",
            "repository.dirty",
            "repository policy requires a clean checkout",
        ));
    }
    equal(
        &receipt.configuration_digest,
        &policy.configuration_digest,
        "policy.configuration",
        "configuration_digest",
        "receipt configuration digest does not match repository policy",
        findings,
    );
    if receipt.overall_status != EvidenceStatus::Pass {
        findings.push(finding(
            "policy.overall_status",
            "overall_status",
            "repository policy requires an overall PASS receipt",
        ));
    }
    let policy_runtimes: BTreeMap<_, _> = policy
        .runtimes
        .iter()
        .map(|runtime| (runtime.id.as_str(), runtime))
        .collect();
    let receipt_runtimes: BTreeMap<_, _> = receipt
        .runtime_receipts
        .iter()
        .map(|runtime| (runtime.runtime_id.as_str(), runtime))
        .collect();
    if policy_runtimes.len() != receipt_runtimes.len()
        || policy_runtimes
            .keys()
            .any(|id| !receipt_runtimes.contains_key(id))
    {
        findings.push(finding(
            "policy.runtime_set",
            "runtime_receipts",
            "receipt runtime set does not exactly match repository policy",
        ));
    }
    for (runtime_id, expected) in policy_runtimes {
        let Some(actual) = receipt_runtimes.get(runtime_id) else {
            continue;
        };
        let platform = &actual.receipt.receipt.platform;
        equal(
            &actual.receipt.receipt.configuration_digest,
            &expected.configuration_digest,
            "policy.runtime_configuration",
            "runtime_receipts.configuration_digest",
            "receipt runtime configuration does not match repository policy",
            findings,
        );
        equal(
            &platform.image_reference,
            &expected.image_reference,
            "policy.runtime_image",
            "runtime_receipts.platform.image_reference",
            "receipt runtime image does not match repository policy",
            findings,
        );
        if !expected.platforms.iter().any(|accepted| {
            accepted.host_os == platform.host_os
                && accepted.host_arch == platform.host_arch
                && accepted.runtime_kind == platform.runtime_kind
        }) {
            findings.push(finding(
                "policy.runtime_platform",
                "runtime_receipts.platform",
                "receipt runtime platform tuple is not accepted by repository policy",
            ));
        }
    }
    let expected_checks: BTreeMap<_, _> = policy
        .required_checks
        .iter()
        .map(|check| (check.id.as_str(), check.runtime_id.as_str()))
        .collect();
    let mut actual_checks = BTreeMap::new();
    for runtime in &receipt.runtime_receipts {
        for check in &runtime.receipt.receipt.checks {
            if check.required {
                actual_checks.insert(check.id.as_str(), (runtime.runtime_id.as_str(), check));
            }
        }
    }
    if actual_checks.len() != expected_checks.len()
        || expected_checks
            .keys()
            .any(|id| !actual_checks.contains_key(id))
    {
        findings.push(finding(
            "policy.required_check_set",
            "checks",
            "required check set does not exactly match repository policy",
        ));
    }
    for (id, runtime_id) in expected_checks {
        match actual_checks.get(id) {
            Some((actual_runtime, check))
                if *actual_runtime == runtime_id && check.status == EvidenceStatus::Pass => {}
            Some((actual_runtime, _)) if *actual_runtime != runtime_id => findings.push(finding(
                "policy.check_runtime",
                "checks.runtime_id",
                "required check ran in a different runtime than repository policy",
            )),
            Some(_) => findings.push(finding(
                "policy.required_check_result",
                "checks.status",
                "one or more policy-required checks did not PASS",
            )),
            None => {}
        }
    }
    match parse_utc_seconds(&receipt.run.finished_at_utc) {
        Some(finished) if finished > evaluated_at => findings.push(finding(
            "policy.future_receipt",
            "run.finished_at_utc",
            "receipt completion time is later than verification time",
        )),
        Some(finished) if evaluated_at - finished > policy.max_age_seconds as i64 => {
            findings.push(finding(
                "policy.stale_receipt",
                "run.finished_at_utc",
                "receipt exceeds repository freshness policy",
            ))
        }
        Some(_) => {}
        None => findings.push(finding(
            "policy.invalid_time",
            "run.finished_at_utc",
            "receipt completion time cannot be evaluated",
        )),
    }
}

fn equal(
    actual: &str,
    expected: &str,
    code: &str,
    field: &str,
    message: &str,
    findings: &mut Vec<VerificationFindingV1>,
) {
    if actual != expected {
        findings.push(finding(code, field, message));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    use crate::cache::{CacheRootOptions, PlatformFamily, ResolvedCacheRoot};
    use crate::process::{
        CapturedStream, CleanupStatus, ExitOutcome, GenerationGuard, ProcessError, ProcessRequest,
        ProcessResult, ProcessTermination, RunIdentity,
    };
    use crate::receipt::{CheckEvidence, PlatformEvidence};
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
        envelope
            .prepare_source_snapshot_overlay(&mut snapshot)
            .expect("matrix snapshot overlay");
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
        envelope
            .prepare_source_snapshot_overlay(&mut snapshot)
            .expect("matrix snapshot overlay");
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
