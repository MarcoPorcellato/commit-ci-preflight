//! Historical Matrix V2 digest projection.
use std::collections::BTreeMap;
use serde::Serialize;
use crate::canonical::canonical_digest;
use crate::matrix::{MatrixContractError, MatrixPlanV2};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyMatrixDigestBasisV1 { plan: LegacyMatrixPlanV2, runtime_digests: BTreeMap<String,String> }
#[derive(Debug, Clone, PartialEq, Eq, Serialize)] struct LegacyMatrixPlanV2 { schema_version:String, project:String, receipt:LegacyNormalizedReceipt, environment_allow:Vec<String>, caches:Vec<LegacyNormalizedCache>, runtimes:Vec<LegacyMatrixRuntimePlanV2> }
#[derive(Debug, Clone, PartialEq, Eq, Serialize)] struct LegacyMatrixRuntimePlanV2 { id:String, configuration_digest:String, runtime:LegacyNormalizedRuntime, checks:Vec<LegacyNormalizedCheck> }
#[derive(Debug, Clone, PartialEq, Eq, Serialize)] struct LegacyNormalizedReceipt { output:String, freshness_seconds:u64 }
#[derive(Debug, Clone, PartialEq, Eq, Serialize)] struct LegacyNormalizedCache { id:String, mount_path:String }
#[derive(Debug, Clone, PartialEq, Eq, Serialize)] struct LegacyNormalizedRuntime { kind:crate::config::RuntimeKind, image:String, cpu_count:u16, memory_mib:u64, pids_limit:u32, network:bool }
#[derive(Debug, Clone, PartialEq, Eq, Serialize)] struct LegacyNormalizedCheck { id:String, required:bool, argv:Vec<String>, working_directory:String, timeout_seconds:u64, depends_on:Vec<String>, artifacts:Vec<String> }

impl LegacyMatrixDigestBasisV1 {
 pub fn outer_digest(&self)->Result<String,MatrixContractError>{canonical_digest(&self.plan).map_err(MatrixContractError::Receipt)}
 pub fn runtime_digest(&self,id:&str)->Result<&str,MatrixContractError>{self.runtime_digests.get(id).map(String::as_str).ok_or_else(||MatrixContractError::UnknownRuntime(id.into()))}
 pub fn report_value(&self)->Result<serde_json::Value,MatrixContractError>{serde_json::to_value(&self.plan).map_err(|e|MatrixContractError::Json(e))}
}
pub fn project_legacy_basis(plan:&MatrixPlanV2)->Result<LegacyMatrixDigestBasisV1,MatrixContractError>{
 let mut runtimes=Vec::new(); let mut digests=BTreeMap::new(); let receipt=LegacyNormalizedReceipt{output:plan.receipt.output.clone(),freshness_seconds:plan.receipt.freshness_seconds}; let caches=plan.caches.iter().map(|c|LegacyNormalizedCache{id:c.id.clone(),mount_path:c.mount_path.clone()}).collect::<Vec<_>>();
 for r in &plan.runtimes { let runtime=LegacyNormalizedRuntime{kind:r.runtime.kind,image:r.runtime.image.clone(),cpu_count:r.runtime.cpu_count,memory_mib:r.runtime.memory_mib,pids_limit:r.runtime.pids_limit,network:r.runtime.network}; let checks=r.checks.iter().map(|c|LegacyNormalizedCheck{id:c.id.clone(),required:c.required,argv:c.argv.clone(),working_directory:c.working_directory.clone(),timeout_seconds:c.timeout_seconds,depends_on:c.depends_on.clone(),artifacts:c.artifacts.clone()}).collect::<Vec<_>>(); let basis=LegacyExecutionPlanV1{schema_version:"1.0".into(),project:plan.project.clone(),runtime:runtime.clone(),receipt:receipt.clone(),environment_allow:plan.environment.inherit.clone(),caches:caches.clone(),checks:checks.clone()}; let d=canonical_digest(&basis).map_err(MatrixContractError::Receipt)?; digests.insert(r.id.clone(),d.clone()); runtimes.push(LegacyMatrixRuntimePlanV2{id:r.id.clone(),configuration_digest:d,runtime,checks}); }
 Ok(LegacyMatrixDigestBasisV1{plan:LegacyMatrixPlanV2{schema_version:plan.schema_version.clone(),project:plan.project.clone(),receipt,environment_allow:plan.environment.inherit.clone(),caches,runtimes},runtime_digests:digests})
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)] struct LegacyExecutionPlanV1 { schema_version:String, project:String, runtime:LegacyNormalizedRuntime, receipt:LegacyNormalizedReceipt, environment_allow:Vec<String>, caches:Vec<LegacyNormalizedCache>, checks:Vec<LegacyNormalizedCheck> }
