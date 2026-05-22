use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Signature scheme v1 (ID in signed bytes) launch time (UTC 2026-05-22).
const SIG_V1_LAUNCH_MS: u64 = 1_779_430_651_000;
/// Legacy envelopes with invalid v1 signatures are accepted with a warning for this window.
const SIG_TRANSITION_MS: u64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct ContentId(pub String);

impl ContentId {
    pub fn from_bytes(data: &[u8]) -> Self {
        Self(blake3::hash(data).to_hex().to_string())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
pub enum Priority {
    Low,
    Normal,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub id: ContentId,
    pub from_peer: String,
    pub to_peer: Option<String>,
    pub payload: Vec<u8>,
    pub created_at_ms: u64,
    pub ttl_secs: u32,
    pub priority: Priority,
    #[serde(default)]
    pub signature: Option<Vec<u8>>,
    /// 0 = legacy (no ID in signed bytes); 1 = v1 (ID included, replay-safe).
    #[serde(default)]
    pub sig_version: u8,
    #[serde(default)]
    pub hop_count: u8,
    #[serde(default = "default_max_hops")]
    pub max_hops: u8,
}

impl Envelope {
    pub fn new(from_peer: String, to_peer: Option<String>, payload: Vec<u8>) -> Self {
        let now = now_ms();
        let mut digest_input = from_peer.as_bytes().to_vec();
        digest_input.extend_from_slice(&payload);
        digest_input.extend_from_slice(&now.to_le_bytes());
        Self {
            id: ContentId::from_bytes(&digest_input),
            from_peer,
            to_peer,
            payload,
            created_at_ms: now,
            ttl_secs: 7 * 24 * 60 * 60,
            priority: Priority::Normal,
            signature: None,
            sig_version: 1,
            hop_count: 0,
            max_hops: default_max_hops(),
        }
    }

    pub fn sign(&mut self, keypair: &libp2p::identity::Keypair) -> anyhow::Result<()> {
        self.sig_version = 1;
        let bytes_to_sign = self.bytes_for_signing();
        let sig = keypair
            .sign(&bytes_to_sign)
            .map_err(|e| anyhow::anyhow!("failed to sign envelope: {e}"))?;
        self.signature = Some(sig);
        Ok(())
    }

    pub fn verify(&self, from_peer_id: &libp2p::PeerId) -> bool {
        let Some(sig) = &self.signature else {
            return false;
        };
        let bytes = if self.sig_version == 0 {
            self.bytes_for_signing_v0()
        } else {
            self.bytes_for_signing()
        };
        let multihash = from_peer_id.as_ref();
        if multihash.code() != 0 {
            return false;
        }
        let Ok(pk) = libp2p::identity::PublicKey::try_decode_protobuf(multihash.digest()) else {
            return false;
        };
        pk.verify(&bytes, sig)
    }

    /// Verify signature; during the 7-day transition after v1 launch, accept failures with a warning.
    pub fn verify_or_transition(&self, from_peer_id: &libp2p::PeerId) -> bool {
        if self.verify(from_peer_id) {
            return true;
        }
        if signature_transition_active() {
            tracing::warn!(
                "envelope signature verification failed (sig_version={}); accepting during transition window",
                self.sig_version
            );
            return true;
        }
        false
    }

    fn bytes_for_signing(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.id.0.as_bytes());
        bytes.extend_from_slice(self.from_peer.as_bytes());
        if let Some(to) = &self.to_peer {
            bytes.extend_from_slice(to.as_bytes());
        }
        bytes.extend_from_slice(&self.payload);
        bytes.extend_from_slice(&self.created_at_ms.to_le_bytes());
        bytes.extend_from_slice(&self.ttl_secs.to_le_bytes());
        bytes.push(self.priority as u8);
        bytes.push(self.hop_count);
        bytes.push(self.max_hops);
        bytes
    }

    fn bytes_for_signing_v0(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.from_peer.as_bytes());
        if let Some(to) = &self.to_peer {
            bytes.extend_from_slice(to.as_bytes());
        }
        bytes.extend_from_slice(&self.payload);
        bytes.extend_from_slice(&self.created_at_ms.to_le_bytes());
        bytes.extend_from_slice(&self.ttl_secs.to_le_bytes());
        bytes.push(self.priority as u8);
        bytes.push(self.hop_count);
        bytes.push(self.max_hops);
        bytes
    }
}

fn signature_transition_active() -> bool {
    now_ms() < SIG_V1_LAUNCH_MS.saturating_add(SIG_TRANSITION_MS)
}

fn default_max_hops() -> u8 {
    8
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_attack_rejected() {
        let keypair = libp2p::identity::Keypair::generate_ed25519();
        let peer_id = keypair.public().to_peer_id();

        let mut envelope = Envelope::new(peer_id.to_string(), None, b"original message".to_vec());
        envelope.sign(&keypair).unwrap();

        assert!(envelope.verify(&peer_id));

        let mut replayed = envelope.clone();
        replayed.created_at_ms += 1000;

        assert!(
            !replayed.verify(&peer_id),
            "replay with changed timestamp must fail verification"
        );
    }
}
