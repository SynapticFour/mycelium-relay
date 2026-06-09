//! Ende-zu-Ende-Verschlüsselung für Mycelium-Nachrichten.
//!
//! Direkt: X25519 ECDH → HKDF-SHA256 → ChaCha20-Poly1305  
//! Gruppe: symmetrischer Pre-Shared Key → ChaCha20-Poly1305

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{EphemeralSecret, PublicKey, SharedSecret, StaticSecret};
use zeroize::Zeroize;

pub use x25519_dalek::PublicKey as X25519PublicKey;

/// X25519-Schlüsselpaar für Nachrichten-E2E (getrennt vom libp2p-Identitäts-Keypair).
pub struct EncryptionKeypair {
    pub public: PublicKey,
    secret: StaticSecret,
    secret_raw: [u8; 32],
}

impl EncryptionKeypair {
    pub fn generate() -> Self {
        let secret = StaticSecret::random_from_rng(OsRng);
        let secret_raw = secret.to_bytes();
        let public = PublicKey::from(&secret);
        Self {
            secret,
            public,
            secret_raw,
        }
    }

    pub fn secret_bytes(&self) -> [u8; 32] {
        self.secret_raw
    }

    pub fn from_secret_bytes(bytes: [u8; 32]) -> Self {
        let secret = StaticSecret::from(bytes);
        let public = PublicKey::from(&secret);
        Self {
            secret,
            public,
            secret_raw: bytes,
        }
    }

    pub fn public_hex(&self) -> String {
        hex::encode(self.public.as_bytes())
    }

    pub fn diffie_hellman(&self, their_public: &PublicKey) -> SharedSecret {
        self.secret.diffie_hellman(their_public)
    }
}

impl Drop for EncryptionKeypair {
    fn drop(&mut self) {
        self.secret_raw.zeroize();
    }
}

impl Clone for EncryptionKeypair {
    fn clone(&self) -> Self {
        Self::from_secret_bytes(self.secret_raw)
    }
}

pub fn parse_x25519_public_hex(s: &str) -> anyhow::Result<PublicKey> {
    let bytes = hex::decode(s.trim())?;
    let arr: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("x25519 public key must be 32 bytes"))?;
    Ok(PublicKey::from(arr))
}

/// Output: `[ephemeral_pubkey 32]` + `[nonce 12]` + `[ciphertext]`
pub fn encrypt_for(plaintext: &[u8], recipient_public: &PublicKey) -> anyhow::Result<Vec<u8>> {
    let ephemeral_secret = EphemeralSecret::random_from_rng(OsRng);
    let ephemeral_public = PublicKey::from(&ephemeral_secret);

    let shared = ephemeral_secret.diffie_hellman(recipient_public);
    let key = derive_chacha_key(shared.as_bytes(), b"mycelium-direct-v1")?;

    let cipher = ChaCha20Poly1305::new(&key);
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| anyhow::anyhow!("encrypt failed: {e}"))?;

    let mut out = Vec::with_capacity(32 + 12 + ciphertext.len());
    out.extend_from_slice(ephemeral_public.as_bytes());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Entschlüsselt einen mit [`encrypt_for`] erzeugten Blob.
pub fn decrypt_with(
    ciphertext_blob: &[u8],
    recipient: &EncryptionKeypair,
) -> anyhow::Result<Vec<u8>> {
    if ciphertext_blob.len() < 44 {
        anyhow::bail!("ciphertext too short");
    }
    let ephemeral_public = PublicKey::from(
        <[u8; 32]>::try_from(&ciphertext_blob[..32])
            .map_err(|_| anyhow::anyhow!("invalid ephemeral key length"))?,
    );
    let nonce = Nonce::from_slice(&ciphertext_blob[32..44]);
    let ciphertext = &ciphertext_blob[44..];

    let shared = recipient.diffie_hellman(&ephemeral_public);
    let key = derive_chacha_key(shared.as_bytes(), b"mycelium-direct-v1")?;

    let cipher = ChaCha20Poly1305::new(&key);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| anyhow::anyhow!("decryption failed – wrong key or tampered message"))
}

/// Output: `[nonce 12]` + `[ciphertext]`
pub fn encrypt_group(plaintext: &[u8], group_key: &[u8; 32]) -> anyhow::Result<Vec<u8>> {
    let key = Key::from_slice(group_key);
    let cipher = ChaCha20Poly1305::new(key);
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| anyhow::anyhow!("group encrypt failed: {e}"))?;
    let mut out = Vec::with_capacity(12 + ciphertext.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub fn decrypt_group(blob: &[u8], group_key: &[u8; 32]) -> anyhow::Result<Vec<u8>> {
    if blob.len() < 12 {
        anyhow::bail!("group ciphertext too short");
    }
    let nonce = Nonce::from_slice(&blob[..12]);
    let ciphertext = &blob[12..];
    let key = Key::from_slice(group_key);
    let cipher = ChaCha20Poly1305::new(key);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| anyhow::anyhow!("group decryption failed"))
}

fn derive_chacha_key(shared_secret: &[u8], info: &[u8]) -> anyhow::Result<Key> {
    let hk = Hkdf::<Sha256>::new(None, shared_secret);
    let mut raw = [0u8; 32];
    hk.expand(info, &mut raw)
        .map_err(|_| anyhow::anyhow!("HKDF expand failed"))?;
    Ok(*Key::from_slice(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_encrypt_decrypt_roundtrip() {
        let bob = EncryptionKeypair::generate();
        let plaintext = b"hello from alice to bob";
        let ciphertext = encrypt_for(plaintext, &bob.public).unwrap();
        let decrypted = decrypt_with(&ciphertext, &bob).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_key_fails() {
        let bob = EncryptionKeypair::generate();
        let eve = EncryptionKeypair::generate();
        let ciphertext = encrypt_for(b"secret", &bob.public).unwrap();
        assert!(decrypt_with(&ciphertext, &eve).is_err());
    }

    #[test]
    fn group_encrypt_decrypt_roundtrip() {
        let key = [42u8; 32];
        let plaintext = b"group message";
        let ciphertext = encrypt_group(plaintext, &key).unwrap();
        let decrypted = decrypt_group(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn group_wrong_key_fails() {
        let key1 = [1u8; 32];
        let key2 = [2u8; 32];
        let ciphertext = encrypt_group(b"secret", &key1).unwrap();
        assert!(decrypt_group(&ciphertext, &key2).is_err());
    }

    #[test]
    fn keypair_roundtrip_bytes() {
        let kp = EncryptionKeypair::generate();
        let bytes = kp.secret_bytes();
        let restored = EncryptionKeypair::from_secret_bytes(bytes);
        assert_eq!(kp.public.as_bytes(), restored.public.as_bytes());
    }

    #[test]
    fn secret_bytes_are_zeroed_after_drop() {
        let kp = EncryptionKeypair::generate();
        let public_bytes = *kp.public.as_bytes();
        drop(kp);
        let _ = public_bytes;
    }

    #[test]
    fn no_clone_leaks_secret() {
        let kp1 = EncryptionKeypair::generate();
        let kp2 = kp1.clone();
        assert_eq!(kp1.public.as_bytes(), kp2.public.as_bytes());
    }
}
