// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! Revoked mini-app IDs (Cell C3) + signed gossip revocations (H18).

use libp2p::identity::Keypair;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use sled::Db;

use super::store::public_key_from_peer_id;

const TREE: &str = "miniapp_revocations";

/// Signed revocation propagated on gossip scope `mycelium/appstore/revocations/v1`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevocationGossip {
    pub app_id: String,
    pub reason: String,
    pub revoked_at_ms: u64,
    pub curator_peer_id: String,
    pub signature: Vec<u8>,
}

impl RevocationGossip {
    pub fn new_signed(
        app_id: String,
        reason: String,
        curator_peer_id: String,
        keypair: &Keypair,
    ) -> anyhow::Result<Self> {
        let revoked_at_ms = mycelium_core::data::now_ms();
        let mut entry = Self {
            app_id,
            reason,
            revoked_at_ms,
            curator_peer_id,
            signature: vec![],
        };
        let bytes = entry.signable_bytes()?;
        entry.signature = keypair.sign(&bytes)?;
        Ok(entry)
    }

    pub fn verify_signature(&self) -> anyhow::Result<bool> {
        let peer_id: PeerId = self
            .curator_peer_id
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid curator_peer_id"))?;
        let Some(pk) = public_key_from_peer_id(&peer_id) else {
            return Ok(false);
        };
        Ok(pk.verify(&self.signable_bytes()?, &self.signature))
    }

    fn signable_bytes(&self) -> anyhow::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.app_id.as_bytes());
        bytes.extend_from_slice(self.reason.as_bytes());
        bytes.extend_from_slice(&self.revoked_at_ms.to_le_bytes());
        bytes.extend_from_slice(self.curator_peer_id.as_bytes());
        Ok(bytes)
    }
}

pub fn ingest_gossip_revocation(db: &Db, entry: &RevocationGossip) -> anyhow::Result<bool> {
    if !entry.verify_signature()? {
        return Ok(false);
    }
    revoke_app(db, &entry.app_id)?;
    super::reputation::record_revocation_hit(db, &entry.app_id)?;
    Ok(true)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RevocationSnapshot {
    pub revoked_app_ids: Vec<String>,
    pub updated_at_ms: u64,
}

impl RevocationSnapshot {
    pub fn is_revoked(&self, app_id: &str) -> bool {
        self.revoked_app_ids.iter().any(|id| id == app_id)
    }
}

pub fn load_revocations(db: &Db) -> anyhow::Result<RevocationSnapshot> {
    let tree = db.open_tree(TREE)?;
    match tree.get(b"snapshot")? {
        Some(bytes) => Ok(serde_json::from_slice(&bytes)?),
        None => Ok(RevocationSnapshot::default()),
    }
}

pub fn save_revocations(db: &Db, snapshot: &RevocationSnapshot) -> anyhow::Result<()> {
    let tree = db.open_tree(TREE)?;
    tree.insert(b"snapshot", serde_json::to_vec(snapshot)?)?;
    Ok(())
}

pub fn revoke_app(db: &Db, app_id: &str) -> anyhow::Result<()> {
    let mut snap = load_revocations(db)?;
    if !snap.revoked_app_ids.iter().any(|x| x == app_id) {
        snap.revoked_app_ids.push(app_id.to_string());
    }
    snap.updated_at_ms = mycelium_core::data::now_ms();
    save_revocations(db, &snap)
}

pub fn is_revoked(db: &Db, app_id: &str) -> anyhow::Result<bool> {
    Ok(load_revocations(db)?.is_revoked(app_id))
}

/// Merge shipped default revocation snapshot (offline baseline, P3).
pub fn merge_bundled_defaults(db: &Db) -> anyhow::Result<usize> {
    const BUNDLED: &str = include_str!("../../assets/miniapp/bundled_revocations.json");
    let bundled: RevocationSnapshot = serde_json::from_str(BUNDLED)?;
    if bundled.revoked_app_ids.is_empty() {
        return Ok(0);
    }
    let mut snap = load_revocations(db)?;
    let mut added = 0usize;
    for id in bundled.revoked_app_ids {
        if !snap.revoked_app_ids.iter().any(|x| x == &id) {
            snap.revoked_app_ids.push(id);
            added += 1;
        }
    }
    if added > 0 {
        snap.updated_at_ms = mycelium_core::data::now_ms();
        save_revocations(db, &snap)?;
    }
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity::Keypair;

    #[test]
    fn gossip_revocation_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = sled::Config::new()
            .path(dir.path().join("db"))
            .open()
            .unwrap();
        let kp = Keypair::generate_ed25519();
        let peer = kp.public().to_peer_id().to_string();
        let entry =
            RevocationGossip::new_signed("com.evil.app".into(), "malware".into(), peer, &kp)
                .unwrap();
        assert!(entry.verify_signature().unwrap());
        assert!(ingest_gossip_revocation(&db, &entry).unwrap());
        assert!(is_revoked(&db, "com.evil.app").unwrap());
    }
}
