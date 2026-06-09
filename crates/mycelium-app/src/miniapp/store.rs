use libp2p::identity::{Keypair, PeerId, PublicKey};
use mycelium_core::data::now_ms;
use serde::{Deserialize, Serialize};

use super::manifest::MiniAppManifest;

/// App store listing distributed via bulletin (scope `mycelium/appstore/v1`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppStoreListing {
    pub manifest: MiniAppManifest,
    pub bundle_hash: String,
    pub sources: Vec<AppSource>,
    pub published_at_ms: u64,
    pub updated_at_ms: u64,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AppSource {
    Peer(String),
    Url(String),
}

pub struct AppStore {
    db: sled::Db,
    installed: sled::Tree,
    listings: sled::Tree,
}

impl AppStore {
    pub fn is_app_revoked(&self, app_id: &str) -> anyhow::Result<bool> {
        super::revocation::is_revoked(&self.db, app_id)
    }

    /// Local-only revocation (tests / emergency); gossip via `AppNode::publish_app_revocation`.
    pub fn revoke_app_local(&self, app_id: &str) -> anyhow::Result<()> {
        super::revocation::revoke_app(&self.db, app_id)
    }

    pub fn record_bridge_anomaly(&self, app_id: &str) -> anyhow::Result<()> {
        super::reputation::record_bridge_anomaly(&self.db, app_id)
    }

    pub fn record_install_reputation(
        &self,
        app_id: &str,
        trust: super::install_policy::InstallTrustLevel,
    ) -> anyhow::Result<()> {
        super::reputation::record_install_trust(&self.db, app_id, trust)
    }

    pub fn policy_snapshot(&self, app_id: &str) -> anyhow::Result<super::MiniAppPolicy> {
        super::MiniAppPolicy::load_with_store(&self.db, app_id, Some(self))
    }

    pub fn ingest_revocation_gossip(
        &self,
        entry: &super::revocation::RevocationGossip,
    ) -> anyhow::Result<bool> {
        super::revocation::ingest_gossip_revocation(&self.db, entry)
    }

    /// Build a signed revocation gossip entry (curator / local node key).
    pub fn build_revocation_gossip(
        &self,
        app_id: &str,
        reason: &str,
        curator_peer_id: &str,
        keypair: &Keypair,
    ) -> anyhow::Result<super::revocation::RevocationGossip> {
        super::revocation::RevocationGossip::new_signed(
            app_id.to_string(),
            reason.to_string(),
            curator_peer_id.to_string(),
            keypair,
        )
    }

    pub fn set_user_safe_mode(&self, app_id: &str, enabled: bool) -> anyhow::Result<()> {
        super::prefs::set_safe_mode(&self.db, app_id, enabled)
    }

    pub fn effective_safe_mode(&self, app_id: &str) -> anyhow::Result<bool> {
        super::prefs::effective_safe_mode(&self.db, app_id)
    }

    pub fn record_user_report(&self, app_id: &str) -> anyhow::Result<()> {
        super::reputation::record_user_report(&self.db, app_id)?;
        Ok(())
    }

    pub fn open(db_path: &str) -> anyhow::Result<Self> {
        let db = sled::Config::new()
            .path(db_path)
            .flush_every_ms(Some(5000))
            .open()?;
        let installed = db.open_tree("installed_apps")?;
        let listings = db.open_tree("app_listings")?;
        let store = Self {
            db,
            installed,
            listings,
        };
        if let Err(e) = super::revocation::merge_bundled_defaults(&store.db) {
            tracing::warn!("bundled miniapp revocations merge failed: {e}");
        }
        Ok(store)
    }

    pub fn install(&self, bundle_bytes: &[u8]) -> anyhow::Result<MiniAppManifest> {
        let bundle = super::bundle::MiniAppBundle::load_from_bytes(bundle_bytes)?;
        if bundle.total_size() > 10 * 1024 * 1024 {
            anyhow::bail!("app bundle exceeds 10 MB limit");
        }

        let manifest = bundle.manifest.clone();
        let key = manifest.id.as_bytes();
        self.installed.insert(key, serde_json::to_vec(&manifest)?)?;

        let files_tree = self.db.open_tree(format!("app_files:{}", manifest.id))?;
        for (path, data) in &bundle.files {
            files_tree.insert(path.as_bytes(), data.as_slice())?;
        }

        tracing::info!(
            "installed mini-app: {} v{}",
            manifest.name,
            manifest.version
        );
        Ok(manifest)
    }

    pub fn uninstall(&self, app_id: &str) -> anyhow::Result<()> {
        self.installed.remove(app_id.as_bytes())?;
        let tree_name = format!("app_files:{app_id}");
        let _ = self.db.drop_tree(tree_name.as_bytes());
        Ok(())
    }

    pub fn installed_apps(&self) -> anyhow::Result<Vec<MiniAppManifest>> {
        let mut apps = Vec::new();
        for item in self.installed.iter() {
            let (_, v) = item?;
            if let Ok(m) = serde_json::from_slice::<MiniAppManifest>(&v) {
                apps.push(m);
            }
        }
        Ok(apps)
    }

    pub fn get_file(&self, app_id: &str, path: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let tree = self.db.open_tree(format!("app_files:{app_id}"))?;
        Ok(tree.get(path.as_bytes())?.map(|v| v.to_vec()))
    }

    pub fn get_manifest(&self, app_id: &str) -> anyhow::Result<Option<MiniAppManifest>> {
        match self.installed.get(app_id.as_bytes())? {
            Some(v) => Ok(Some(serde_json::from_slice(&v)?)),
            None => Ok(None),
        }
    }

