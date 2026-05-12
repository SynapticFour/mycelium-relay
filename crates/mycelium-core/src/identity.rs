use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeIdentity {
    pub peer_id: String,
    pub public_key: Vec<u8>,
}
