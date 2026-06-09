//! Optional inner framing for E2E direct ciphertext plaintexts so a recipient
//! can identify the libp2p author after multi-hop relay.
//!
//! If the magic prefix is absent, [`unwrap_inner`] treats the whole slice as the
//! app payload and uses the transport hop id as the logical author (legacy).

const MAGIC: &[u8] = b"MYCDR01\0";
const MAX_AUTHOR_BYTES: usize = 512;

/// Wraps app-level ciphertext plaintext: `MAGIC || be32(len) || author_utf8 || app`.
pub fn wrap_inner(author_peer_id: &str, app: &[u8]) -> Vec<u8> {
    let auth = author_peer_id.as_bytes();
    assert!(
        auth.len() <= MAX_AUTHOR_BYTES,
        "author peer id too long for E2E wrap"
    );
    let mut v = Vec::with_capacity(MAGIC.len() + 4 + auth.len() + app.len());
    v.extend_from_slice(MAGIC);
    v.extend_from_slice(&(auth.len() as u32).to_be_bytes());
    v.extend_from_slice(auth);
    v.extend_from_slice(app);
    v
}

/// Returns `(logical_author_peer_id, app_bytes)` for `AppMessage` decoding.
/// If `data` has no magic prefix, returns `(relay_from.clone(), data.to_vec())`.
pub fn unwrap_inner(data: &[u8], relay_from: &str) -> (String, Vec<u8>) {
    if data.len() < MAGIC.len() + 4 || !data.starts_with(MAGIC) {
        return (relay_from.to_string(), data.to_vec());
    }
    let rest = &data[MAGIC.len()..];
    let n = match rest.get(..4).and_then(|b| <[u8; 4]>::try_from(b).ok()) {
        Some(b) => u32::from_be_bytes(b) as usize,
        None => return (relay_from.to_string(), data.to_vec()),
    };
    if n > MAX_AUTHOR_BYTES || rest.len() < 4 + n {
        return (relay_from.to_string(), data.to_vec());
    }
    let author = std::str::from_utf8(&rest[4..4 + n]).unwrap_or("");
    let author = if author.is_empty() {
        relay_from.to_string()
    } else {
        author.to_string()
    };
    let app = rest[4 + n..].to_vec();
    (author, app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_unwrap_roundtrip() {
        let app = b"app-bytes";
        let w = wrap_inner("12D3KooWAuthor", app);
        let (a, p) = unwrap_inner(&w, "relay");
        assert_eq!(a, "12D3KooWAuthor");
        assert_eq!(p, app);
    }

    #[test]
    fn legacy_no_magic_uses_relay() {
        let raw = vec![1u8, 2, 3];
        let (a, p) = unwrap_inner(&raw, "relay-peer");
        assert_eq!(a, "relay-peer");
        assert_eq!(p, raw);
    }
}
