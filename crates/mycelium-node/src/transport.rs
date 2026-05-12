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
use std::path::Path;
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
    ) -> anyhow::Result<Self> {
        let keypair_path = keypair_path.unwrap_or_else(|| ".mycelium-node/identity".to_string());
        let key = load_or_create_keypair(&keypair_path)?;
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
        Ok(transport)
    }

    fn dial_all_bootstraps(&mut self) {
        for addr in self.bootstrap_peers.clone() {
            let _ = self.swarm.dial(addr);
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
            })) => {
                if self.peers.insert(peer_id) {
                    self.swarm
                        .behaviour_mut()
                        .gossip
                        .add_explicit_peer(&peer_id);
                    return Ok(Some(TransportEvent::PeerUp {
                        peer_id: peer_id.to_string(),
                    }));
                }
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
            SwarmEvent::Behaviour(MeshEvent::Relay(_)) => {}
            SwarmEvent::Behaviour(MeshEvent::Dcutr(_)) => {}
            _ => {}
        }
        Ok(None)
    }
}

#[allow(dead_code)]
fn _assert_response_type(_: DirectMessageResponse) {}

/// Opens or creates the sled-backed identity store at `path` and returns the node's Ed25519 keypair.
pub fn load_or_create_keypair(path: &str) -> anyhow::Result<identity::Keypair> {
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let db = sled::open(path)?;
    let tree = db.open_tree("identity")?;
    if let Some(bytes) = tree.get(b"ed25519_secret_key")? {
        let mut secret_bytes = bytes.to_vec();
        let secret = identity::ed25519::SecretKey::try_from_bytes(&mut secret_bytes)
            .map_err(|e| anyhow::anyhow!("invalid stored ed25519 secret key: {e}"))?;
        let ed = identity::ed25519::Keypair::from(secret);
        Ok(identity::Keypair::from(ed))
    } else {
        let keypair = identity::Keypair::generate_ed25519();
        let ed = keypair
            .clone()
            .try_into_ed25519()
            .map_err(|_| anyhow::anyhow!("generated non-ed25519 keypair"))?;
        tree.insert(b"ed25519_secret_key", ed.secret().as_ref())?;
        tree.flush()?;
        Ok(keypair)
    }
}

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
    }
}
