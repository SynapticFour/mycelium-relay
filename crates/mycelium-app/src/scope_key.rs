// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use rand::RngCore;
use serde::{Deserialize, Serialize};

/// Symmetric key for an encrypted bulletin scope (stored locally only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeKey {
    pub scope: String,
    pub display_name: String,
    pub key: [u8; 32],
    pub added_at_ms: u64,
}

impl ScopeKey {
    pub fn new(scope: String, display_name: String) -> Self {
        let mut key = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut key);
        Self {
            scope,
            display_name,
            key,
            added_at_ms: mycelium_core::data::now_ms(),
        }
    }

    pub fn export_invite(&self) -> String {
        serde_json::json!({
            "type": "mycelium-scope-key",
            "version": 1,
            "scope": self.scope,
            "display_name": self.display_name,
            "key": hex::encode(self.key),
        })
        .to_string()
    }

    pub fn from_invite(json: &str) -> anyhow::Result<Self> {
        let v: serde_json::Value = serde_json::from_str(json)?;
        if v.get("type").and_then(|t| t.as_str()) != Some("mycelium-scope-key") {
            anyhow::bail!("invalid invite type");
        }
        let key_hex = v["key"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing key"))?;
        let key_bytes = hex::decode(key_hex)?;
        let key: [u8; 32] = key_bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid key length"))?;
        Ok(Self {
            scope: v["scope"].as_str().unwrap_or("unknown").to_string(),
            display_name: v["display_name"]
                .as_str()
                .unwrap_or("Encrypted Channel")
                .to_string(),
            key,
            added_at_ms: mycelium_core::data::now_ms(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mycelium_core::crypto::{decrypt_group, encrypt_group};

    #[test]
    fn scope_key_roundtrip() {
        let sk = ScopeKey::new("mycelium/rescue/alpha".into(), "Team Alpha".into());
        let invite = sk.export_invite();
        let restored = ScopeKey::from_invite(&invite).unwrap();
        assert_eq!(sk.key, restored.key);
        assert_eq!(sk.scope, restored.scope);
    }

    #[test]
    fn scope_key_encryption_roundtrip() {
        let sk = ScopeKey::new("test/scope".into(), "Test".into());
        let plaintext = b"hello encrypted bulletin";
        let encrypted = encrypt_group(plaintext, &sk.key).unwrap();
        let decrypted = decrypt_group(&encrypted, &sk.key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn enc1_bulletin_payload_roundtrip() {
        use crate::envelope::{AppId, AppMessage, AppPayload, BulletinPost};

        let sk = ScopeKey::new("mycelium/rescue/alpha".into(), "Team".into());
        let now = mycelium_core::data::now_ms();
        let post = BulletinPost {
            id: uuid::Uuid::new_v4(),
            from_display_name: "alice".into(),
            title: "help".into(),
            body: "need water".into(),
            scope: sk.scope.clone(),
            timestamp_ms: now,
            expires_at_ms: now + 3_600_000,
        };
        let plaintext = AppMessage {
            app_id: AppId::Bulletin,
            payload: AppPayload::Bulletin(post),
        }
        .encode()
        .unwrap();
        let encrypted = encrypt_group(&plaintext, &sk.key).unwrap();
        let mut payload = b"enc1:".to_vec();
        payload.extend_from_slice(&encrypted);
        assert!(payload.starts_with(b"enc1:"));
        let cipher = &payload[5..];
        let plain = decrypt_group(cipher, &sk.key).unwrap();
        let decoded = AppMessage::decode(&plain).unwrap();
        match decoded.payload {
            AppPayload::Bulletin(p) => assert_eq!(p.body, "need water"),
            _ => panic!("expected bulletin"),
        }
    }
}
