// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use rand::RngCore;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub key: [u8; 32],
    pub members: Vec<String>,
    pub created_at_ms: u64,
}

impl Group {
    pub fn new(name: String) -> Self {
        let mut key = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut key);
        let id = hex::encode(blake3::hash(&key).as_bytes());
        Self {
            id,
            name,
            key,
            members: Vec::new(),
            created_at_ms: mycelium_core::data::now_ms(),
        }
    }

    pub fn export_invite(&self) -> String {
        serde_json::json!({
            "type": "mycelium-group-invite",
            "version": 1,
            "id": self.id,
            "name": self.name,
            "key": hex::encode(self.key),
        })
        .to_string()
    }

    pub fn from_invite(json: &str) -> anyhow::Result<Self> {
        let v: serde_json::Value = serde_json::from_str(json)?;
        let key_hex = v["key"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing key"))?;
        let key_bytes = hex::decode(key_hex)?;
        let key: [u8; 32] = key_bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid key length"))?;
        Ok(Self {
            id: v["id"].as_str().unwrap_or("").to_string(),
            name: v["name"].as_str().unwrap_or("Group").to_string(),
            key,
            members: Vec::new(),
            created_at_ms: mycelium_core::data::now_ms(),
        })
    }

    pub fn record_peer_seen(&mut self, peer_id: String) {
        if peer_id.is_empty() {
            return;
        }
        if !self.members.iter().any(|p| p == &peer_id) {
            self.members.push(peer_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_invite_roundtrip() {
        let group = Group::new("Test Group".into());
        let invite = group.export_invite();
        let restored = Group::from_invite(&invite).expect("parse");
        assert_eq!(restored.key, group.key);
        assert_eq!(restored.name, "Test Group");
    }

    #[test]
    fn group_record_peer_seen_dedupes() {
        let mut g = Group::new("G".into());
        g.record_peer_seen("peer-a".into());
        g.record_peer_seen("peer-b".into());
        g.record_peer_seen("peer-a".into());
        assert_eq!(g.members, vec!["peer-a".to_string(), "peer-b".to_string()]);
    }
}
