use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrustStatus {
    Trusted,
    Untrusted,
    Stale,
    Invalid,
    Missing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedAsset {
    pub id: String,
    pub marketplace: String,
    pub revision: String,
    pub trust_status: TrustStatus,
    pub source_branch: String,
}

