//! Reproducible build attestation (H23 / P3).

use super::bundle::MiniAppBundle;

use serde::{Deserialize, Serialize};

/// Declared in `mycelium-app.json`; signed as part of store listings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ReproducibleBuild {
    /// Public URL to build recipe (RECIPE.md, CI workflow, etc.).
    #[serde(default)]
    pub recipe_url: Option<String>,
    /// BLAKE3 hex digest of the canonical recipe file bytes.
    #[serde(default)]
    pub recipe_digest_hex: Option<String>,
    /// Must match [`content_attestation_hash`] (manifest without this block).
    #[serde(default)]
    pub attested_bundle_hash: Option<String>,
    /// Tool that produced the attestation (e.g. miniapp-sdk version).
    #[serde(default)]
    pub sdk_version: Option<String>,
}

impl ReproducibleBuild {
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(h) = &self.attested_bundle_hash {
            if h.len() != 64 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
                anyhow::bail!("attested_bundle_hash must be 64 hex chars (BLAKE3)");
            }
        }
        if let Some(d) = &self.recipe_digest_hex {
            if d.len() != 64 || !d.chars().all(|c| c.is_ascii_hexdigit()) {
                anyhow::bail!("recipe_digest_hex must be 64 hex chars (BLAKE3)");
            }
        }
        Ok(())
    }

    /// True when the manifest attests the given bundle hash.
    pub fn attests_bundle_hash(&self, bundle_hash_hex: &str) -> bool {
        self.attested_bundle_hash
            .as_deref()
            .is_some_and(|h| h.eq_ignore_ascii_case(bundle_hash_hex))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attests_matching_hash() {
        let rb = ReproducibleBuild {
            attested_bundle_hash: Some("a".repeat(64)),
            ..Default::default()
        };
        let h = "a".repeat(64);
        assert!(rb.attests_bundle_hash(&h));
    }
}

pub fn digest_file_hex(path: &std::path::Path) -> anyhow::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

/// BLAKE3 over canonical bundle bytes: manifest JSON without `reproducible_build`,
/// plus all other files sorted by path. Stable when only attestation metadata changes.
pub fn content_attestation_hash(bundle: &MiniAppBundle) -> anyhow::Result<String> {
    let mut manifest = bundle.manifest.clone();
    manifest.reproducible_build = None;
    let manifest_bytes = serde_json::to_vec(&manifest)?;

    let mut paths: Vec<&String> = bundle
        .files
        .keys()
        .filter(|p| *p != "mycelium-app.json")
        .collect();
    paths.sort();

    let mut hasher = blake3::Hasher::new();
    hasher.update(b"mycelium-miniapp-content-v1\0");
    hasher.update(&(manifest_bytes.len() as u64).to_le_bytes());
    hasher.update(&manifest_bytes);
    for path in paths {
        let data = &bundle.files[path];
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update(&(data.len() as u64).to_le_bytes());
        hasher.update(data);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod content_hash_tests {
    use super::*;
    #[test]
    fn content_hash_ignores_reproducible_block() {
        let mut bundle = MiniAppBundle {
            manifest: crate::miniapp::manifest::MiniAppManifest {
                id: "com.test.app".into(),
                name: "T".into(),
                description: "d".into(),
                version: "1.0.0".into(),
                developer: "X".into(),
                developer_peer_id: None,
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
            },
            files: std::collections::HashMap::from([(
                "index.html".into(),
                b"<html></html>".to_vec(),
            )]),
        };
        let h0 = content_attestation_hash(&bundle).unwrap();
        bundle.manifest.reproducible_build = Some(ReproducibleBuild {
            attested_bundle_hash: Some("b".repeat(64)),
            ..Default::default()
        });
        let h1 = content_attestation_hash(&bundle).unwrap();
        assert_eq!(h0, h1);
    }
}
