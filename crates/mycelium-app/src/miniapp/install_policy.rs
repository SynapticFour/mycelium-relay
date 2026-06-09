//! Install-time trust decisions for mini-app bundles (Cell C0).

use super::bundle::MiniAppBundle;
use super::bundle_scan::scan_bundle;
use super::manifest::MiniAppManifest;
use super::reproducible_build::content_attestation_hash;
use super::store::{AppStore, AppStoreListing};

/// How the host trusts this bundle at install time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallTrust {
    /// Cached store listing: valid signature + matching `bundle_hash`.
    VerifiedListing,
    /// User explicitly accepted sideload risk in provenance UI.
    SideloadAcknowledged,
}

/// Outcome of static install preview (before writing to sled).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallTrustLevel {
    /// Listing signature OK and hash matches.
    VerifiedListing,
    /// Listing exists and hash matches; signature not re-checked here.
    MatchingListingHash,
    /// No matching listing; only installable with sideload ack.
    SideloadOnly,
    /// A listing exists but hash differs.
    HashMismatch,
}

#[derive(Debug, Clone)]
pub struct InstallPreview {
    pub manifest: MiniAppManifest,
    pub bundle_hash: String,
    pub trust_level: InstallTrustLevel,
    pub listing_signature_ok: bool,
    pub installed_version: Option<String>,
    /// Incoming bundle version is lower than installed (install blocked unless overridden).
    pub is_downgrade: bool,
    /// Entry HTML contains inline `<script>` (discouraged under strict CSP).
    pub has_inline_script: bool,
    /// Host may omit CSP `unsafe-inline` when false (H05).
    pub strict_csp_eligible: bool,
    /// Manifest `reproducible_build.attested_bundle_hash` matches content attestation hash (H23).
    pub reproducible_attested: bool,
}

pub fn bundle_hash_hex(bundle_bytes: &[u8]) -> String {
    blake3::hash(bundle_bytes).to_hex().to_string()
}

/// True when HTML contains inline `<script>` blocks (not only `src=` external).
pub fn html_has_inline_script(html: &str) -> bool {
    let lower = html.to_lowercase();
    lower.split("<script").skip(1).any(|chunk| {
        let trimmed = chunk.trim_start();
        !trimmed.starts_with("src=") && !trimmed.starts_with("src ")
    })
}

fn entry_has_inline_script(manifest: &MiniAppManifest, bundle: &MiniAppBundle) -> bool {
    let Some(bytes) = bundle.get_file(&manifest.entry) else {
        return false;
    };
    let Ok(html) = std::str::from_utf8(bytes) else {
        return false;
    };
    html_has_inline_script(html)
}

fn is_downgrade(installed: &str, incoming: &str) -> bool {
    // Simple numeric semver tuple compare (major.minor.patch).
    fn parts(v: &str) -> (u64, u64, u64) {
        let mut nums = v.split('.').map(|s| s.parse().unwrap_or(0));
        (
            nums.next().unwrap_or(0),
            nums.next().unwrap_or(0),
            nums.next().unwrap_or(0),
        )
    }
    parts(incoming) < parts(installed)
}

impl AppStore {
    /// Inspect a bundle without installing.
    pub fn preview_install(&self, bundle_bytes: &[u8]) -> anyhow::Result<InstallPreview> {
        let bundle = MiniAppBundle::load_from_bytes(bundle_bytes)?;
        scan_bundle(&bundle, bundle_bytes)?;
        if self.is_app_revoked(&bundle.manifest.id)? {
            anyhow::bail!("app id is revoked: {}", bundle.manifest.id);
        }
        if bundle.total_size() > 10 * 1024 * 1024 {
            anyhow::bail!("app bundle exceeds 10 MB limit");
        }
        let manifest = bundle.manifest.clone();
        let hash = bundle_hash_hex(bundle_bytes);
        let has_inline_script = entry_has_inline_script(&manifest, &bundle);

        let installed_version = self.get_manifest(&manifest.id)?.map(|m| m.version);

        let mut listing_signature_ok = false;
        let mut trust_level = InstallTrustLevel::SideloadOnly;

        if let Some(listing) = self.find_listing(&manifest.id)? {
            if listing.bundle_hash == hash {
                listing_signature_ok = listing.verify_signature().unwrap_or(false);
                trust_level = if listing_signature_ok {
                    InstallTrustLevel::VerifiedListing
                } else {
                    InstallTrustLevel::MatchingListingHash
                };
            } else {
                trust_level = InstallTrustLevel::HashMismatch;
            }
        }

        let is_downgrade = installed_version
            .as_ref()
            .is_some_and(|installed| is_downgrade(installed, &manifest.version));

        let content_hash = content_attestation_hash(&bundle)?;
        let reproducible_attested = manifest.reproducible_attests_hash(&content_hash);

        Ok(InstallPreview {
            manifest,
            bundle_hash: hash,
            trust_level,
            listing_signature_ok,
            installed_version,
            is_downgrade,
            has_inline_script,
            strict_csp_eligible: !has_inline_script,
            reproducible_attested,
        })
    }

