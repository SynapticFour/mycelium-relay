// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! Kademlia DHT integration tests.

use libp2p::identity;
use libp2p::relay::client;
use mycelium_core::bootstrap::{peer_id_from_multiaddr, BOOTSTRAP_PEERS, RELAY_PEER_ID};
use mycelium_core::transport::ConnectivityMode;
use mycelium_node::behaviour::MeshBehaviour;
use mycelium_node::connectivity::{kad_action_for_mode, KadConnectivityAction};

#[test]
fn peer_id_from_multiaddr_parses_bootstrap() {
    for addr in BOOTSTRAP_PEERS {
        let peer_id = peer_id_from_multiaddr(addr).expect("bootstrap addr must contain /p2p/");
        assert_eq!(peer_id, RELAY_PEER_ID);
    }
}

#[tokio::test]
async fn mesh_behaviour_includes_kad() {
    let key = identity::Keypair::generate_ed25519();
    let (_relay_transport, relay_behaviour) = client::new(key.public().to_peer_id());
    let _behaviour = MeshBehaviour::new(&key, relay_behaviour);
}

#[test]
fn kad_paused_in_mesh_only_mode() {
    assert_eq!(
        kad_action_for_mode(ConnectivityMode::MeshOnly),
        KadConnectivityAction::Pause
    );
}

#[test]
fn kad_active_on_internet_mode() {
    assert_eq!(
        kad_action_for_mode(ConnectivityMode::Internet),
        KadConnectivityAction::Activate
    );
}

#[test]
fn kad_bootstrap_addrs_present() {
    assert!(
        !BOOTSTRAP_PEERS.is_empty(),
        "at least one bootstrap peer required for kad bootstrap"
    );
}
