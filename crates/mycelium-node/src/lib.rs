mod behaviour;
pub mod connectivity;
mod forwarding;
mod node;
pub mod secrets;
pub mod security;
mod storage;
mod transport;

pub use connectivity::ConnectivityMonitor;
pub use mycelium_core::at_rest::parse_storage_key_hex;
pub use mycelium_core::transport::{ConnectivityMode, StoreStats};
pub use node::{
    NodeCommand, NodeConfig, NodeHandle, NodeMetrics, NodeRunner, PeerReputationSnapshot,
    SYSTEM_SCOPES,
};
pub use secrets::{load_or_create_enc_keypair, StorageKey};

/// Load or create the libp2p identity using OS keyring / encrypted storage (no explicit storage key).
pub fn load_or_create_keypair(path: &str) -> anyhow::Result<libp2p::identity::Keypair> {
    secrets::load_or_create_keypair(path, None)
}
