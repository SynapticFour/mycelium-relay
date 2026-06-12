// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use crate::behaviour::{
    make_direct_response, DirectMessageRequest, DirectMessageResponse, MeshBehaviour,
};
use crate::connectivity::{kad_action_for_mode, KadConnectivityAction};
use futures::StreamExt;
use libp2p::multiaddr::Protocol;
use libp2p::{
    gossipsub, identify, identity, kad, mdns, noise, request_response,
    swarm::{Swarm, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, SwarmBuilder,
};
use mycelium_core::bootstrap::{is_relay_peer, peer_id_from_parsed_multiaddr};
use mycelium_core::transport::{
    ConnectivityMode, MeshTransport, MessageAck, ScopeId, TransportEvent, WireMessage,
};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::time::Instant;
use tokio::sync::watch;
use tracing::warn;

type MeshEvent = <MeshBehaviour as libp2p::swarm::NetworkBehaviour>::ToSwarm;

/// Maximale Anzahl direkter Peer-Verbindungen.
const MAX_DIRECT_PEERS_DEFAULT: usize = 50;

/// Result of attempting to admit a peer under the direct-peer cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerAdmitAction {
    /// Peer admitted; no eviction needed.
    Admitted,
    /// Evict this peer before admitting the new one.
    Evict(PeerId),
    /// Reject the incoming connection.
    Reject,
}

/// Tracks direct peer connections with an LRU cap.
#[derive(Debug)]
pub struct DirectPeerCap {
    max_peers: usize,
    peers: HashSet<PeerId>,
    peer_last_seen: HashMap<PeerId, Instant>,
    pub rejections: u64,
}

impl DirectPeerCap {
    pub fn new(max_peers: usize) -> Self {
        Self {
            max_peers,
            peers: HashSet::new(),
            peer_last_seen: HashMap::new(),
            rejections: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    pub fn max_peers(&self) -> usize {
        self.max_peers
    }

    pub fn rejections(&self) -> u64 {
        self.rejections
    }

    pub fn contains(&self, peer_id: &PeerId) -> bool {
        self.peers.contains(peer_id)
    }

    pub fn touch(&mut self, peer_id: PeerId) {
        if self.peers.contains(&peer_id) {
            self.peer_last_seen.insert(peer_id, Instant::now());
        }
    }

    pub fn remove(&mut self, peer_id: &PeerId) {
        self.peers.remove(peer_id);
        self.peer_last_seen.remove(peer_id);
    }

    /// Decide whether `peer_id` may join the tracked peer set.
    pub fn admit(&mut self, peer_id: PeerId) -> PeerAdmitAction {
        if self.peers.contains(&peer_id) {
            self.touch(peer_id);
            return PeerAdmitAction::Admitted;
        }
        if self.peers.len() >= self.max_peers {
            if let Some(oldest) = self.find_least_recently_used_peer() {
                self.remove(&oldest);
                PeerAdmitAction::Evict(oldest)
            } else {
                self.rejections = self.rejections.saturating_add(1);
                PeerAdmitAction::Reject
            }
        } else {
            PeerAdmitAction::Admitted
        }
    }

    pub fn insert(&mut self, peer_id: PeerId) {
        self.peers.insert(peer_id);
        self.peer_last_seen.insert(peer_id, Instant::now());
    }

    fn find_least_recently_used_peer(&self) -> Option<PeerId> {
        self.peer_last_seen
            .iter()
            .min_by_key(|(_, &last_seen)| last_seen)
            .map(|(&peer_id, _)| peer_id)
    }
}

pub struct Libp2pTransport {
    swarm: Swarm<MeshBehaviour>,
    local_peer_id: PeerId,
    keypair: identity::Keypair,
    peer_cap: DirectPeerCap,
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
        max_peers: usize,
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
            .without_bandwidth_metrics()
            .with_behaviour(|kp, relay_behaviour| Ok(MeshBehaviour::new(kp, relay_behaviour)))?
            .build();

        swarm.listen_on(listen_addr)?;
        let _ = swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?);

        let bootstrap_peers: Vec<Multiaddr> = bootstrap_peers
            .into_iter()
            .filter_map(|s| s.parse().ok())
            .collect();

        let max_peers = if max_peers == 0 {
            MAX_DIRECT_PEERS_DEFAULT
        } else {
            max_peers
        };

        let mut transport = Self {
            swarm,
            local_peer_id,
            keypair: key,
            peer_cap: DirectPeerCap::new(max_peers),
            connectivity_rx,
            bootstrap_peers,
        };
        transport.dial_all_bootstraps();
        transport.enable_relay_inbound_listen();
        transport.setup_kad();
        Ok(transport)
    }

