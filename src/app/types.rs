use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// Shared application-specific types for API communication

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub struct ZinitResponse {
    pub state: ZinitState,
    pub body: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ZinitState {
    Ok,
    Error,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub struct Status {
    pub name: String,
    pub pid: u32,
    pub state: String,
    pub target: String,
    pub after: HashMap<String, String>,
}

// Note: We might not need ZinitResponse/ZinitState directly in the jsonrpsee API,
// as errors are handled by jsonrpsee's error mechanism and success returns the specific data (like Status or Value::Null).
// We'll keep them for now and refine later if they become unused by the RPC trait.