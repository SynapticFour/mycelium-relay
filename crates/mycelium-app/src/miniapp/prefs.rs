//! Per-app host preferences (safe mode toggle, etc.).

use serde::{Deserialize, Serialize};
use sled::Db;

const TREE: &str = "miniapp_prefs";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppPrefs {
    pub safe_mode: bool,
}

pub fn load(db: &Db, app_id: &str) -> anyhow::Result<AppPrefs> {
    let tree = db.open_tree(TREE)?;
    Ok(match tree.get(app_id.as_bytes())? {
        Some(v) => serde_json::from_slice(&v)?,
        None => AppPrefs::default(),
    })
}

pub fn set_safe_mode(db: &Db, app_id: &str, enabled: bool) -> anyhow::Result<()> {
    let mut prefs = load(db, app_id)?;
    prefs.safe_mode = enabled;
    let tree = db.open_tree(TREE)?;
    tree.insert(app_id.as_bytes(), serde_json::to_vec(&prefs)?)?;
    Ok(())
}

pub fn effective_safe_mode(db: &Db, app_id: &str) -> anyhow::Result<bool> {
    let prefs = load(db, app_id)?;
    if prefs.safe_mode {
        return Ok(true);
    }
    let rep = super::reputation::load(db, app_id)?;
    Ok(super::reputation::forced_safe_mode(rep.score))
}
