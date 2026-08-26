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

use crate::config::{
    NormalizedArtifactContract, NormalizedCache, NormalizedCheck, NormalizedEnvironment,
    NormalizedFixedEnvironment, NormalizedReceipt, NormalizedRuntime,
    NormalizedRuntimeInternalEnvironment,
};
use crate::matrix::{MatrixError, MatrixPlanV2, MatrixRuntimePlanV2};
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
    receipt: LegacyNormalizedReceipt,
    environment_allow: Vec<String>,
    caches: Vec<LegacyNormalizedCache>,
    checks: Vec<LegacyNormalizedCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LegacyMatrixPlanV2 {
    schema_version: String,
    project: String,
    receipt: LegacyNormalizedReceipt,
    environment_allow: Vec<String>,
    caches: Vec<LegacyNormalizedCache>,
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
struct LegacyNormalizedReceipt {
    output: String,
    freshness_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LegacyNormalizedCache {
    id: String,
    mount_path: String,
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
    // Matrix V2 has no storage field today. Exhaustive destructuring at every
    // current nested boundary makes a future field addition fail compilation
    // until it is either represented in the historical shape or rejected.
    let environment_allow = legacy_environment(environment)?;
    let legacy_receipt = legacy_receipt(receipt);
    let legacy_caches = caches.iter().map(legacy_cache).collect::<Vec<_>>();
    let mut runtime_digests = BTreeMap::new();
    let mut legacy_runtimes = Vec::with_capacity(runtimes.len());
    for runtime_plan in runtimes {
        let MatrixRuntimePlanV2 {
            id,
            configuration_digest: _,
            runtime,
            checks,
        } = runtime_plan;
        let checks = checks
            .iter()
            .map(legacy_check)
            .collect::<Result<Vec<_>, MatrixError>>()?;
        let legacy_runtime = legacy_runtime(runtime)?;
        let configuration_digest = canonical_digest(&LegacyExecutionPlanV1 {
            schema_version: "1.0".to_owned(),
            project: project.clone(),
            runtime: legacy_runtime.clone(),
            receipt: legacy_receipt.clone(),
            environment_allow: environment_allow.clone(),
            caches: legacy_caches.clone(),
            checks: checks.clone(),
        })
        .map_err(MatrixError::Receipt)?;
        runtime_digests.insert(id.clone(), configuration_digest.clone());
        legacy_runtimes.push(LegacyMatrixRuntimePlanV2 {
            id: id.clone(),
            configuration_digest,
            runtime: legacy_runtime,
            checks,
        });
    }

    Ok(LegacyMatrixDigestBasisV1 {
        plan: LegacyMatrixPlanV2 {
            schema_version: schema_version.clone(),
            project: project.clone(),
            receipt: legacy_receipt,
            environment_allow,
            caches: legacy_caches,
            runtimes: legacy_runtimes,
        },
        runtime_digests,
    })
}

fn legacy_environment(environment: &NormalizedEnvironment) -> Result<Vec<String>, MatrixError> {
    let NormalizedEnvironment {
        inherit,
        fixed,
        runtime_internal,
        remote_secret_only,
    } = environment;
    if let Some(binding) = fixed.first() {
        let NormalizedFixedEnvironment {
            name: _,
            value_digest: _,
        } = binding;
        return Err(MatrixError::LegacyPlanNotRepresentable("environment.fixed"));
    }
    if let Some(binding) = runtime_internal.first() {
        let NormalizedRuntimeInternalEnvironment {
            name: _,
            cache_id: _,
            container_target: _,
        } = binding;
        return Err(MatrixError::LegacyPlanNotRepresentable(
            "environment.runtime_internal",
        ));
    }
    if !remote_secret_only.is_empty() {
        return Err(MatrixError::LegacyPlanNotRepresentable(
            "environment.remote_secret_only",
        ));
    }
    Ok(inherit.clone())
}

fn legacy_receipt(receipt: &NormalizedReceipt) -> LegacyNormalizedReceipt {
    let NormalizedReceipt {
        output,
        freshness_seconds,
    } = receipt;
    LegacyNormalizedReceipt {
        output: output.clone(),
        freshness_seconds: *freshness_seconds,
    }
}

fn legacy_cache(cache: &NormalizedCache) -> LegacyNormalizedCache {
    let NormalizedCache { id, mount_path } = cache;
    LegacyNormalizedCache {
        id: id.clone(),
        mount_path: mount_path.clone(),
    }
}

fn legacy_runtime(runtime: &NormalizedRuntime) -> Result<LegacyNormalizedRuntime, MatrixError> {
    let NormalizedRuntime {
        kind,
        image,
        cpu_count,
        memory_mib,
        pids_limit,
        network,
        pull_policy,
        swap_mode,
    } = runtime;
    if pull_policy.is_some() {
        return Err(MatrixError::LegacyPlanNotRepresentable(
            "runtime.pull_policy",
        ));
    }
    if swap_mode.is_some() {
        return Err(MatrixError::LegacyPlanNotRepresentable("runtime.swap_mode"));
    }
    Ok(LegacyNormalizedRuntime {
        kind: *kind,
        image: image.clone(),
        cpu_count: *cpu_count,
        memory_mib: *memory_mib,
        pids_limit: *pids_limit,
        network: *network,
    })
}

fn legacy_check(check: &NormalizedCheck) -> Result<LegacyNormalizedCheck, MatrixError> {
    let NormalizedCheck {
        id,
        required,
        argv,
        working_directory,
        timeout_seconds,
        depends_on,
        artifacts,
        artifact_contracts,
    } = check;
    if let Some(contract) = artifact_contracts.first() {
        let NormalizedArtifactContract {
            path: _,
            kind: _,
            max_bytes: _,
            max_entries: _,
            producer_check: _,
        } = contract;
        return Err(MatrixError::LegacyPlanNotRepresentable(
            "checks.artifact_contracts",
        ));
    }
    Ok(LegacyNormalizedCheck {
        id: id.clone(),
        required: *required,
        argv: argv.clone(),
        working_directory: working_directory.clone(),
        timeout_seconds: *timeout_seconds,
        depends_on: depends_on.clone(),
        artifacts: artifacts.clone(),
    })
}
