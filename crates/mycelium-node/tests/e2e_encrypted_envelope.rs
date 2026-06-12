// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! E2E decrypt must not attach a misleading libp2p envelope signature.

use mycelium_core::crypto::{decrypt_with, encrypt_for, EncryptionKeypair};
use mycelium_core::data::Envelope;
use mycelium_core::e2e_direct_wrap;
use mycelium_core::transport::{DirectMessage, WireMessage};

#[test]
fn encrypted_direct_decrypt_has_no_mesh_signature() {
    let author = "12D3KooWAuthorPeerIdForTestOnly12";
    let recipient_enc = EncryptionKeypair::generate();
    let sender_enc = EncryptionKeypair::generate();

    let app_payload = b"hello-e2e-app-bytes".to_vec();
    let inner = e2e_direct_wrap::wrap_inner(author, &app_payload);
    let encrypted_payload = encrypt_for(&inner, &recipient_enc.public).unwrap();

    let wire = WireMessage::EncryptedDirect {
        to_peer: "12D3KooWRecipient".into(),
        sender_enc_pubkey: sender_enc.public_hex(),
        encrypted_payload,
        mesh_signature: None,
        hop_count: 0,
        max_hops: 8,
    };

    let relay_from = "12D3KooWRelayHop";
    let encrypted = match &wire {
        WireMessage::EncryptedDirect {
            encrypted_payload, ..
        } => encrypted_payload,
        _ => panic!("expected EncryptedDirect"),
    };
    let plain = decrypt_with(encrypted, &recipient_enc).unwrap();
    let (logical_from, app_bytes) = e2e_direct_wrap::unwrap_inner(&plain, relay_from);
    assert_eq!(logical_from, author);
    assert_eq!(app_bytes, app_payload);

    let envelope = Envelope::new(logical_from, Some("12D3KooWRecipient".into()), app_bytes);
    assert!(
        envelope.signature.is_none(),
        "E2E local delivery must not add a mesh signature from the recipient's libp2p key"
    );

    let dm = DirectMessage {
        envelope,
        body: "[encrypted]".into(),
    };
    assert_eq!(dm.body, "[encrypted]");
    assert_eq!(dm.envelope.from_peer, author);
}
