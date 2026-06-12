// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use crate::behaviour::{
    make_direct_response, DirectMessageRequest, DirectMessageResponse, MeshBehaviour,
};
use futures::StreamExt;
use libp2p::{
    gossipsub, identify, identity, mdns, noise, request_response,
    swarm::{Swarm, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, SwarmBuilder,
};
use mycelium_core::transport::{
    ConnectivityMode, MeshTransport, MessageAck, ScopeId, TransportEvent, WireMessage,
};
use std::{collections::HashSet, str::FromStr};
use tokio::sync::watch;
use tracing::warn;

type MeshEvent = <MeshBehaviour as libp2p::swarm::NetworkBehaviour>::ToSwarm;

pub struct Libp2pTransport {
    swarm: Swarm<MeshBehaviour>,
    local_peer_id: PeerId,
    keypair: identity::Keypair,
    peers: HashSet<PeerId>,
    connectivity_rx: Option<watch::Receiver<ConnectivityMode>>,
    bootstrap_peers: Vec<Multiaddr>,
}

impl Libp2pTransport {
    pub fn new(
        listen_addr: Multiaddr,
        keypair_path: Option<String>,
        bootstrap_peers: Vec<String>,
        connectivity_rx: Option<watch::Receiver<ConnectivityMode>>,
        storage_key: crate::secrets::StorageKey,
    ) -> anyhow::Result<Self> {
        let keypair_path = keypair_path.unwrap_or_else(|| ".mycelium-node/identity".to_string());
        let key = crate::secrets::load_or_create_keypair(&keypair_path, storage_key)?;
        let local_peer_id = key.public().to_peer_id();
        let mut swarm = SwarmBuilder::with_existing_identity(key.clone())
            .with_tokio()
            .with_tcp(
                tcp::Config::default().nodelay(true),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_quic()
            .with_relay_client(noise::Config::new, yamux::Config::default)?
            .without_bandwidth_logging()
            .with_behaviour(|kp, relay_behaviour| Ok(MeshBehaviour::new(kp, relay_behaviour)))?
            .build();

        swarm.listen_on(listen_addr)?;
        let _ = swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?);

        let bootstrap_peers: Vec<Multiaddr> = bootstrap_peers
            .into_iter()
            .filter_map(|s| s.parse().ok())
            .collect();

        let mut transport = Self {
            swarm,
            local_peer_id,
            keypair: key,
            peers: HashSet::new(),
            connectivity_rx,
            bootstrap_peers,
        };
        transport.dial_all_bootstraps();
        transport.enable_relay_inbound_listen();
        Ok(transport)
    }

    /// Listen on the public relay so other peers can reach us via `/p2p-circuit` (NAT traversal).
    fn enable_relay_inbound_listen(&mut self) {
        for addr in &self.bootstrap_peers {
            let addr_str = addr.to_string();
            if !addr_str.contains("/tcp/") {
                continue;
            }
            let listen_str = format!("{addr_str}/p2p-circuit");
            match listen_str.parse::<Multiaddr>() {
                Ok(listen_addr) => {
                    if let Err(e) = self.swarm.listen_on(listen_addr.clone()) {
                        tracing::warn!("relay inbound listen on {listen_addr}: {e}");
                    } else {
                        tracing::info!("relay inbound listen enabled on {listen_addr}");
                    }
                }
                Err(e) => tracing::warn!("invalid relay listen multiaddr {listen_str}: {e}"),
            }
            break;
        }
    }

    fn dial_all_bootstraps(&mut self) {
        for addr in self.bootstrap_peers.clone() {
            if let Err(e) = self.swarm.dial(addr.clone()) {
                tracing::warn!("failed to dial bootstrap peer {addr}: {e}");
            }
        }
    }

    fn handle_connectivity_change(&mut self, mode: ConnectivityMode) {
        match mode {
            ConnectivityMode::Internet => {
                self.dial_all_bootstraps();
            }
            ConnectivityMode::MeshOnly => {
                tracing::info!("switched to mesh-only mode (mDNS continues)");
            }
        }
    }
}

#[async_trait::async_trait]
impl MeshTransport for Libp2pTransport {
    fn local_peer_id(&self) -> String {
        self.local_peer_id.to_string()
    }

    fn known_peers(&self) -> Vec<String> {
        self.peers.iter().map(ToString::to_string).collect()
    }

    fn local_keypair(&self) -> Option<identity::Keypair> {
        Some(self.keypair.clone())
    }

    async fn dial_peer(&mut self, multiaddr: String) -> anyhow::Result<()> {
        let addr: Multiaddr = multiaddr.parse()?;
        self.swarm.dial(addr)?;
        Ok(())
    }

    async fn send_direct(&mut self, to_peer: String, message: WireMessage) -> anyhow::Result<()> {
        let peer = PeerId::from_str(&to_peer)?;
        let request = DirectMessageRequest { message };
        self.swarm
            .behaviour_mut()
            .direct
            .send_request(&peer, request);
        Ok(())
    }

    async fn publish_scoped(&mut self, scope: ScopeId, payload: Vec<u8>) -> anyhow::Result<()> {
        let topic = gossipsub::IdentTopic::new(scope);
        self.swarm.behaviour_mut().gossip.publish(topic, payload)?;
        Ok(())
    }

    async fn subscribe_scope(&mut self, scope: ScopeId) -> anyhow::Result<()> {
        let topic = gossipsub::IdentTopic::new(scope.clone());
        self.swarm.behaviour_mut().gossip.subscribe(&topic)?;
        tracing::info!("subscribed to gossip topic: {scope}");
        Ok(())
    }

    async fn unsubscribe_scope(&mut self, scope: ScopeId) -> anyhow::Result<()> {
        let topic = gossipsub::IdentTopic::new(scope.clone());
        self.swarm.behaviour_mut().gossip.unsubscribe(&topic)?;
        tracing::info!("unsubscribed from gossip topic: {scope}");
        Ok(())
    }

    async fn next_event(&mut self) -> anyhow::Result<TransportEvent> {
        loop {
            if let Some(rx) = &mut self.connectivity_rx {
                tokio::select! {
                    changed = rx.changed() => {
                        if changed.is_err() {
                            self.connectivity_rx = None;
                            continue;
                        }
                        let mode = *rx.borrow_and_update();
                        self.handle_connectivity_change(mode);
                        return Ok(TransportEvent::ConnectivityChanged { mode });
                    }
                    event = self.swarm.select_next_some() => {
                        if let Some(ev) = self.handle_swarm_event(event)? {
                            return Ok(ev);
                        }
                    }
                }
            } else {
                let event = self.swarm.select_next_some().await;
                if let Some(ev) = self.handle_swarm_event(event)? {
                    return Ok(ev);
                }
            }
        }
    }
}

impl Libp2pTransport {
    fn handle_swarm_event(
        &mut self,
        event: SwarmEvent<MeshEvent>,
    ) -> anyhow::Result<Option<TransportEvent>> {
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                return Ok(Some(TransportEvent::Listening {
                    address: address.to_string(),
                }));
            }
            SwarmEvent::Behaviour(MeshEvent::Mdns(mdns::Event::Discovered(list))) => {
                if let Some((peer, _addr)) = list.into_iter().next() {
                    self.peers.insert(peer);
                    self.swarm.behaviour_mut().gossip.add_explicit_peer(&peer);
                    return Ok(Some(TransportEvent::PeerUp {
                        peer_id: peer.to_string(),
                    }));
                }
            }
            SwarmEvent::Behaviour(MeshEvent::Mdns(mdns::Event::Expired(list))) => {
                if let Some((peer, _addr)) = list.into_iter().next() {
                    self.peers.remove(&peer);
                    self.swarm
                        .behaviour_mut()
                        .gossip
                        .remove_explicit_peer(&peer);
                    return Ok(Some(TransportEvent::PeerDown {
                        peer_id: peer.to_string(),
                    }));
                }
            }
            SwarmEvent::Behaviour(MeshEvent::Identify(identify::Event::Received {
                peer_id,
                ..
            })) if self.peers.insert(peer_id) => {
                self.swarm
                    .behaviour_mut()
                    .gossip
                    .add_explicit_peer(&peer_id);
                return Ok(Some(TransportEvent::PeerUp {
                    peer_id: peer_id.to_string(),
                }));
            }
            SwarmEvent::Behaviour(MeshEvent::Direct(request_response::Event::Message {
                peer,
                message:
                    request_response::Message::Request {
                        request, channel, ..
                    },
                ..
            })) => {
                let message_id = message_id_for_wire(&request.message);
                let response = make_direct_response(message_id);
                if let Err(err_response) = self
                    .swarm
                    .behaviour_mut()
                    .direct
                    .send_response(channel, response)
                {
                    warn!(
                        "failed to send direct response for message_id={}",
                        err_response.message_id
                    );
                }
                return Ok(Some(TransportEvent::DirectReceived {
                    from_peer: peer.to_string(),
                    message: request.message,
                }));
            }
            SwarmEvent::Behaviour(MeshEvent::Direct(request_response::Event::Message {
                peer,
                message: request_response::Message::Response { response, .. },
                ..
            })) => {
                let ack = MessageAck {
                    message_id: response.message_id,
                    accepted: response.accepted,
                };
                return Ok(Some(TransportEvent::DirectAck {
                    from_peer: peer.to_string(),
                    ack,
                }));
            }
            SwarmEvent::Behaviour(MeshEvent::Direct(
                request_response::Event::OutboundFailure { peer, error, .. },
            )) => {
                return Ok(Some(TransportEvent::SendFailure {
                    to_peer: peer.to_string(),
                    reason: error.to_string(),
                }));
            }
            SwarmEvent::Behaviour(MeshEvent::Gossip(gossipsub::Event::Message {
                propagation_source,
                message,
                ..
            })) => {
                return Ok(Some(TransportEvent::ScopedReceived {
                    from_peer: propagation_source.to_string(),
                    scope: message.topic.to_string(),
                    payload: message.data,
                }));
            }
            SwarmEvent::Behaviour(MeshEvent::Relay(ev)) => {
                tracing::debug!("relay client event: {ev:?}");
            }
            SwarmEvent::Behaviour(MeshEvent::Dcutr(_)) => {}
            _ => {}
        }
        Ok(None)
    }
}

