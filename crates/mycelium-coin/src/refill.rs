use crate::address::{address_from_public_key, validate_address};
use libp2p::identity::{Keypair, PublicKey};
use serde::{Deserialize, Serialize};

/// Hot-signed mesh refill requests expire after this interval (ms).
pub const REFILL_REQUEST_TTL_MS: u64 = 15 * 60 * 1000;

/// Request from hot wallet for the cold node to send Muon to `hot_address`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RefillRequest {
    pub request_id: String,
    pub hot_address: String,
    pub cold_address: String,
    pub amount_muon: u64,
    pub fee_muon: u64,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub hot_public_key: Vec<u8>,
    pub signature: Vec<u8>,
}

impl RefillRequest {
    pub fn new_signed(
        hot_keypair: &Keypair,
        cold_address: String,
        amount_muon: u64,
        fee_muon: u64,
    ) -> anyhow::Result<Self> {
        let hot_address = crate::address::address_from_keypair(hot_keypair);
        let hot_public_key = hot_keypair.public().encode_protobuf();
        let created_at_ms = mycelium_core::data::now_ms();
        let expires_at_ms = created_at_ms.saturating_add(REFILL_REQUEST_TTL_MS);
        let request_id = uuid::Uuid::new_v4().to_string();
        let mut req = Self {
            request_id,
            hot_address,
            cold_address,
            amount_muon,
            fee_muon,
            created_at_ms,
            expires_at_ms,
            hot_public_key,
            signature: Vec::new(),
        };
        let bytes = req.bytes_for_signing();
        req.signature = hot_keypair
            .sign(&bytes)
            .map_err(|e| anyhow::anyhow!("refill sign failed: {e}"))?;
        Ok(req)
    }

    pub fn verify(&self) -> bool {
        if !validate_address(&self.hot_address) || !validate_address(&self.cold_address) {
            return false;
        }
        if self.hot_address == self.cold_address {
            return false;
        }
        if self.expires_at_ms < self.created_at_ms {
            return false;
        }
        let Ok(pk) = PublicKey::try_decode_protobuf(&self.hot_public_key) else {
            return false;
        };
        if address_from_public_key(&pk) != self.hot_address {
            return false;
        }
        let bytes = self.bytes_for_signing();
        pk.verify(&bytes, &self.signature)
    }

    fn bytes_for_signing(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(self.request_id.as_bytes());
        b.extend_from_slice(self.hot_address.as_bytes());
        b.extend_from_slice(self.cold_address.as_bytes());
        b.extend_from_slice(&self.amount_muon.to_le_bytes());
        b.extend_from_slice(&self.fee_muon.to_le_bytes());
        b.extend_from_slice(&self.created_at_ms.to_le_bytes());
        b.extend_from_slice(&self.expires_at_ms.to_le_bytes());
        b.extend_from_slice(&self.hot_public_key);
        b
    }
}
