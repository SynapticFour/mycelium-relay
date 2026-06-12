// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use super::matcher::ProximityMatcher;
use super::matches::ProximityMatchState;
use super::presence::{
    PresenceProfile, PresenceSignal, ProximityDirectMessage, ProximityMatchIntent,
};
use super::store::ProximityStore;
use mycelium_core::crypto::EncryptionKeypair;

#[test]
fn presence_signal_expires() {
    let mut signal = PresenceSignal::new("abc".into(), PresenceProfile::default(), 1);
    signal.created_at_ms = mycelium_core::data::now_ms().saturating_sub(2_000);
    assert!(signal.is_expired());
}

#[test]
fn presence_ephemeral_id_changes() {
    let p = PresenceProfile::default();
    let s1 = PresenceSignal::new("key".into(), p.clone(), 300);
    let s2 = PresenceSignal::new("key".into(), p, 300);
    assert_ne!(s1.ephemeral_id, s2.ephemeral_id);
}

#[test]
fn proximity_store_prunes_expired() {
    let mut store = ProximityStore::new();
    let mut s = PresenceSignal::new("k".into(), PresenceProfile::default(), 0);
    s.created_at_ms = 0;
    store.insert(s);
    assert_eq!(store.prune(), 1);
    assert_eq!(store.count(), 0);
}

#[test]
fn matcher_scores_common_interests() {
    let matcher = ProximityMatcher {
        my_looking_for: None,
        my_interests: vec!["Music".into(), "Hiking".into()],
    };
    let profile = PresenceProfile {
        interests: vec!["Music".into(), "Art".into()],
        ..PresenceProfile::default()
    };
    let signal = PresenceSignal::new("k".into(), profile, 300);
    let score = matcher.score(&signal);
    assert_eq!(score.common_interests, vec!["Music".to_string()]);
    assert!(score.score >= 10);
}

#[test]
fn mutual_match_requires_both_directions() {
    let mut state = ProximityMatchState::default();
    state.record_outgoing("peer-a");
    assert!(!state.is_mutual("peer-a"));
    state.record_incoming("peer-a");
    assert!(state.is_mutual("peer-a"));
}

#[test]
fn match_intent_expires() {
    let mut intent = ProximityMatchIntent::new("abc".into());
    intent.created_at_ms = 0;
    intent.ttl_secs = 1;
    assert!(intent.is_expired());
}

#[test]
fn proximity_direct_message_not_misread_as_presence() {
    let sender = EncryptionKeypair::generate();
    let recipient = EncryptionKeypair::generate();
    let encrypted =
        mycelium_core::crypto::encrypt_for(b"hello", &recipient.public).expect("encrypt");
    let direct = ProximityDirectMessage {
        target_enc_pubkey_hex: recipient.public_hex(),
        sender_enc_pubkey_hex: sender.public_hex(),
        encrypted_payload: encrypted,
        created_at_ms: mycelium_core::data::now_ms(),
    };
    let bytes = direct.encode().expect("encode");
    assert!(ProximityDirectMessage::decode(&bytes).is_ok());
    assert!(PresenceSignal::decode(&bytes).is_err());
}
