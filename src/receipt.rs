pub use ccp_core::canonical::{canonical_digest, canonical_json};
pub use ccp_core::errors::ReceiptError;
pub use ccp_core::receipt::*;

pub fn receipt_v2_schema_json() -> Result<String, ReceiptError> {
    crate::schema_contract::combined_receipt_v2_schema_json().map_err(ReceiptError::Serialization)
}
