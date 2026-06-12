// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use crate::scheduler::LinkProfile;
use async_trait::async_trait;
use mycelium_core::transport::{MeshTransport, ScopeId, TransportEvent, WireMessage};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum SimAction {
    SendDirect {
        from_peer: String,
        to_peer: String,
        message: WireMessage,
    },
    PublishScoped {
        from_peer: String,
        scope: ScopeId,
        payload: Vec<u8>,
    },
}

pub struct SimTransport {
    local_peer: String,
    links: Arc<Mutex<HashMap<(String, String), LinkProfile>>>,
    event_rx: mpsc::UnboundedReceiver<TransportEvent>,
    action_tx: mpsc::UnboundedSender<SimAction>,
}

impl SimTransport {
    pub fn new(
        local_peer: String,
        links: Arc<Mutex<HashMap<(String, String), LinkProfile>>>,
        event_rx: mpsc::UnboundedReceiver<TransportEvent>,
        action_tx: mpsc::UnboundedSender<SimAction>,
    ) -> Self {
        Self {
            local_peer,
            links,
            event_rx,
            action_tx,
        }
    }
}

#[async_trait]
impl MeshTransport for SimTransport {
    fn local_peer_id(&self) -> String {
        self.local_peer.clone()
    }

    fn known_peers(&self) -> Vec<String> {
        let links = self.links.lock().expect("links lock poisoned");
        links
            .keys()
            .filter_map(|(a, b)| {
                if a == &self.local_peer {
                    Some(b.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    fn local_keypair(&self) -> Option<libp2p::identity::Keypair> {
        None
    }

    async fn dial_peer(&mut self, _multiaddr: String) -> anyhow::Result<()> {
        Ok(())
    }

    async fn send_direct(&mut self, to_peer: String, message: WireMessage) -> anyhow::Result<()> {
        self.action_tx.send(SimAction::SendDirect {
            from_peer: self.local_peer.clone(),
            to_peer,
            message,
        })?;
        Ok(())
    }

    async fn publish_scoped(&mut self, scope: ScopeId, payload: Vec<u8>) -> anyhow::Result<()> {
        self.action_tx.send(SimAction::PublishScoped {
            from_peer: self.local_peer.clone(),
            scope,
            payload,
        })?;
        Ok(())
    }

    async fn subscribe_scope(&mut self, _scope: ScopeId) -> anyhow::Result<()> {
        Ok(())
    }

    async fn unsubscribe_scope(&mut self, _scope: ScopeId) -> anyhow::Result<()> {
        Ok(())
    }

    async fn next_event(&mut self) -> anyhow::Result<TransportEvent> {
        self.event_rx
            .recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("sim transport event channel closed"))
    }
}
