// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! Shipped official mini-app listings + bundle bytes (offline store seed).

use super::install_policy::InstallTrust;
use super::manifest::MiniAppManifest;
use super::store::{AppStore, AppStoreListing};

const BUNDLED_LISTINGS_JSON: &str = include_str!("../../assets/miniapp/bundled_listings.json");

#[derive(serde::Deserialize)]
struct BundledListingsFile {
    listings: Vec<AppStoreListing>,
}

struct OfficialBundle {
    app_id: &'static str,
    bytes: &'static [u8],
}

const OFFICIAL_BUNDLES: &[OfficialBundle] = &[
    OfficialBundle {
        app_id: "network.mycelium.meshaid",
        bytes: include_bytes!("../../assets/miniapp/bundles/meshaid.mxa"),
    },
    OfficialBundle {
        app_id: "network.mycelium.meshmarket",
        bytes: include_bytes!("../../assets/miniapp/bundles/meshmarket.mxa"),
    },
    OfficialBundle {
        app_id: "network.mycelium.proximity",
        bytes: include_bytes!("../../assets/miniapp/bundles/proximity.mxa"),
    },
];

pub fn official_bundle_bytes(app_id: &str) -> Option<&'static [u8]> {
    OFFICIAL_BUNDLES
        .iter()
        .find(|b| b.app_id == app_id)
        .map(|b| b.bytes)
}

/// Merge shipped listings into the local store cache (idempotent).
pub fn merge_bundled_listings(store: &AppStore) -> anyhow::Result<usize> {
    let bundled: BundledListingsFile = serde_json::from_str(BUNDLED_LISTINGS_JSON)?;
    let mut merged = 0usize;
    for listing in bundled.listings {
        if !listing.verify_signature()? {
            tracing::warn!(
                "skipping bundled listing {} — invalid signature",
                listing.manifest.id
            );
            continue;
        }
        let existing = store
            .browse_listings()?
            .into_iter()
            .find(|l| l.manifest.id == listing.manifest.id);
        let should_write = existing
            .as_ref()
            .is_none_or(|e| listing.updated_at_ms >= e.updated_at_ms);
        if should_write {
            store.cache_listing(&listing)?;
            merged += 1;
        }
    }
    Ok(merged)
}

impl AppStore {
    /// Install a shipped official bundle when a matching signed listing is cached.
    pub fn install_official_bundle(&self, app_id: &str) -> anyhow::Result<MiniAppManifest> {
        let bytes = official_bundle_bytes(app_id)
            .ok_or_else(|| anyhow::anyhow!("no bundled bytes for {app_id}"))?;
        self.install_verified(bytes, InstallTrust::VerifiedListing, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_bundled_listings_populates_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = AppStore::open(dir.path().join("db").to_str().unwrap()).unwrap();
        let merged = merge_bundled_listings(&store).expect("merge");
        assert!(merged >= 3, "expected official listings, got {merged}");
        let rows = store.browse_listings().unwrap();
        assert!(rows.iter().any(|l| l.manifest.id == "network.mycelium.meshaid"));
        assert!(official_bundle_bytes("network.mycelium.meshaid").is_some());
    }
}