    fn setup_kad(&mut self) {
        self.swarm
            .behaviour_mut()
            .kad
            .set_mode(Some(kad::Mode::Client));
        for addr in &self.bootstrap_peers {
            if let Some(peer_id) = peer_id_from_parsed_multiaddr(addr) {
                self.swarm
                    .behaviour_mut()
                    .kad
                    .add_address(&peer_id, addr.clone());
                tracing::info!("kad: added bootstrap peer {peer_id}");
            }
        }
        if !self.bootstrap_peers.is_empty() {
            match self.swarm.behaviour_mut().kad.bootstrap() {
                Ok(_) => tracing::debug!("kad: bootstrap started"),
                Err(e) => tracing::warn!("kad bootstrap failed: {e}"),
            }
        }
    }

    fn kad_bootstrap(&mut self) {
        if let Err(e) = self.swarm.behaviour_mut().kad.bootstrap() {
            tracing::warn!("kad bootstrap failed: {e}");
        }
    }

    fn set_kad_mode(&mut self, mode: kad::Mode) {
        self.swarm.behaviour_mut().kad.set_mode(Some(mode));
    }

    fn start_kad_find_peer(&mut self, peer_id: PeerId) {
        tracing::debug!("kad: lookup closest peers to {peer_id}");
        self.swarm.behaviour_mut().kad.get_closest_peers(peer_id);
    }

    fn kad_maybe_dial(&mut self, peer: PeerId, addrs: impl IntoIterator<Item = Multiaddr>) {
        if peer == self.local_peer_id || is_relay_peer(&peer.to_string()) {
            return;
        }
        for addr in addrs {
            self.swarm
                .behaviour_mut()
                .kad
                .add_address(&peer, addr.clone());
            if !self.swarm.is_connected(&peer) {
                if let Err(e) = self.swarm.dial(addr) {
                    tracing::debug!("kad dial to {peer}: {e}");
                }
                break;
            }
        }
    }

    fn apply_kad_connectivity(&mut self, action: KadConnectivityAction) {
        self.set_kad_mode(kad::Mode::Client);
        match action {
            KadConnectivityAction::Activate => {
                self.dial_all_bootstraps();
                self.kad_bootstrap();
                tracing::info!("switched to internet mode — kademlia active");
            }
            KadConnectivityAction::Pause => {
                tracing::info!("switched to mesh-only mode — kademlia paused");
            }
        }
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
                tracing::warn!("dial {addr}: {e}");
            } else {
                tracing::info!("dialing {addr}");
            }
        }
    }

    fn handle_connection_established(&mut self, peer_id: PeerId) -> bool {
        if peer_id == self.local_peer_id || is_relay_peer(&peer_id.to_string()) {
            return false;
        }
        if self.peer_cap.contains(&peer_id) {
            self.peer_cap.touch(peer_id);
            return false;
        }
        match self.peer_cap.admit(peer_id) {
            PeerAdmitAction::Evict(evict) => {
                tracing::debug!(
                    "peer cap reached ({}), dropping LRU peer {evict}",
                    self.peer_cap.max_peers()
                );
                let _ = self.swarm.disconnect_peer_id(evict);
            }
            PeerAdmitAction::Reject => {
                tracing::debug!("peer cap reached, rejecting new connection from {peer_id}");
                let _ = self.swarm.disconnect_peer_id(peer_id);
                return false;
            }
            PeerAdmitAction::Admitted => {}
        }
        self.peer_cap.insert(peer_id);
        true
    }

    fn queue_dial_target(&mut self, multiaddr: String) -> anyhow::Result<()> {
        let addr: Multiaddr = multiaddr.parse()?;
        if !self.bootstrap_peers.contains(&addr) {
            self.bootstrap_peers.push(addr.clone());
            tracing::info!("queued dial target: {addr}");
        }
        self.swarm.dial(addr)?;
        Ok(())
    }

    fn handle_connectivity_change(&mut self, mode: ConnectivityMode) {
        let action = kad_action_for_mode(mode);
        self.apply_kad_connectivity(action);
        match mode {
            ConnectivityMode::Internet => {}
            ConnectivityMode::MeshOnly => {}
        }
    }
}

#[async_trait::async_trait]
impl MeshTransport for Libp2pTransport {
    fn local_peer_id(&self) -> String {
        self.local_peer_id.to_string()
    }

    fn known_peers(&self) -> Vec<String> {
        self.peer_cap
            .peers
            .iter()
            .map(ToString::to_string)
            .filter(|p| !is_relay_peer(p))
            .collect()
    }

    fn local_keypair(&self) -> Option<identity::Keypair> {
        Some(self.keypair.clone())
    }

    fn connected_peer_count(&self) -> usize {
        self.peer_cap.len()
    }

    fn max_direct_peers(&self) -> usize {
        self.peer_cap.max_peers()
    }

    fn peer_cap_rejections(&self) -> u64 {
        self.peer_cap.rejections()
    }

    async fn dial_peer(&mut self, multiaddr: String) -> anyhow::Result<()> {
        let addr: Multiaddr = multiaddr.parse()?;
        self.swarm.dial(addr)?;
        Ok(())
    }

    async fn remember_and_dial(&mut self, multiaddr: String) -> anyhow::Result<()> {
        self.queue_dial_target(multiaddr)
    }

    fn redial_stored_targets(&mut self) {
        self.dial_all_bootstraps();
    }

