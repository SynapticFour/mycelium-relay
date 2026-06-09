//! Cross-cutting mini-app security tests (H27 / P4).

use super::bundle::MiniAppBundle;
use super::bundle_scan::scan_bundle;
use super::capability_token::{
    issue_capability, mac_key_from_db_path, permission_for_method, validate_capability,
    DEFAULT_CAP_TTL_MS,
};
use super::manifest::Permission;
use super::store::AppStore;
use libp2p::identity::Keypair;
use std::io::Write;
use tempfile::tempdir;
use zip::write::FileOptions;

fn sample_zip(manifest_json: &str, entry_html: &str) -> Vec<u8> {
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
fn install_preview_rejects_revoked_app_id() {
    let dir = tempdir().unwrap();
    let store = AppStore::open(dir.path().join("db").to_str().unwrap()).unwrap();
    store.revoke_app_local("com.revoked.app").unwrap();
    let manifest = r#"{"id":"com.revoked.app","name":"T","description":"d","version":"1.0.0","developer":"X","entry":"index.html","permissions":[],"min_mycelium_version":"0.1.0","accepts_payments":false,"categories":[],"runtime":"webview","bulletin_scopes":[]}"#;
    let zip = sample_zip(manifest, "<html></html>");
    assert!(store.preview_install(&zip).is_err());
}

#[test]
fn bundle_scan_blocks_javascript_urls() {
    let manifest = r#"{"id":"com.test.app","name":"T","description":"d","version":"1.0.0","developer":"X","entry":"index.html","permissions":[],"min_mycelium_version":"0.1.0","accepts_payments":false,"categories":[],"runtime":"webview","bulletin_scopes":[]}"#;
    let zip = sample_zip(manifest, r#"<a href="javascript:void(0)">x</a>"#);
    let bundle = MiniAppBundle::load_from_bytes(&zip).unwrap();
    assert!(scan_bundle(&bundle, &zip).is_err());
}

#[test]
fn capability_required_for_identity_get() {
    let key = mac_key_from_db_path("/tmp/sec-test");
    let session = "sess-1";
    assert!(permission_for_method("identity.get").is_some());
    let cap = issue_capability(
        &key,
        "com.app",
        &Permission::Identity,
        session,
        DEFAULT_CAP_TTL_MS,
    );
    assert!(validate_capability(&key, "com.app", &Permission::Identity, session, &cap).is_ok());
}

#[test]
fn signed_revocation_gossip_ingests() {
    let dir = tempdir().unwrap();
    let store = AppStore::open(dir.path().join("db").to_str().unwrap()).unwrap();
    let kp = Keypair::generate_ed25519();
    let peer = kp.public().to_peer_id().to_string();
    let entry = store
        .build_revocation_gossip("com.bad.app", "test", &peer, &kp)
        .unwrap();
    assert!(store.ingest_revocation_gossip(&entry).unwrap());
    assert!(store.is_app_revoked("com.bad.app").unwrap());
}
