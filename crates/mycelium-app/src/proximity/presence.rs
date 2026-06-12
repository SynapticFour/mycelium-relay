// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROXIMITY_SCOPE: &str = "mycelium/proximity/v1";

/// Ein Presence-Signal das ein Nutzer in den Mesh broadcastet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceSignal {
    pub ephemeral_id: Uuid,
    pub enc_pubkey_hex: String,
    pub profile: PresenceProfile,
    pub created_at_ms: u64,
    pub ttl_secs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PresenceProfile {
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub age: Option<u8>,
    pub gender: Option<String>,
    pub looking_for: Option<String>,
    pub interests: Vec<String>,
    pub photo_base64: Option<String>,
}

/// E2E-Nachricht an einen Empfänger (identifiziert via X25519 pubkey, kein libp2p Peer-ID nötig).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProximityDirectMessage {
    pub target_enc_pubkey_hex: String,
    pub sender_enc_pubkey_hex: String,
    pub encrypted_payload: Vec<u8>,
    pub created_at_ms: u64,
}

/// Signalisiert Interesse an einem nearby Profil. Gegenseitige Intents = Match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProximityMatchIntent {
    pub from_enc_pubkey_hex: String,
    pub created_at_ms: u64,
    pub ttl_secs: u32,
}

impl PresenceProfile {
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(bio) = &self.bio {
            if bio.chars().count() > 120 {
                anyhow::bail!("bio must be at most 120 characters");
            }
        }
        if let Some(photo) = &self.photo_base64 {
            if photo.len() > 68_000 {
                anyhow::bail!("photo_base64 exceeds 50KB limit");
            }
        }
        Ok(())
    }
}

impl PresenceSignal {
    pub fn new(enc_pubkey_hex: String, profile: PresenceProfile, ttl_secs: u32) -> Self {
        profile.validate().ok();
        Self {
            ephemeral_id: Uuid::new_v4(),
            enc_pubkey_hex,
            profile,
            created_at_ms: mycelium_core::data::now_ms(),
            ttl_secs,
        }
    }

    pub fn is_expired(&self) -> bool {
        let age_ms = mycelium_core::data::now_ms().saturating_sub(self.created_at_ms);
        age_ms > self.ttl_secs as u64 * 1000
    }

    pub fn encode(&self) -> anyhow::Result<Vec<u8>> {
        Ok(bincode::serialize(self)?)
    }

    pub fn decode(bytes: &[u8]) -> anyhow::Result<Self> {
        Ok(bincode::deserialize(bytes)?)
    }
}

impl ProximityDirectMessage {
    pub fn encode(&self) -> anyhow::Result<Vec<u8>> {
        Ok(bincode::serialize(self)?)
    }

    pub fn decode(bytes: &[u8]) -> anyhow::Result<Self> {
        Ok(bincode::deserialize(bytes)?)
    }
}

impl ProximityMatchIntent {
    pub fn new(from_enc_pubkey_hex: String) -> Self {
        Self {
            from_enc_pubkey_hex,
            created_at_ms: mycelium_core::data::now_ms(),
            ttl_secs: 300,
        }
    }

    pub fn is_expired(&self) -> bool {
        let age_ms = mycelium_core::data::now_ms().saturating_sub(self.created_at_ms);
        age_ms > self.ttl_secs as u64 * 1000
    }

    pub fn encode(&self) -> anyhow::Result<Vec<u8>> {
        Ok(bincode::serialize(self)?)
    }

    pub fn decode(bytes: &[u8]) -> anyhow::Result<Self> {
        Ok(bincode::deserialize(bytes)?)
    }
}