    async fn send_direct(&mut self, to_peer: String, message: WireMessage) -> anyhow::Result<()> {
        let peer = PeerId::from_str(&to_peer)?;
        self.peer_cap.touch(peer);
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
        if !self.swarm.behaviour_mut().gossip.subscribe(&topic)? {
            anyhow::bail!("failed to subscribe to gossip topic: {scope}");
        }
        tracing::info!("subscribed to gossip topic: {scope}");
        Ok(())
    }

    async fn unsubscribe_scope(&mut self, scope: ScopeId) -> anyhow::Result<()> {
        let topic = gossipsub::IdentTopic::new(scope.clone());
        let _ = self.swarm.behaviour_mut().gossip.unsubscribe(&topic);
        tracing::info!("unsubscribed from gossip topic: {scope}");
        Ok(())
    }

    fn kad_find_peer(&mut self, target_peer_id: &str) {
        if let Ok(peer_id) = PeerId::from_str(target_peer_id) {
            self.start_kad_find_peer(peer_id);
        }
    }

    fn kad_on_connectivity_changed(&mut self, mode: ConnectivityMode) {
        self.apply_kad_connectivity(kad_action_for_mode(mode));
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
                for (peer, addr) in list {
                    if peer == self.local_peer_id {
                        continue;
                    }
                    self.swarm.behaviour_mut().gossip.add_explicit_peer(&peer);
                    let mut dial_addr = addr.clone();
                    let has_peer = dial_addr
                        .iter()
                        .any(|p| matches!(p, Protocol::P2p(id) if id == peer));
                    if !has_peer {
                        dial_addr.push(Protocol::P2p(peer));
                    }
                    if !self.bootstrap_peers.contains(&dial_addr) {
                        self.bootstrap_peers.push(dial_addr.clone());
                    }
                    tracing::info!("mDNS discovered {peer} at {addr}, dialing {dial_addr}");
                    if let Err(e) = self.swarm.dial(dial_addr) {
                        tracing::debug!("mDNS dial to {peer}: {e}");
                    }
                }
            }
            SwarmEvent::Behaviour(MeshEvent::Mdns(mdns::Event::Expired(list))) => {
                // mDNS TTL expiry ≠ TCP disconnect; only adjust gossip if not connected.
                for (peer, _) in list {
                    if peer == self.local_peer_id || is_relay_peer(&peer.to_string()) {
                        continue;
                    }
                    if self.swarm.is_connected(&peer) {
                        continue;
                    }
                    self.swarm
                        .behaviour_mut()
                        .gossip
                        .remove_explicit_peer(&peer);
                }
            }
            SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                tracing::warn!("outgoing connection error peer={peer_id:?}: {error}");
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. }
                if self.handle_connection_established(peer_id) =>
            {
                self.swarm
                    .behaviour_mut()
                    .gossip
                    .add_explicit_peer(&peer_id);
                return Ok(Some(TransportEvent::PeerUp {
                    peer_id: peer_id.to_string(),
                }));
            }
            SwarmEvent::ConnectionClosed { peer_id, .. }
                if !is_relay_peer(&peer_id.to_string()) && self.peer_cap.contains(&peer_id) =>
            {
                self.peer_cap.remove(&peer_id);
                self.swarm
                    .behaviour_mut()
                    .gossip
                    .remove_explicit_peer(&peer_id);
                return Ok(Some(TransportEvent::PeerDown {
                    peer_id: peer_id.to_string(),
                }));
            }
            SwarmEvent::Behaviour(MeshEvent::Identify(identify::Event::Received {
                peer_id,
                ..
            })) if self.handle_connection_established(peer_id) => {
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
                self.peer_cap.touch(peer);
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
            SwarmEvent::Behaviour(MeshEvent::Kad(kad_event)) => {
                self.handle_kad_event(kad_event);
            }
            SwarmEvent::Behaviour(MeshEvent::Relay(ev)) => {
                tracing::debug!("relay client event: {ev:?}");
            }
            SwarmEvent::Behaviour(MeshEvent::Dcutr(_)) => {}
            _ => {}
        }
        Ok(None)
    }

    fn handle_kad_event(&mut self, event: kad::Event) {
        match event {
            kad::Event::OutboundQueryProgressed {
                result:
                    kad::QueryResult::Bootstrap(Ok(kad::BootstrapOk {
                        num_remaining: 0, ..
                    })),
                ..
            } => {
                tracing::info!("kad: bootstrap complete");
            }
            kad::Event::RoutingUpdated {
                peer, addresses, ..
            } => {
                tracing::debug!("kad: routing table updated, peer={peer}");
                self.kad_maybe_dial(peer, addresses.iter().cloned());
            }
            kad::Event::OutboundQueryProgressed {
                result: kad::QueryResult::GetClosestPeers(Ok(kad::GetClosestPeersOk { peers, .. })),
                ..
            } => {
                tracing::debug!("kad: found {} close peers", peers.len());
                for info in peers {
                    self.kad_maybe_dial(info.peer_id, info.addrs);
                }
            }
            _ => {}
        }
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
