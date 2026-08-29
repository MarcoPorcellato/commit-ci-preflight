use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCapabilityEvidenceV1 {
    pub schema_version: String,
    pub memory_limit_supported: bool,
    pub swap_limit_supported: bool,
    pub context_digest: String,
    pub resolved_image_id: String,
    pub resolved_image_reference: String,
}
