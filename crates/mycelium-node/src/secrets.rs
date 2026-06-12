// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! Load identity and encryption keys through [`mycelium_core::at_rest::SecretVault`].

use libp2p::identity;
use mycelium_core::at_rest::SecretVault;
use mycelium_core::crypto::EncryptionKeypair;
use std::path::Path;

const IDENTITY_SECRET: &str = "ed25519_identity";
const ENC_SECRET: &str = "enc_x25519";

/// Optional 32-byte master key (Android); when `None`, OS keyring or fallback is used.
pub type StorageKey = Option<[u8; 32]>;

pub fn open_vault(db_path: &str, storage_key: StorageKey) -> anyhow::Result<SecretVault> {
    SecretVault::open_with_migration(db_path, storage_key)
}

pub fn load_or_create_keypair(
    path: &str,
    storage_key: StorageKey,
) -> anyhow::Result<identity::Keypair> {
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let db_path = Path::new(path)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or(path);
    let vault = open_vault(db_path, storage_key)?;

    match vault.read_secret(IDENTITY_SECRET) {
        Ok(Some(bytes)) => return keypair_from_ed25519_secret(&bytes),
        Ok(None) => {}
        Err(e) => {
            tracing::warn!(
                "unreadable encrypted identity at {:?}: {e}; removing corrupt secret",
                vault.secrets_dir().join(format!("{IDENTITY_SECRET}.enc"))
            );
            let _ = vault.delete_secret(IDENTITY_SECRET);
        }
    }

    if let Some(legacy) = read_legacy_sled_identity(path)? {
        vault.write_secret(IDENTITY_SECRET, &legacy)?;
        remove_legacy_sled_secret(path)?;
        tracing::info!("migrated Ed25519 identity to encrypted at-rest storage");
        return keypair_from_ed25519_secret(&legacy);
    }

    let keypair = identity::Keypair::generate_ed25519();
    let secret = ed25519_secret_bytes(&keypair)?;
    vault.write_secret(IDENTITY_SECRET, &secret)?;
    tracing::info!("generated new Ed25519 identity (encrypted at rest)");
    Ok(keypair)
}

pub fn load_or_create_enc_keypair(
    db_path: &str,
    storage_key: StorageKey,
) -> anyhow::Result<EncryptionKeypair> {
    let vault = open_vault(db_path, storage_key)?;

    if let Some(bytes) = vault.read_secret(ENC_SECRET)? {
        return enc_from_secret_bytes(&bytes);
    }

    let legacy_path = Path::new(db_path).join("enc_x25519.secret");
    if legacy_path.exists() {
        let bytes = std::fs::read(&legacy_path)?;
        vault.write_secret(ENC_SECRET, &bytes)?;
        std::fs::remove_file(&legacy_path)?;
        tracing::info!("migrated X25519 enc key to encrypted at-rest storage");
        return enc_from_secret_bytes(&bytes);
    }

    let kp = EncryptionKeypair::generate();
    vault.write_secret(ENC_SECRET, &kp.secret_bytes())?;
    tracing::info!("generated new X25519 encryption keypair (encrypted at rest)");
    Ok(kp)
}

fn keypair_from_ed25519_secret(secret: &[u8]) -> anyhow::Result<identity::Keypair> {
    let mut secret_bytes = secret.to_vec();
    let sk = identity::ed25519::SecretKey::try_from_bytes(&mut secret_bytes)
        .map_err(|e| anyhow::anyhow!("invalid ed25519 secret: {e}"))?;
    Ok(identity::Keypair::from(identity::ed25519::Keypair::from(
        sk,
    )))
}

fn ed25519_secret_bytes(keypair: &identity::Keypair) -> anyhow::Result<Vec<u8>> {
    let ed = keypair
        .clone()
        .try_into_ed25519()
        .map_err(|_| anyhow::anyhow!("keypair is not ed25519"))?;
    Ok(ed.secret().as_ref().to_vec())
}

fn enc_from_secret_bytes(bytes: &[u8]) -> anyhow::Result<EncryptionKeypair> {
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid enc key length"))?;
    Ok(EncryptionKeypair::from_secret_bytes(arr))
}

fn read_legacy_sled_identity(path: &str) -> anyhow::Result<Option<Vec<u8>>> {
    let db = sled::open(path)?;
    let tree = db.open_tree("identity")?;
    Ok(tree.get(b"ed25519_secret_key")?.map(|v| v.to_vec()))
}

fn remove_legacy_sled_secret(path: &str) -> anyhow::Result<()> {
    let db = sled::open(path)?;
    let tree = db.open_tree("identity")?;
    tree.remove(b"ed25519_secret_key")?;
    tree.flush()?;
    Ok(())
}
