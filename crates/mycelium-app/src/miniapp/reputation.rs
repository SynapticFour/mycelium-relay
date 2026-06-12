// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! Local per-app reputation score (H21). Not global truth — device-local policy input.

use serde::{Deserialize, Serialize};
use sled::Db;

use super::install_policy::InstallTrustLevel;

const TREE: &str = "miniapp_reputation";

pub const SCORE_DEFAULT: u8 = 50;
pub const SCORE_FORCE_SAFE: u8 = 40;
pub const SCORE_SUGGEST_SAFE: u8 = 70;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppReputation {
    pub score: u8,
    pub updated_at_ms: u64,
}

impl Default for AppReputation {
    fn default() -> Self {
        Self {
            score: SCORE_DEFAULT,
            updated_at_ms: 0,
        }
    }
}

pub fn forced_safe_mode(score: u8) -> bool {
    score < SCORE_FORCE_SAFE
}

pub fn suggest_safe_mode(score: u8) -> bool {
    (SCORE_FORCE_SAFE..SCORE_SUGGEST_SAFE).contains(&score)
}

pub fn load(db: &Db, app_id: &str) -> anyhow::Result<AppReputation> {
    let tree = db.open_tree(TREE)?;
    Ok(match tree.get(app_id.as_bytes())? {
        Some(v) => serde_json::from_slice(&v)?,
        None => AppReputation::default(),
    })
}

pub fn save(db: &Db, app_id: &str, rep: &AppReputation) -> anyhow::Result<()> {
    let tree = db.open_tree(TREE)?;
    tree.insert(app_id.as_bytes(), serde_json::to_vec(rep)?)?;
    Ok(())
}

pub fn apply_delta(db: &Db, app_id: &str, delta: i16) -> anyhow::Result<AppReputation> {
    let mut rep = load(db, app_id)?;
    let next = (rep.score as i16 + delta).clamp(0, 100) as u8;
    rep.score = next;
    rep.updated_at_ms = mycelium_core::data::now_ms();
    save(db, app_id, &rep)?;
    Ok(rep)
}

pub fn record_install_trust(db: &Db, app_id: &str, trust: InstallTrustLevel) -> anyhow::Result<()> {
    let delta = match trust {
        InstallTrustLevel::VerifiedListing => 20,
        InstallTrustLevel::MatchingListingHash => 5,
        InstallTrustLevel::SideloadOnly => -15,
        InstallTrustLevel::HashMismatch => -25,
    };
    apply_delta(db, app_id, delta)?;
    Ok(())
}

pub fn record_user_report(db: &Db, app_id: &str) -> anyhow::Result<AppReputation> {
    apply_delta(db, app_id, -30)
}

pub fn record_revocation_hit(db: &Db, app_id: &str) -> anyhow::Result<AppReputation> {
    apply_delta(db, app_id, -100)
}

pub fn record_bridge_anomaly(db: &Db, app_id: &str) -> anyhow::Result<()> {
    apply_delta(db, app_id, -25)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn install_trust_adjusts_score() {
        let dir = tempdir().unwrap();
        let db = sled::Config::new()
            .path(dir.path().join("db"))
            .open()
            .unwrap();
        record_install_trust(&db, "com.app", InstallTrustLevel::VerifiedListing).unwrap();
        let rep = load(&db, "com.app").unwrap();
        assert_eq!(rep.score, 70);
        record_revocation_hit(&db, "com.app").unwrap();
        let rep2 = load(&db, "com.app").unwrap();
        assert_eq!(rep2.score, 0);
        assert!(forced_safe_mode(rep2.score));
    }
}
