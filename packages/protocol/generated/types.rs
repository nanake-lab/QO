/// Generated from JSON Schema. Run pnpm generate to update.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    pub versions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    pub version: ProtocolVersion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProtocolVersion {
    #[serde(rename = "qo.v1")]
    QoV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginLinkResult {
    pub url: String,
}
