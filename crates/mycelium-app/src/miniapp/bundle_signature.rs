// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! Ed25519 bundle attestation (SD-041 / SD-240).

use libp2p::identity::Keypair;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};

use super::manifest::MiniAppManifest;
use super::store::public_key_from_peer_id;

pub const BUNDLE_SIG_FILE: &str = "mycelium-bundle.sig.json";

/// Blake3 hex of bundle contents excluding the sidecar signature file (signing payload).
/// Uses sorted `(path, bytes)` tuples so ZIP metadata timestamps cannot alter the hash.
pub fn bundle_hash_for_signature(bundle_bytes: &[u8]) -> anyhow::Result<String> {
    let cursor = std::io::Cursor::new(bundle_bytes);
    let mut archive = zip::ZipArchive::new(cursor)?;
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        if name == BUNDLE_SIG_FILE {
            continue;
        }
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut buf)?;
        entries.push((name, buf));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut digest_input = Vec::new();
    for (name, buf) in entries {
        digest_input.extend_from_slice(name.as_bytes());
        digest_input.extend_from_slice(b"\0");
        digest_input.extend_from_slice(&(buf.len() as u64).to_le_bytes());
        digest_input.extend_from_slice(&buf);
    }
    Ok(blake3::hash(&digest_input).to_hex().to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BundleSignatureFile {
    pub bundle_hash: String,
    /// Ed25519 signature (hex) over UTF-8 `bundle_hash`.
    pub signature: String,
}

impl BundleSignatureFile {
    pub fn sign(bundle_bytes: &[u8], keypair: &Keypair) -> anyhow::Result<Self> {
        let bundle_hash = bundle_hash_for_signature(bundle_bytes)?;
        let sig = keypair
            .sign(bundle_hash.as_bytes())
            .map_err(|e| anyhow::anyhow!("ed25519 sign bundle hash: {e}"))?;
        Ok(Self {
            bundle_hash,
            signature: hex::encode(sig),
        })
    }

    pub fn verify(&self, bundle_bytes: &[u8], manifest: &MiniAppManifest) -> anyhow::Result<bool> {
        if self.signature.is_empty() {
            return Ok(false);
        }
        let expected_hash = match bundle_hash_for_signature(bundle_bytes) {
            Ok(h) => h,
            Err(_) => return Ok(false),
        };
        if self.bundle_hash != expected_hash {
            return Ok(false);
        }
        let Some(peer_id_str) = &manifest.developer_peer_id else {
            tracing::warn!(
                "bundle signature present but manifest {} has no developer_peer_id",
                manifest.id
            );
            return Ok(false);
        };
        let peer_id: PeerId = peer_id_str
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid developer_peer_id"))?;
        let Some(pk) = public_key_from_peer_id(&peer_id) else {
            return Ok(false);
        };
        let sig_bytes = hex::decode(self.signature.trim())?;
        Ok(pk.verify(self.bundle_hash.as_bytes(), &sig_bytes))
    }
}

pub fn parse_bundle_signature(bytes: &[u8]) -> anyhow::Result<BundleSignatureFile> {
    Ok(serde_json::from_slice(bytes)?)
}

/// Returns `Ok(true)` when a valid sidecar signature is present and verifies.
pub fn verify_bundle_developer_signature(
    bundle: &super::bundle::MiniAppBundle,
    bundle_bytes: &[u8],
) -> anyhow::Result<bool> {
    let Some(sig_bytes) = bundle.get_file(BUNDLE_SIG_FILE) else {
        return Ok(false);
    };
    let sig_file = parse_bundle_signature(sig_bytes)?;
    sig_file.verify(bundle_bytes, &bundle.manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::miniapp::manifest::Permission;

    #[test]
    fn bundle_signature_roundtrip() {
        use std::io::Write;
        use zip::write::FileOptions;

        let keypair = Keypair::generate_ed25519();
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
        let manifest_json = serde_json::to_string(&manifest).unwrap();
        let mut bundle_bytes = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut bundle_bytes));
            let opts = FileOptions::default();
            zip.start_file("mycelium-app.json", opts).unwrap();
            zip.write_all(manifest_json.as_bytes()).unwrap();
            zip.start_file("index.html", opts).unwrap();
            zip.write_all(b"<html></html>").unwrap();
            zip.finish().unwrap();
        }
        let sig = BundleSignatureFile::sign(&bundle_bytes, &keypair).unwrap();
        assert!(sig.verify(&bundle_bytes, &manifest).unwrap());
        assert!(!sig.verify(b"tampered", &manifest).unwrap());
    }
}
