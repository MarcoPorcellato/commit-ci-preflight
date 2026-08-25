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

use std::collections::BTreeMap;

use serde::Serialize;

use crate::config::{NormalizedCache, NormalizedReceipt, NormalizedRuntime};
use crate::matrix::{MatrixError, MatrixPlanV2};
use crate::receipt::canonical_digest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyMatrixDigestBasisV1 {
    plan: LegacyMatrixPlanV2,
    runtime_digests: BTreeMap<String, String>,
}

impl LegacyMatrixDigestBasisV1 {
    pub(crate) fn outer_digest(&self) -> Result<String, MatrixError> {
        canonical_digest(&self.plan).map_err(MatrixError::Receipt)
    }

    pub(crate) fn runtime_digest(&self, id: &str) -> Result<&str, MatrixError> {
        self.runtime_digests
            .get(id)
            .map(String::as_str)
            .ok_or_else(|| MatrixError::UnknownRuntime(id.to_owned()))
    }

    pub(crate) fn report_value(&self) -> Result<serde_json::Value, MatrixError> {
        serde_json::to_value(&self.plan).map_err(MatrixError::Json)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LegacyExecutionPlanV1 {
    schema_version: String,
    project: String,
    runtime: LegacyNormalizedRuntime,
    receipt: NormalizedReceipt,
    environment_allow: Vec<String>,
    caches: Vec<NormalizedCache>,
    checks: Vec<LegacyNormalizedCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LegacyMatrixPlanV2 {
    schema_version: String,
    project: String,
    receipt: NormalizedReceipt,
    environment_allow: Vec<String>,
    caches: Vec<NormalizedCache>,
    runtimes: Vec<LegacyMatrixRuntimePlanV2>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LegacyNormalizedRuntime {
    kind: crate::config::RuntimeKind,
    image: String,
    cpu_count: u16,
    memory_mib: u64,
    pids_limit: u32,
    network: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LegacyNormalizedCheck {
    id: String,
    required: bool,
    argv: Vec<String>,
    working_directory: String,
    timeout_seconds: u64,
    depends_on: Vec<String>,
    artifacts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LegacyMatrixRuntimePlanV2 {
    id: String,
    configuration_digest: String,
    runtime: LegacyNormalizedRuntime,
    checks: Vec<LegacyNormalizedCheck>,
}

pub(crate) fn project_legacy_basis(
    plan: &MatrixPlanV2,
) -> Result<LegacyMatrixDigestBasisV1, MatrixError> {
    let MatrixPlanV2 {
        schema_version,
        project,
        receipt,
        environment,
        caches,
        runtimes,
    } = plan;
    // Matrix V2 has no storage field today. This intentionally exhaustive
    // destructuring makes any future field addition fail compilation until its
    // legacy representability classification, including storage, is explicit.
    for runtime in runtimes {
        validate_runtime_representability(&runtime.runtime)?;
    }
    if !environment.fixed.is_empty() {
        return Err(MatrixError::LegacyPlanNotRepresentable("environment.fixed"));
    }
    if !environment.runtime_internal.is_empty() {
        return Err(MatrixError::LegacyPlanNotRepresentable(
            "environment.runtime_internal",
        ));
    }
    if !environment.remote_secret_only.is_empty() {
        return Err(MatrixError::LegacyPlanNotRepresentable(
            "environment.remote_secret_only",
        ));
    }
    let mut runtime_digests = BTreeMap::new();
    let mut legacy_runtimes = Vec::with_capacity(runtimes.len());
    for runtime in runtimes {
        let checks = runtime
            .checks
            .iter()
            .map(|check| {
                if !check.artifact_contracts.is_empty() {
                    return Err(MatrixError::LegacyPlanNotRepresentable(
                        "checks.artifact_contracts",
                    ));
                }
                Ok(LegacyNormalizedCheck {
                    id: check.id.clone(),
                    required: check.required,
                    argv: check.argv.clone(),
                    working_directory: check.working_directory.clone(),
                    timeout_seconds: check.timeout_seconds,
                    depends_on: check.depends_on.clone(),
                    artifacts: check.artifacts.clone(),
                })
            })
            .collect::<Result<Vec<_>, MatrixError>>()?;
        let legacy_runtime = legacy_runtime(&runtime.runtime);
        let configuration_digest = canonical_digest(&LegacyExecutionPlanV1 {
            schema_version: "1.0".to_owned(),
            project: project.clone(),
            runtime: legacy_runtime.clone(),
            receipt: receipt.clone(),
            environment_allow: environment.inherit.clone(),
            caches: caches.clone(),
            checks: checks.clone(),
        })
        .map_err(MatrixError::Receipt)?;
        runtime_digests.insert(runtime.id.clone(), configuration_digest.clone());
        legacy_runtimes.push(LegacyMatrixRuntimePlanV2 {
            id: runtime.id.clone(),
            configuration_digest,
            runtime: legacy_runtime,
            checks,
        });
    }

    Ok(LegacyMatrixDigestBasisV1 {
        plan: LegacyMatrixPlanV2 {
            schema_version: schema_version.clone(),
            project: project.clone(),
            receipt: receipt.clone(),
            environment_allow: environment.inherit.clone(),
            caches: caches.clone(),
            runtimes: legacy_runtimes,
        },
        runtime_digests,
    })
}

fn validate_runtime_representability(runtime: &NormalizedRuntime) -> Result<(), MatrixError> {
    if runtime.pull_policy.is_some() {
        return Err(MatrixError::LegacyPlanNotRepresentable(
            "runtime.pull_policy",
        ));
    }
    if runtime.swap_mode.is_some() {
        return Err(MatrixError::LegacyPlanNotRepresentable("runtime.swap_mode"));
    }
    Ok(())
}

fn legacy_runtime(runtime: &NormalizedRuntime) -> LegacyNormalizedRuntime {
    LegacyNormalizedRuntime {
        kind: runtime.kind,
        image: runtime.image.clone(),
        cpu_count: runtime.cpu_count,
        memory_mib: runtime.memory_mib,
        pids_limit: runtime.pids_limit,
        network: runtime.network,
    }
}
