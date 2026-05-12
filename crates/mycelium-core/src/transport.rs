use crate::data::Envelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectMessage {
    pub envelope: Envelope,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WireMessage {
    Data(DirectMessage),
    SyncBloom { bloom: Vec<u8>, count: u64 },
    SyncIds { ids: Vec<String> },
    SyncRequest { ids: Vec<String> },
    SyncData { messages: Vec<DirectMessage> },
    ScopeAnnounce { scopes: Vec<String> },
}

impl DirectMessage {
    pub fn is_expired(&self, now_ms: u64) -> bool {
        let expiry = self
            .envelope
            .created_at_ms
            .saturating_add(self.envelope.ttl_secs as u64 * 1000);
        now_ms >= expiry
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAck {
    pub message_id: String,
    pub accepted: bool,
}

pub type ScopeId = String;

/// Coarse network reachability used for hybrid Internet / mesh routing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConnectivityMode {
    /// Reachable bootstrap / Internet path — prefer wide-area transports and refill.
    Internet,
    /// Offline or bootstrap unreachable — stay on local mesh (mDNS, LAN peers).
    MeshOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Scope(pub String);

impl Scope {
    pub fn matches(&self, pattern: &str) -> bool {
        if let Some(prefix) = pattern.strip_suffix("/*") {
            self.0.starts_with(prefix)
        } else {
            self.0 == pattern
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreStats {
    pub count: usize,
    pub oldest_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireFrame {
    pub envelope: Envelope,
    pub scope: Option<ScopeId>,
    pub hop_count: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransportEvent {
    Listening {
        address: String,
    },
    PeerUp {
        peer_id: String,
    },
    PeerDown {
        peer_id: String,
    },
    DirectReceived {
        from_peer: String,
        message: WireMessage,
    },
    ScopedReceived {
        from_peer: String,
        scope: ScopeId,
        payload: Vec<u8>,
    },
    DirectAck {
        from_peer: String,
        ack: MessageAck,
    },
    SendFailure {
        to_peer: String,
        reason: String,
    },
    ConnectivityChanged {
        mode: ConnectivityMode,
    },
}

#[async_trait]
pub trait MeshTransport: Send {
    fn local_peer_id(&self) -> String;
    fn known_peers(&self) -> Vec<String>;
    fn local_keypair(&self) -> Option<libp2p::identity::Keypair>;
    async fn dial_peer(&mut self, multiaddr: String) -> anyhow::Result<()>;
    async fn send_direct(&mut self, to_peer: String, message: WireMessage) -> anyhow::Result<()>;
    async fn publish_scoped(&mut self, scope: ScopeId, payload: Vec<u8>) -> anyhow::Result<()>;
    async fn next_event(&mut self) -> anyhow::Result<TransportEvent>;
}

#[async_trait]
pub trait MessageStore: Send + Sync {
    async fn persist(&self, message: &DirectMessage) -> anyhow::Result<()>;
    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<DirectMessage>>;
    async fn contains(&self, message_id: &str) -> anyhow::Result<bool>;
    async fn load_by_id(&self, message_id: &str) -> anyhow::Result<Option<DirectMessage>>;
    async fn list_ids_window(&self, _window: Duration) -> anyhow::Result<Vec<String>>;
    async fn gc_expired(&self) -> anyhow::Result<usize>;
    async fn stats(&self) -> anyhow::Result<StoreStats>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Envelope;

    #[test]
    fn direct_message_expiry_respects_ttl() {
        let mut envelope = Envelope::new("a".to_string(), Some("b".to_string()), b"hi".to_vec());
        envelope.created_at_ms = 1_000;
        envelope.ttl_secs = 2;
        let msg = DirectMessage {
            envelope,
            body: "hi".to_string(),
        };
        assert!(!msg.is_expired(2_999));
        assert!(msg.is_expired(3_000));
    }
}
