// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! Unsigned or invalid EncryptedDirect wire frames must be rejected at the node.

use libp2p::identity;
use mycelium_core::crypto::{encrypt_for, EncryptionKeypair};
use mycelium_core::e2e_direct_wrap;
use mycelium_core::transport::{sign_encrypted_direct, verify_encrypted_direct, WireMessage};

#[test]
fn unsigned_encrypted_direct_fails_verify() {
    let wire = WireMessage::EncryptedDirect {
        to_peer: "12D3KooWRecipient".into(),
        sender_enc_pubkey: EncryptionKeypair::generate().public_hex(),
        encrypted_payload: vec![1, 2, 3],
        mesh_signature: None,
        hop_count: 0,
        max_hops: 8,
    };
    let kp = identity::Keypair::generate_ed25519();
    let peer_id = kp.public().to_peer_id();
    assert!(!verify_encrypted_direct(&wire, &peer_id));
}

#[test]
fn signed_encrypted_direct_roundtrip() {
    let recipient = EncryptionKeypair::generate();
    let sender_kp = identity::Keypair::generate_ed25519();
    let sender_id = sender_kp.public().to_peer_id();
    let author = sender_id.to_string();
    let inner = e2e_direct_wrap::wrap_inner(&author, b"payload");
    let encrypted = encrypt_for(&inner, &recipient.public).unwrap();

    let mut wire = WireMessage::EncryptedDirect {
        to_peer: "12D3KooWRecipient".into(),
        sender_enc_pubkey: EncryptionKeypair::generate().public_hex(),
        encrypted_payload: encrypted,
        mesh_signature: None,
        hop_count: 0,
        max_hops: 8,
    };
    sign_encrypted_direct(&mut wire, &sender_kp).unwrap();
    assert!(verify_encrypted_direct(&wire, &sender_id));
}
