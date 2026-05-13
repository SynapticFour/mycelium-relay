use axum::{routing::get, Json, Router};
use clap::Parser;
use libp2p::{
    futures::StreamExt,
    identify, noise, ping, relay,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, SwarmBuilder,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::info;

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

#[derive(Clone)]
struct StatusState {
    peer_id: String,
    connections: Arc<AtomicU64>,
    reservations: Arc<AtomicU64>,
    started: Instant,
    version: &'static str,
}

async fn status(axum::extract::State(s): axum::extract::State<StatusState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "peer_id": s.peer_id,
        "connections": s.connections.load(Ordering::Relaxed),
        "reservations": s.reservations.load(Ordering::Relaxed),
        "uptime_secs": s.started.elapsed().as_secs(),
        "version": s.version,
    }))
}

async fn health() -> &'static str {
    "ok"
}

fn atomic_saturating_sub(a: &AtomicU64) {
    let _ = a.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| Some(n.saturating_sub(1)));
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
        .unwrap_or_else(|| ".mycelium-relay/identity".to_string());
    let key = mycelium_node::load_or_create_keypair(&keypair_path)?;
    let local_peer_id = key.public().to_peer_id();

    info!("Relay Peer ID: {local_peer_id}");
    info!("Bootstrap multiaddr: /dns4/mycelium-relay.fly.dev/tcp/4001/p2p/{local_peer_id}");

    let connections = Arc::new(AtomicU64::new(0));
    let reservations = Arc::new(AtomicU64::new(0));

    let state = StatusState {
        peer_id: local_peer_id.to_string(),
        connections: connections.clone(),
        reservations: reservations.clone(),
        started: Instant::now(),
        version: env!("CARGO_PKG_VERSION"),
    };
    let app = Router::new()
        .route("/", get(status))
        .route("/health", get(health))
        .with_state(state);
    let status_port = args.status_port;
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{status_port}"))
            .await
            .expect("bind status port");
        info!("Status server on http://0.0.0.0:{status_port}");
        let _ = axum::serve(listener, app).await;
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

    swarm.listen_on(args.listen.parse::<Multiaddr>()?)?;
    swarm.listen_on(args.listen_quic.parse::<Multiaddr>()?)?;

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("Listening on {address}");
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                connections.fetch_add(1, Ordering::Relaxed);
                info!("+ {peer_id} ({})", connections.load(Ordering::Relaxed));
            }
            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                atomic_saturating_sub(&connections);
                info!("- {peer_id} ({})", connections.load(Ordering::Relaxed));
            }
            SwarmEvent::Behaviour(RelayBehaviourEvent::Relay(ev)) => match ev {
                relay::Event::ReservationReqAccepted {
                    src_peer_id,
                    renewed,
                    ..
                } if !renewed => {
                    reservations.fetch_add(1, Ordering::Relaxed);
                    info!("Reservation accepted: {src_peer_id}");
                }
                relay::Event::ReservationTimedOut { src_peer_id } => {
                    atomic_saturating_sub(&reservations);
                    info!("Reservation timed out: {src_peer_id}");
                }
                relay::Event::CircuitClosed {
                    src_peer_id,
                    dst_peer_id,
                    ..
                } => {
                    info!("Circuit closed: {src_peer_id} → {dst_peer_id}");
                }
                _ => {}
            },
            SwarmEvent::Behaviour(_) => {}
            _ => {}
        }
    }
}
