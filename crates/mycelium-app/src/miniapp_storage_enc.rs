//! At-rest encryption for mini-app KV (`miniapp_storage` tree).

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;

const ENC_PREFIX: &[u8] = b"enc1:";
const NONCE_LEN: usize = 12;

pub fn derive_record_key(master: &[u8; 32], scoped_key: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    Hkdf::<Sha256>::new(None, master)
        .expand(
            format!("mycelium-miniapp-kv:{scoped_key}").as_bytes(),
            &mut out,
        )
        .expect("hkdf expand");
    out
}

pub fn encrypt_value(master: &[u8; 32], scoped_key: &str, plaintext: &str) -> Vec<u8> {
    let key = derive_record_key(master, scoped_key);
    let cipher = ChaCha20Poly1305::new(&key.into());
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
        .expect("encrypt");
    let mut out = Vec::with_capacity(ENC_PREFIX.len() + NONCE_LEN + ct.len());
    out.extend_from_slice(ENC_PREFIX);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    out
}

pub fn decrypt_value(master: &[u8; 32], scoped_key: &str, blob: &[u8]) -> anyhow::Result<String> {
    if !blob.starts_with(ENC_PREFIX) {
        return String::from_utf8(blob.to_vec())
            .map_err(|_| anyhow::anyhow!("invalid miniapp storage utf8"));
    }
    let rest = &blob[ENC_PREFIX.len()..];
    if rest.len() < NONCE_LEN + 16 {
        anyhow::bail!("truncated encrypted miniapp value");
    }
    let (nonce_bytes, ct) = rest.split_at(NONCE_LEN);
    let key = derive_record_key(master, scoped_key);
    let cipher = ChaCha20Poly1305::new(&key.into());
    let pt = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ct)
        .map_err(|_| anyhow::anyhow!("miniapp storage decrypt failed"))?;
    String::from_utf8(pt).map_err(|_| anyhow::anyhow!("invalid decrypted utf8"))
}