#[allow(dead_code)]
fn _assert_response_type(_: DirectMessageResponse) {}

fn message_id_for_wire(message: &WireMessage) -> String {
    match message {
        WireMessage::Data(msg) => msg.envelope.id.0.clone(),
        WireMessage::SyncBloom { bloom, count } => {
            format!("sync_bloom:{}:{}", count, blake3::hash(bloom).to_hex())
        }
        WireMessage::SyncIds { ids } => format!(
            "sync_ids:{}",
            blake3::hash(ids.join(",").as_bytes()).to_hex()
        ),
        WireMessage::SyncRequest { ids } => {
            format!(
                "sync_req:{}",
                blake3::hash(ids.join(",").as_bytes()).to_hex()
            )
        }
        WireMessage::SyncData { messages } => {
            let joined = messages
                .iter()
                .map(|m| m.envelope.id.0.as_str())
                .collect::<Vec<_>>()
                .join(",");
            format!("sync_data:{}", blake3::hash(joined.as_bytes()).to_hex())
        }
        WireMessage::ScopeAnnounce { scopes } => {
            format!(
                "scope_announce:{}",
                blake3::hash(scopes.join(",").as_bytes()).to_hex()
            )
        }
        WireMessage::EncryptedDirect {
            encrypted_payload, ..
        } => {
            format!("enc_direct:{}", blake3::hash(encrypted_payload).to_hex())
        }
        WireMessage::EncryptedGroup {
            group_id,
            encrypted_payload,
        } => {
            format!(
                "enc_group:{}:{}",
                group_id,
                blake3::hash(encrypted_payload).to_hex()
            )
        }
        WireMessage::PeerInfo { enc_pubkey_hex, .. } => {
            format!(
                "peer_info:{}",
                blake3::hash(enc_pubkey_hex.as_bytes()).to_hex()
            )
        }
    }
}
