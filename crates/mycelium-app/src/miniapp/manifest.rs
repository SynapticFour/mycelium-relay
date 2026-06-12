// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use serde::{Deserialize, Serialize};

use super::reproducible_build::ReproducibleBuild;

/// Each mini-app ships a manifest as `mycelium-app.json` at the bundle root.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MiniAppManifest {
    /// Reverse-domain id, e.g. `com.example.delivery`
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub developer: String,
    pub developer_peer_id: Option<String>,
    pub entry: String,
    pub icon_base64: Option<String>,
    pub permissions: Vec<Permission>,
    pub min_mycelium_version: String,
    pub accepts_payments: bool,
    pub payment_address: Option<String>,
    pub categories: Vec<String>,
    /// Guest runtime: `webview` (default). `wasm` is not supported yet (Cell C5).
    #[serde(default = "default_runtime")]
    pub runtime: String,
    /// If non-empty, `bulletin.post` / `bulletin.get` may only use these scopes.
    #[serde(default)]
    pub bulletin_scopes: Vec<String>,
    /// Optional reproducible-build attestation (H23).
    #[serde(default)]
    pub reproducible_build: Option<ReproducibleBuild>,
}

fn default_runtime() -> String {
    "webview".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "PascalCase")]
pub enum Permission {
    Messaging,
    /// Broadcast to mesh chat scope (separate from direct `Messaging`).
    MessagingBroadcast,
    Identity,
    Payments,
    Storage,
    BulletinRead,
    BulletinWrite,
    PeerDiscovery,
    Camera,
}

impl MiniAppManifest {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.id.is_empty() || !self.id.contains('.') {
            anyhow::bail!("app id must be reverse-domain format (e.g. com.example.app)");
        }
        if self.name.is_empty() {
            anyhow::bail!("name is required");
        }
        if self.description.chars().count() > 120 {
            anyhow::bail!("description must be ≤ 120 characters");
        }
        if self.entry.is_empty() {
            anyhow::bail!("entry point is required");
        }
        if self.runtime != "webview" {
            anyhow::bail!(
                "runtime {:?} is not supported (only \"webview\" in this release)",
                self.runtime
            );
        }
        if let Some(rb) = &self.reproducible_build {
            rb.validate()?;
        }
        Ok(())
    }

    pub fn reproducible_attests_hash(&self, bundle_hash_hex: &str) -> bool {
        self.reproducible_build
            .as_ref()
            .is_some_and(|rb| rb.attests_bundle_hash(bundle_hash_hex))
    }

    pub fn allows_bulletin_scope(&self, scope: &str) -> bool {
        if self.bulletin_scopes.is_empty() {
            return true;
        }
        self.bulletin_scopes.iter().any(|s| s == scope)
    }
}
