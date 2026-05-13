use libp2p::{
    gossipsub::{self, MessageAuthenticity},
    identify,
    mdns,
    relay,
    request_response::{self, ProtocolSupport},
    swarm::NetworkBehaviour,
    StreamProtocol,
};
use mycelium_core::data::now_ms;
use mycelium_core::transport::WireMessage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectMessageRequest {
    pub message: WireMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectMessageResponse {
    pub message_id: String,
    pub accepted: bool,
    pub received_at_ms: u64,
}

#[derive(NetworkBehaviour)]
pub struct MeshBehaviour {
    pub relay: relay::client::Behaviour,
    pub dcutr: libp2p::dcutr::Behaviour,
    pub identify: identify::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
    pub gossip: gossipsub::Behaviour,
    pub direct: request_response::cbor::Behaviour<DirectMessageRequest, DirectMessageResponse>,
}

impl MeshBehaviour {
    pub fn new(local_key: &libp2p::identity::Keypair, relay: relay::client::Behaviour) -> Self {
        let local_peer_id = local_key.public().to_peer_id();
        let gossip = gossipsub::Behaviour::new(
            MessageAuthenticity::Signed(local_key.clone()),
            gossipsub::Config::default(),
        )
        .expect("gossipsub init");

        let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id)
            .expect("mdns init");
        let direct = request_response::cbor::Behaviour::new(
            [(
                StreamProtocol::new("/mycelium/direct/1"),
                ProtocolSupport::Full,
            )],
            request_response::Config::default(),
        );
        let identify = identify::Behaviour::new(identify::Config::new(
            "mycelium/0.1".into(),
            local_key.public(),
        ));

        Self {
            relay,
            dcutr: libp2p::dcutr::Behaviour::new(local_peer_id),
            identify,
            mdns,
            gossip,
            direct,
        }
    }
}

pub fn make_direct_response(message_id: String) -> DirectMessageResponse {
    DirectMessageResponse {
        message_id,
        accepted: true,
        received_at_ms: now_ms(),
    }
}
