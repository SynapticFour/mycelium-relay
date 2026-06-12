// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! At-rest secret storage and legacy migration.

use libp2p::identity;
use mycelium_core::at_rest::SecretVault;
use mycelium_node::secrets;

#[test]
fn identity_encrypted_not_plaintext_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().to_str().unwrap();
    let identity_path = format!("{db}/identity");
    let _kp = secrets::load_or_create_keypair(&identity_path, None).unwrap();
    let enc_path = dir.path().join(".secrets/ed25519_identity.enc");
    assert!(enc_path.exists(), "expected encrypted identity blob");
    let blob = std::fs::read(&enc_path).unwrap();
    let ed_secret = identity::Keypair::generate_ed25519()
        .try_into_ed25519()
        .unwrap()
        .secret();
    assert!(
        !blob.windows(32).any(|w| w == ed_secret.as_ref()),
        "raw ed25519 secret must not appear in ciphertext file"
    );
}

#[test]
fn migrates_legacy_sled_identity() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().to_str().unwrap();
    let identity_path = format!("{db}/identity");
    std::fs::create_dir_all(&identity_path).unwrap();
    let db_sled = sled::open(&identity_path).unwrap();
    let tree = db_sled.open_tree("identity").unwrap();
    let kp = identity::Keypair::generate_ed25519();
    let ed = kp.clone().try_into_ed25519().unwrap();
    tree.insert(b"ed25519_secret_key", ed.secret().as_ref())
        .unwrap();
    tree.flush().unwrap();
    drop(tree);
    drop(db_sled);

    let loaded = secrets::load_or_create_keypair(&identity_path, None).unwrap();
    assert_eq!(
        loaded.public().to_peer_id(),
        kp.public().to_peer_id(),
        "migrated key must match legacy"
    );
    assert!(
        dir.path().join(".secrets/ed25519_identity.enc").exists(),
        "expected encrypted blob after migration"
    );
}

#[test]
fn explicit_storage_key_used_on_android_path() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().to_str().unwrap();
    let key: [u8; 32] = [7u8; 32];
    let identity_path = format!("{db}/identity");
    let kp1 = secrets::load_or_create_keypair(&identity_path, Some(key)).unwrap();
    let kp2 = secrets::load_or_create_keypair(&identity_path, Some(key)).unwrap();
    assert_eq!(kp1.public().to_peer_id(), kp2.public().to_peer_id());
}

#[test]
fn recreates_enc_key_when_ciphertext_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().to_str().unwrap();
    let vault = SecretVault::open_with_migration(db, None).unwrap();
    vault.write_secret("enc_x25519", &[0u8; 32]).unwrap();
    std::fs::write(
        vault.secrets_dir().join("enc_x25519.enc"),
        b"not-valid-chacha-ciphertext",
    )
    .unwrap();

    let kp = secrets::load_or_create_enc_keypair(db, None).unwrap();
    assert_eq!(kp.secret_bytes().len(), 32);
    assert!(vault.secrets_dir().join("enc_x25519.enc").exists());
    let blob = std::fs::read(vault.secrets_dir().join("enc_x25519.enc")).unwrap();
    assert_ne!(blob, b"not-valid-chacha-ciphertext");
}

#[test]
fn panic_wipe_removes_db_including_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path();
    let vault = SecretVault::open_with_migration(db_path.to_str().unwrap(), None).unwrap();
    vault.write_secret("identity", b"test").unwrap();
    assert!(vault.secrets_dir().join("identity.enc").exists()); // test secret name "identity"
    std::fs::remove_dir_all(db_path).unwrap();
    assert!(!db_path.exists());
}
