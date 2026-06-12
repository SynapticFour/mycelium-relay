// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! Ephemeral relay identity rotation.

use async_trait::async_trait;
use libp2p::identity;
use mycelium_core::transport::{MeshTransport, TransportEvent};
use mycelium_node::{NodeConfig, NodeRunner};
use std::future;
use tempfile::tempdir;

struct StubTransport {
    local_id: String,
    keypair: identity::Keypair,
}

impl StubTransport {
    fn new() -> Self {
        let keypair = identity::Keypair::generate_ed25519();
        let local_id = keypair.public().to_peer_id().to_string();
        Self { local_id, keypair }
    }
}

#[async_trait]
impl MeshTransport for StubTransport {
    fn local_peer_id(&self) -> String {
        self.local_id.clone()
    }

    fn known_peers(&self) -> Vec<String> {
        Vec::new()
    }

    fn local_keypair(&self) -> Option<identity::Keypair> {
        Some(self.keypair.clone())
    }

    async fn dial_peer(&mut self, _multiaddr: String) -> anyhow::Result<()> {
        Ok(())
    }

    async fn remember_and_dial(&mut self, _multiaddr: String) -> anyhow::Result<()> {
        Ok(())
    }

    fn redial_stored_targets(&mut self) {}

    async fn send_direct(
        &mut self,
        _to_peer: String,
        _message: mycelium_core::transport::WireMessage,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn publish_scoped(&mut self, _scope: String, _payload: Vec<u8>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn subscribe_scope(&mut self, _scope: String) -> anyhow::Result<()> {
        Ok(())
    }

    async fn unsubscribe_scope(&mut self, _scope: String) -> anyhow::Result<()> {
        Ok(())
    }

    async fn next_event(&mut self) -> anyhow::Result<TransportEvent> {
        future::pending().await
    }
}

#[test]
fn relay_peer_id_differs_from_identity() {
    let dir = tempdir().expect("tempdir");
    let config = NodeConfig::with_defaults(dir.path().to_str().expect("temp path"));
    let transport = StubTransport::new();
    let local = transport.local_peer_id();
    let (runner, handle) =
        NodeRunner::new_with_transport(config, Box::new(transport)).expect("node runner");

    assert_ne!(runner.relay_peer_id(), local);
    assert_ne!(handle.relay_peer_id(), local);
}

#[test]
fn relay_key_rotates_after_24h() {
    let dir = tempdir().expect("tempdir");
    let config = NodeConfig::with_defaults(dir.path().to_str().expect("temp path"));
    let (runner, _) = NodeRunner::new_with_transport(config, Box::new(StubTransport::new()))
        .expect("node runner");

    runner.set_relay_keypair_age_for_test(86_401);
    let old = runner.relay_peer_id();
    runner.rotate_relay_keypair_if_due();
    assert_ne!(runner.relay_peer_id(), old);
}
