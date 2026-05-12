use libp2p::identity::PublicKey;

/// MeshCoin address = `mxc1` + base58 payload (20-byte pubkey digest + 4-byte checksum).
pub fn address_from_keypair(keypair: &libp2p::identity::Keypair) -> String {
    address_from_public_key(&keypair.public())
}

pub fn address_from_public_key(pk: &PublicKey) -> String {
    let pk_bytes = pk.encode_protobuf();
    let hash = blake3::hash(&pk_bytes);
    let payload = &hash.as_bytes()[..20];
    let checksum_full = blake3::hash(payload);
    let checksum = &checksum_full.as_bytes()[..4];
    let mut full = payload.to_vec();
    full.extend_from_slice(checksum);
    format!("mxc1{}", bs58::encode(full).into_string())
}

pub fn validate_address(addr: &str) -> bool {
    if !addr.starts_with("mxc1") {
        return false;
    }
    let encoded = &addr[4..];
    let decoded = match bs58::decode(encoded).into_vec() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if decoded.len() != 24 {
        return false;
    }
    let payload = &decoded[..20];
    let stored_checksum = &decoded[20..];
    let expected_full = blake3::hash(payload);
    let expected = &expected_full.as_bytes()[..4];
    stored_checksum == expected
}
