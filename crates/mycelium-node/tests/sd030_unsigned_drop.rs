// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! SD-030: unsigned and invalid-signature WireMessage::Data must be rejected.

use libp2p::identity;
use mycelium_core::data::Envelope;
use mycelium_core::transport::DirectMessage;
use mycelium_node::security::{
    is_gossip_relay_body, validate_data_message_signature, Sd030DropReason,
    UNSIGNED_GRACE_PERIOD_UNTIL_MS,
};

#[test]
fn unsigned_data_message_is_dropped_after_grace() {
    let mut envelope = Envelope::new("12D3KooWAuthor".into(), None, b"data".to_vec());
    envelope.signature = None;
    let message = DirectMessage {
        envelope,
        body: "hello".into(),
    };
    assert_eq!(
        validate_data_message_signature(&message, UNSIGNED_GRACE_PERIOD_UNTIL_MS + 1),
        Err(Sd030DropReason::NoSignature)
    );
}

#[test]
fn invalid_sig_message_is_dropped() {
    let keypair = identity::Keypair::generate_ed25519();
    let other_keypair = identity::Keypair::generate_ed25519();
    let claimed_author = other_keypair.public().to_peer_id().to_string();
    let mut envelope = Envelope::new(claimed_author, None, b"data".to_vec());
    envelope.sign(&keypair).unwrap();
    let message = DirectMessage {
        envelope,
        body: "hello".into(),
    };
    assert_eq!(
        validate_data_message_signature(&message, 0),
        Err(Sd030DropReason::InvalidSignature)
    );
}

#[test]
fn gossip_relay_bodies_are_exempt() {
    assert!(is_gossip_relay_body("[mycelium:group]"));
    assert!(is_gossip_relay_body("[appstore]"));
}
