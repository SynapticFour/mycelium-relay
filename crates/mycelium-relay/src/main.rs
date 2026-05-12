use axum::{routing::get, Json, Router};
use clap::Parser;
use libp2p::{
    futures::StreamExt,
    identify, noise, ping, relay,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, SwarmBuilder,
};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tracing::info;

#[derive(Clone)]
struct RelayState {
    peer_id: String,
    connections: Arc<RwLock<u64>>,
    reservations: Arc<RwLock<u64>>,
    uptime_started: SystemTime,
}

async fn status_handler(
    axum::extract::State(state): axum::extract::State<RelayState>,
) -> Json<serde_json::Value> {
    let conns = *state.connections.read().await;
    let reservs = *state.reservations.read().await;
    let uptime = SystemTime::now()
        .duration_since(state.uptime_started)
        .unwrap_or_default()
        .as_secs();
    Json(serde_json::json!({
        "status": "ok",
        "peer_id": state.peer_id,
        "connections": conns,
        "reservations": reservs,
        "uptime_secs": uptime,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn health_handler() -> &'static str {
    "ok"
}

async fn start_status_server(state: RelayState, port: u16) {
    let app = Router::new()
        .route("/", get(status_handler))
        .route("/health", get(health_handler))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("bind status port");
    tracing::info!("Status server listening on http://0.0.0.0:{port}");
    let _ = axum::serve(listener, app).await;
}

#[derive(Parser)]
#[command(name = "mycelium-relay")]
struct Args {
    #[arg(long, default_value = "/ip4/0.0.0.0/tcp/4001")]
    listen: String,
    #[arg(long, default_value = "/ip4/0.0.0.0/udp/4001/quic-v1")]
    listen_quic: String,
    #[arg(long)]
    keypair_path: Option<String>,
    #[arg(long, default_value_t = 8080)]
    status_port: u16,
}

#[derive(NetworkBehaviour)]
struct RelayBehaviour {
    relay: relay::Behaviour,
    identify: identify::Behaviour,
    ping: ping::Behaviour,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("mycelium_relay=info".parse()?)
                .add_directive("libp2p_relay=info".parse()?),
        )
        .init();

    let args = Args::parse();
    let keypair_path = args
        .keypair_path
        .clone()
        .unwrap_or_else(|| ".mycelium-relay/identity".to_string());
    let key = mycelium_node::load_or_create_keypair(&keypair_path)?;
    let local_peer_id = key.public().to_peer_id();

    info!("Relay Peer ID: {local_peer_id}");
    info!("Bootstrap multiaddr: /ip4/<YOUR_IP>/tcp/4001/p2p/{local_peer_id}");

    let connections = Arc::new(RwLock::new(0u64));
    let reservations = Arc::new(RwLock::new(0u64));

    let relay_state = RelayState {
        peer_id: local_peer_id.to_string(),
        connections: connections.clone(),
        reservations: reservations.clone(),
        uptime_started: SystemTime::now(),
    };

    let status_port = args.status_port;
    tokio::spawn(async move {
        start_status_server(relay_state, status_port).await;
    });

    let mut swarm = SwarmBuilder::with_existing_identity(key)
        .with_tokio()
        .with_tcp(tcp::Config::default(), noise::Config::new, yamux::Config::default)?
        .with_quic()
        .with_behaviour(|key| {
            let identify = identify::Behaviour::new(identify::Config::new(
                "/mycelium/1.0.0".into(),
                key.public(),
            ));
            Ok(RelayBehaviour {
                relay: relay::Behaviour::new(
                    key.public().to_peer_id(),
                    relay::Config {
                        max_reservations: 1024,
                        max_reservations_per_peer: 4,
                        reservation_duration: Duration::from_secs(3600),
                        reservation_rate_limiters: vec![],
                        max_circuits: 1024,
                        max_circuits_per_peer: 16,
                        max_circuit_duration: Duration::from_secs(300),
                        max_circuit_bytes: 10 * 1024 * 1024,
                        circuit_src_rate_limiters: vec![],
                    },
                ),
                identify,
                ping: ping::Behaviour::new(ping::Config::new()),
            })
        })?
        .build();

    let listen_tcp: Multiaddr = args.listen.parse()?;
    let listen_quic: Multiaddr = args.listen_quic.parse()?;
    swarm.listen_on(listen_tcp)?;
    swarm.listen_on(listen_quic)?;

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => info!("Listening on {address}"),
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                let mut c = connections.write().await;
                *c += 1;
                info!("Connection from {peer_id} (total: {c})");
            }
            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                let mut c = connections.write().await;
                *c = c.saturating_sub(1);
                info!("Disconnected {peer_id} (total: {c})");
            }
            SwarmEvent::Behaviour(RelayBehaviourEvent::Relay(ev)) => match ev {
                relay::Event::ReservationReqAccepted { renewed, .. } => {
                    if !renewed {
                        let mut r = reservations.write().await;
                        *r += 1;
                    }
                }
                relay::Event::ReservationTimedOut { .. } => {
                    let mut r = reservations.write().await;
                    *r = r.saturating_sub(1);
                }
                _ => {}
            },
            SwarmEvent::Behaviour(_) => {}
            _ => {}
        }
    }
}
