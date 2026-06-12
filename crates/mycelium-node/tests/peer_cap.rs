// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! Direct peer connection cap enforcement.

use libp2p::identity;
use libp2p::PeerId;
use mycelium_node::{DirectPeerCap, PeerAdmitAction};

#[test]
fn peer_cap_enforced() {
    let mut cap = DirectPeerCap::new(3);
    let keys: Vec<_> = (0..4)
        .map(|_| identity::Keypair::generate_ed25519())
        .collect();
    let peer_ids: Vec<PeerId> = keys.iter().map(|k| k.public().to_peer_id()).collect();

    for peer_id in &peer_ids[..3] {
        assert_eq!(cap.admit(*peer_id), PeerAdmitAction::Admitted);
        cap.insert(*peer_id);
        assert!(cap.len() <= 3);
    }

    match cap.admit(peer_ids[3]) {
        PeerAdmitAction::Evict(evicted) => {
            cap.remove(&evicted);
            cap.insert(peer_ids[3]);
        }
        other => panic!("expected LRU eviction at cap, got {other:?}"),
    }
    assert!(cap.len() <= 3);
}

#[test]
fn peer_cap_rejections_counted() {
    let mut cap = DirectPeerCap::new(0);
    let peer = identity::Keypair::generate_ed25519().public().to_peer_id();
    assert_eq!(cap.admit(peer), PeerAdmitAction::Reject);
    assert!(cap.rejections() >= 1);
}