    pub fn cache_listing(&self, listing: &AppStoreListing) -> anyhow::Result<()> {
        let key = listing.manifest.id.as_bytes();
        self.listings.insert(key, serde_json::to_vec(listing)?)?;
        Ok(())
    }

    pub fn browse_listings(&self) -> anyhow::Result<Vec<AppStoreListing>> {
        let mut listings = Vec::new();
        for item in self.listings.iter() {
            let (_, v) = item?;
            if let Ok(l) = serde_json::from_slice::<AppStoreListing>(&v) {
                listings.push(l);
            }
        }
        listings.sort_by_key(|l| std::cmp::Reverse(l.updated_at_ms));
        Ok(listings)
    }
}

impl AppStoreListing {
    /// Build a signed listing using the developer's libp2p identity keypair.
    pub fn new_signed(
        manifest: MiniAppManifest,
        bundle_bytes: &[u8],
        sources: Vec<AppSource>,
        keypair: &Keypair,
    ) -> anyhow::Result<Self> {
        let bundle_hash = blake3::hash(bundle_bytes).to_hex().to_string();
        let now = now_ms();
        let mut listing = Self {
            manifest,
            bundle_hash,
            sources,
            published_at_ms: now,
            updated_at_ms: now,
            signature: Vec::new(),
        };
        let bytes_to_sign = listing.signable_bytes()?;
        listing.signature = keypair
            .sign(&bytes_to_sign)
            .map_err(|e| anyhow::anyhow!("signing failed: {e}"))?;
        Ok(listing)
    }

    /// Verify Ed25519 signature against `manifest.developer_peer_id`.
    /// Returns `Ok(false)` when the signature is missing, the developer peer id is absent,
    /// the peer id cannot be mapped to an inline Ed25519 key, or verification fails.
    pub fn verify_signature(&self) -> anyhow::Result<bool> {
        if self.signature.is_empty() {
            return Ok(false);
        }
        let Some(peer_id_str) = &self.manifest.developer_peer_id else {
            tracing::warn!(
                "rejected listing {}: missing developer_peer_id",
                self.manifest.id
            );
            return Ok(false);
        };

        let peer_id: PeerId = peer_id_str
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid peer_id in manifest"))?;

        let bytes = self.signable_bytes()?;

        let Some(pk) = public_key_from_peer_id(&peer_id) else {
            tracing::warn!(
                "rejected listing {}: peer_id has no inline ed25519 pubkey",
                self.manifest.id
            );
            return Ok(false);
        };

        Ok(pk.verify(&bytes, &self.signature))
    }

    fn signable_bytes(&self) -> anyhow::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let manifest_json = serde_json::to_string(&self.manifest)?;
        bytes.extend_from_slice(manifest_json.as_bytes());
        bytes.extend_from_slice(self.bundle_hash.as_bytes());
        bytes.extend_from_slice(&self.published_at_ms.to_le_bytes());
        bytes.extend_from_slice(&self.updated_at_ms.to_le_bytes());
        bytes.extend_from_slice(&serde_json::to_vec(&self.sources)?);
        Ok(bytes)
    }
}

pub(crate) fn public_key_from_peer_id(peer_id: &PeerId) -> Option<PublicKey> {
    let mh = peer_id.as_ref();
    // Identity multihash (code 0) embeds the protobuf-encoded public key.
    if mh.code() != 0 {
        return None;
    }
    PublicKey::try_decode_protobuf(mh.digest()).ok()
}

/// Back-compat helper: builds a listing and signs it with the node keypair.
pub fn listing_from_manifest_and_bundle(
    manifest: MiniAppManifest,
    bundle_bytes: &[u8],
    sources: Vec<AppSource>,
    keypair: &Keypair,
) -> anyhow::Result<AppStoreListing> {
    AppStoreListing::new_signed(manifest, bundle_bytes, sources, keypair)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest(peer_id: Option<String>) -> MiniAppManifest {
        MiniAppManifest {
            id: "com.test.app".into(),
            name: "Test".into(),
            description: "d".into(),
            version: "0.1.0".into(),
            developer: "T".into(),
            developer_peer_id: peer_id,
            entry: "index.html".into(),
            icon_base64: None,
            permissions: vec![],
            min_mycelium_version: "0.1.0".into(),
            accepts_payments: false,
            payment_address: None,
            categories: vec![],
            runtime: "webview".into(),
            bulletin_scopes: vec![],
            reproducible_build: None,
        }
    }

    #[test]
    fn listing_sign_and_verify() {
        let keypair = Keypair::generate_ed25519();
        let peer_id = keypair.public().to_peer_id().to_string();
        let manifest = sample_manifest(Some(peer_id));
        let bundle_bytes = b"fake bundle content";
        let listing =
            AppStoreListing::new_signed(manifest, bundle_bytes, vec![], &keypair).unwrap();
        assert!(listing.verify_signature().unwrap());
    }

    #[test]
    fn tampered_listing_fails_verification() {
        let keypair = Keypair::generate_ed25519();
        let peer_id = keypair.public().to_peer_id().to_string();
        let manifest = sample_manifest(Some(peer_id));
        let mut listing =
            AppStoreListing::new_signed(manifest, b"bundle", vec![], &keypair).unwrap();
        listing.bundle_hash = "tampered_hash".into();
        assert!(!listing.verify_signature().unwrap());
    }

    #[test]
    fn listing_without_peer_id_is_rejected() {
        let keypair = Keypair::generate_ed25519();
        let manifest = sample_manifest(None);
        let listing = AppStoreListing::new_signed(manifest, b"x", vec![], &keypair).unwrap();
        assert!(!listing.verify_signature().unwrap());
    }
}
