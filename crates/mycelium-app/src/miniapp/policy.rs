//! Combined policy view for hosts (reputation + safe mode + revocation).

use serde::{Deserialize, Serialize};
use sled::Db;

use super::install_policy::html_has_inline_script;
use super::prefs;
use super::reputation::{self, AppReputation};
use super::revocation;
use super::store::AppStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiniAppPolicy {
    pub app_id: String,
    pub reputation_score: u8,
    pub safe_mode_active: bool,
    pub safe_mode_forced: bool,
    pub safe_mode_suggested: bool,
    pub user_safe_mode: bool,
    pub revoked: bool,
    pub strict_csp_eligible: bool,
}

impl MiniAppPolicy {
    pub fn load(db: &Db, app_id: &str) -> anyhow::Result<Self> {
        Self::load_with_store(db, app_id, None)
    }

    pub fn load_with_store(
        db: &Db,
        app_id: &str,
        store: Option<&AppStore>,
    ) -> anyhow::Result<Self> {
        let rep: AppReputation = reputation::load(db, app_id)?;
        let user_safe = prefs::load(db, app_id)?.safe_mode;
        let forced = reputation::forced_safe_mode(rep.score);
        let suggested = reputation::suggest_safe_mode(rep.score);
        let revoked = revocation::is_revoked(db, app_id)?;
        let strict_csp_eligible = store
            .and_then(|s| {
                let manifest = s.get_manifest(app_id).ok()??;
                let bytes = s.get_file(app_id, &manifest.entry).ok()??;
                String::from_utf8(bytes).ok()
            })
            .is_some_and(|html| !html_has_inline_script(&html));
        Ok(Self {
            app_id: app_id.to_string(),
            reputation_score: rep.score,
            safe_mode_active: user_safe || forced,
            safe_mode_forced: forced,
            safe_mode_suggested: suggested && !user_safe && !forced,
            user_safe_mode: user_safe,
            revoked,
            strict_csp_eligible,
        })
    }
}
