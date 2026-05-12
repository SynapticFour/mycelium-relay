use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

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
            hop_count: 0,
            max_hops: default_max_hops(),
        }
    }

    pub fn sign(&mut self, keypair: &libp2p::identity::Keypair) -> anyhow::Result<()> {
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
        let bytes = self.bytes_for_signing();
        let multihash = from_peer_id.as_ref();
        if multihash.code() != 0 {
            return false;
        }
        let Ok(pk) = libp2p::identity::PublicKey::try_decode_protobuf(multihash.digest()) else {
            return false;
        };
        pk.verify(&bytes, sig)
    }

    fn bytes_for_signing(&self) -> Vec<u8> {
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

fn default_max_hops() -> u8 {
    8
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