    /// Install only when trust requirements are met.
    pub fn install_verified(
        &self,
        bundle_bytes: &[u8],
        trust: InstallTrust,
        allow_downgrade: bool,
    ) -> anyhow::Result<MiniAppManifest> {
        let preview = self.preview_install(bundle_bytes)?;

        match (trust, preview.trust_level) {
            (InstallTrust::VerifiedListing, InstallTrustLevel::VerifiedListing) => {}
            (InstallTrust::VerifiedListing, InstallTrustLevel::MatchingListingHash) => {
                anyhow::bail!(
                    "listing signature invalid for {}; install blocked in verified mode",
                    preview.manifest.id
                );
            }
            (InstallTrust::VerifiedListing, _) => {
                anyhow::bail!(
                    "bundle not verified against signed listing (trust={:?})",
                    preview.trust_level
                );
            }
            (InstallTrust::SideloadAcknowledged, InstallTrustLevel::HashMismatch) => {
                anyhow::bail!("bundle hash does not match cached store listing");
            }
            (InstallTrust::SideloadAcknowledged, _) => {}
        }

        if preview.is_downgrade && !allow_downgrade {
            if let Some(ref installed) = preview.installed_version {
                anyhow::bail!(
                    "downgrade blocked: installed v{installed} > incoming v{}",
                    preview.manifest.version
                );
            }
        }

        let manifest = self.install(bundle_bytes)?;
        let _ = self.record_install_reputation(&manifest.id, preview.trust_level);
        Ok(manifest)
    }

