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
    SyncBloom {
        bloom: Vec<u8>,
        count: u64,
    },
    SyncIds {
        ids: Vec<String>,
    },
    SyncRequest {
        ids: Vec<String>,
    },
    SyncData {
        messages: Vec<DirectMessage>,
    },
    ScopeAnnounce {
        scopes: Vec<String>,
    },
    /// X25519-E2E für genau einen Empfänger (`to_peer` = libp2p Peer-ID String).
    EncryptedDirect {
        to_peer: String,
        /// Hex-kodierter X25519-Public-Key des Senders (64 hex chars).
        sender_enc_pubkey: String,
        /// `[ephemeral_pubkey 32]` + `[nonce 12]` + `[ciphertext]`
        encrypted_payload: Vec<u8>,
        /// libp2p-Identitäts-Signatur des unmittelbaren Senders (Relay-Hop).
        #[serde(default)]
        mesh_signature: Option<Vec<u8>>,
        #[serde(default)]
        hop_count: u8,
        #[serde(default = "default_max_hops")]
        max_hops: u8,
    },
    /// Symmetrisch verschlüsselte Gruppenlast; `group_id` ist öffentlicher Identifier (kein Key).
    EncryptedGroup {
        group_id: String,
        /// `[nonce 12]` + `[ciphertext]`
        encrypted_payload: Vec<u8>,
    },
    /// Bekanntgabe des X25519-Enc-Public-Keys nach Verbindungsaufbau.
    PeerInfo {
        enc_pubkey_hex: String,
        display_name: Option<String>,
        supported_scopes: Vec<String>,
    },
}

fn default_max_hops() -> u8 {
    8
}

/// Canonical bytes signed by the libp2p peer that forwards this wire frame.
pub fn encrypted_direct_signing_bytes(
    to_peer: &str,
    sender_enc_pubkey: &str,
    encrypted_payload: &[u8],
    hop_count: u8,
    max_hops: u8,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"MYCEL/ENC_DIRECT/v1\0");
    bytes.extend_from_slice(to_peer.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(sender_enc_pubkey.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&(encrypted_payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(encrypted_payload);
    bytes.push(hop_count);
    bytes.push(max_hops);
    bytes
}

/// Attach a libp2p identity signature to an [`WireMessage::EncryptedDirect`].
pub fn sign_encrypted_direct(
    message: &mut WireMessage,
    keypair: &libp2p::identity::Keypair,
) -> anyhow::Result<()> {
    let WireMessage::EncryptedDirect {
        to_peer,
        sender_enc_pubkey,
        encrypted_payload,
        hop_count,
        max_hops,
        mesh_signature,
    } = message
    else {
        anyhow::bail!("not EncryptedDirect");
    };
    let bytes = encrypted_direct_signing_bytes(
        to_peer,
        sender_enc_pubkey,
        encrypted_payload,
        *hop_count,
        *max_hops,
    );
    let sig = keypair
        .sign(&bytes)
        .map_err(|e| anyhow::anyhow!("failed to sign EncryptedDirect: {e}"))?;
    *mesh_signature = Some(sig);
    Ok(())
}

/// Verify the mesh signature on an incoming [`WireMessage::EncryptedDirect`].
pub fn verify_encrypted_direct(message: &WireMessage, sender_peer_id: &libp2p::PeerId) -> bool {
    let WireMessage::EncryptedDirect {
        to_peer,
        sender_enc_pubkey,
        encrypted_payload,
        mesh_signature,
        hop_count,
        max_hops,
    } = message
    else {
        return false;
    };
    let Some(sig) = mesh_signature else {
        return false;
    };
    let bytes = encrypted_direct_signing_bytes(
        to_peer,
        sender_enc_pubkey,
        encrypted_payload,
        *hop_count,
        *max_hops,
    );
    let multihash = sender_peer_id.as_ref();
    if multihash.code() != 0 {
        return false;
    }
    let Ok(pk) = libp2p::identity::PublicKey::try_decode_protobuf(multihash.digest()) else {
        return false;
    };
    pk.verify(&bytes, sig)
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
    /// Join a gossipsub topic so scoped messages for `scope` are received.
    async fn subscribe_scope(&mut self, scope: ScopeId) -> anyhow::Result<()>;
    /// Leave a gossipsub topic.
    async fn unsubscribe_scope(&mut self, scope: ScopeId) -> anyhow::Result<()>;
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