    fn find_listing(&self, app_id: &str) -> anyhow::Result<Option<AppStoreListing>> {
        Ok(self
            .browse_listings()?
            .into_iter()
            .find(|l| l.manifest.id == app_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::miniapp::manifest::Permission;
    use tempfile::tempdir;

    fn sample_zip(manifest_json: &str, entry_html: &str) -> Vec<u8> {
        use std::io::Write;
        use zip::write::FileOptions;
        let mut zip_buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
            let opts = FileOptions::default();
            zip.start_file("mycelium-app.json", opts).unwrap();
            zip.write_all(manifest_json.as_bytes()).unwrap();
            zip.start_file("index.html", opts).unwrap();
            zip.write_all(entry_html.as_bytes()).unwrap();
            zip.finish().unwrap();
        }
        zip_buf
    }

    #[test]
    fn sideload_install_without_listing() {
        let dir = tempdir().unwrap();
        let store = AppStore::open(dir.path().join("db").to_str().unwrap()).unwrap();
        let manifest = r#"{"id":"com.test.app","name":"T","description":"d","version":"1.0.0","developer":"X","entry":"index.html","permissions":[],"min_mycelium_version":"0.1.0","accepts_payments":false,"categories":[]}"#;
        let zip = sample_zip(manifest, "<html><body>ok</body></html>");
        let preview = store.preview_install(&zip).unwrap();
        assert_eq!(preview.trust_level, InstallTrustLevel::SideloadOnly);
        store
            .install_verified(&zip, InstallTrust::SideloadAcknowledged, false)
            .unwrap();
    }

    #[test]
    fn reproducible_attested_when_manifest_matches_hash() {
        let dir = tempdir().unwrap();
        let store = AppStore::open(dir.path().join("db").to_str().unwrap()).unwrap();
        let body = "<html><body></body></html>";
        let base = r#"{"id":"com.test.app","name":"T","description":"d","version":"1.0.0","developer":"X","entry":"index.html","permissions":[],"min_mycelium_version":"0.1.0","accepts_payments":false,"categories":[],"runtime":"webview","bulletin_scopes":[]}"#;
        let zip_base = sample_zip(base, body);
        let bundle = MiniAppBundle::load_from_bytes(&zip_base).unwrap();
        let content_hash = content_attestation_hash(&bundle).unwrap();
        let manifest_json = format!(
            r#"{{"id":"com.test.app","name":"T","description":"d","version":"1.0.0","developer":"X","entry":"index.html","permissions":[],"min_mycelium_version":"0.1.0","accepts_payments":false,"categories":[],"runtime":"webview","bulletin_scopes":[],"reproducible_build":{{"attested_bundle_hash":"{content_hash}"}}}}"#
        );
        let zip = sample_zip(&manifest_json, body);
        let preview = store.preview_install(&zip).unwrap();
        assert!(preview.reproducible_attested);
        assert!(preview.strict_csp_eligible);
    }

    #[test]
    fn downgrade_blocked_without_override() {
        let dir = tempdir().unwrap();
        let store = AppStore::open(dir.path().join("db").to_str().unwrap()).unwrap();
        let v1 = r#"{"id":"com.test.app","name":"T","description":"d","version":"2.0.0","developer":"X","entry":"index.html","permissions":[],"min_mycelium_version":"0.1.0","accepts_payments":false,"categories":[],"runtime":"webview","bulletin_scopes":[]}"#;
        let v0 = r#"{"id":"com.test.app","name":"T","description":"d","version":"1.0.0","developer":"X","entry":"index.html","permissions":[],"min_mycelium_version":"0.1.0","accepts_payments":false,"categories":[],"runtime":"webview","bulletin_scopes":[]}"#;
        let zip_hi = sample_zip(v1, "<html></html>");
        store
            .install_verified(&zip_hi, InstallTrust::SideloadAcknowledged, false)
            .unwrap();
        let zip_lo = sample_zip(v0, "<html></html>");
        let preview = store.preview_install(&zip_lo).unwrap();
        assert!(preview.is_downgrade);
        assert!(store
            .install_verified(&zip_lo, InstallTrust::SideloadAcknowledged, false)
            .is_err());
        assert!(store
            .install_verified(&zip_lo, InstallTrust::SideloadAcknowledged, true)
            .is_ok());
    }

    #[test]
    fn verified_install_requires_signed_listing() {
        let dir = tempdir().unwrap();
        let store = AppStore::open(dir.path().join("db").to_str().unwrap()).unwrap();
        let keypair = libp2p::identity::Keypair::generate_ed25519();
        let peer_id = keypair.public().to_peer_id().to_string();
        let manifest = MiniAppManifest {
            id: "com.test.app".into(),
            name: "T".into(),
            description: "d".into(),
            version: "1.0.0".into(),
            developer: "X".into(),
            developer_peer_id: Some(peer_id),
            entry: "index.html".into(),
            icon_base64: None,
            permissions: vec![Permission::Storage],
            min_mycelium_version: "0.1.0".into(),
            accepts_payments: false,
            payment_address: None,
            categories: vec![],
            runtime: "webview".into(),
            bulletin_scopes: vec![],
            reproducible_build: None,
        };
        let zip = {
            let json = serde_json::to_string(&manifest).unwrap();
            sample_zip(&json, "<html></html>")
        };
        let listing =
            AppStoreListing::new_signed(manifest.clone(), &zip, vec![], &keypair).unwrap();
        store.cache_listing(&listing).unwrap();
        assert!(store
            .install_verified(&zip, InstallTrust::VerifiedListing, false)
            .is_ok());
    }

    #[test]
    fn hash_mismatch_blocked_on_sideload() {
        let dir = tempdir().unwrap();
        let store = AppStore::open(dir.path().join("db").to_str().unwrap()).unwrap();
        let keypair = libp2p::identity::Keypair::generate_ed25519();
        let manifest = MiniAppManifest {
            id: "com.test.app".into(),
            name: "T".into(),
            description: "d".into(),
            version: "1.0.0".into(),
            developer: "X".into(),
            developer_peer_id: Some(keypair.public().to_peer_id().to_string()),
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
        };
        let zip_a = {
            let json = serde_json::to_string(&manifest).unwrap();
            sample_zip(&json, "<html></html>")
        };
        let zip_b = sample_zip(
            &serde_json::to_string(&manifest).unwrap(),
            "<html>tampered</html>",
        );
        let listing = AppStoreListing::new_signed(manifest, &zip_a, vec![], &keypair).unwrap();
        store.cache_listing(&listing).unwrap();
        assert!(store
            .install_verified(&zip_b, InstallTrust::SideloadAcknowledged, false)
            .is_err());
    }
}
