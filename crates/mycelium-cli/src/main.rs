use anyhow::Context;
use clap::Parser;
use mycelium_app::api::start_api_server;
use mycelium_app::node::AppNode;
use mycelium_app::notify::NoopNotifier;
use mycelium_app::storage::AppStorage;
use mycelium_core::energy::NodeState;
use mycelium_node::{ConnectivityMonitor, NodeCommand, NodeConfig, NodeRunner};
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "mycelium")]
#[command(about = "Delay-tolerant local-first mesh node")]
struct Cli {
    #[arg(long, default_value = "/ip4/0.0.0.0/tcp/0")]
    listen: String,
    #[arg(long, default_value = ".mycelium-node")]
    db: String,
    #[arg(long, default_value = "active")]
    energy_state: String,
    #[arg(long, default_value = "anon")]
    name: String,
    #[arg(long, default_value_t = 7760)]
    api_port: u16,
    /// Bootstrap relay multiaddrs (repeatable).
    #[arg(long = "bootstrap", value_name = "MULTIADDR")]
    bootstrap: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let listen_addr = cli.listen.parse().context("invalid --listen multiaddr")?;
    let connectivity = ConnectivityMonitor::new();
    ConnectivityMonitor::spawn_monitor(connectivity.mode_tx.clone());
    let config = NodeConfig {
        listen_addr,
        db_path: cli.db.clone(),
        keypair_path: None,
        forwarding_interval_ms: 500,
        sync_interval_secs: 30,
        bootstrap_peers: cli.bootstrap.clone(),
        connectivity_rx: Some(connectivity.mode_rx),
    };
    let (runner, handle) = NodeRunner::new(config)?;
    let local_peer_id = runner.local_peer_id().to_string();
    info!("local peer id: {}", local_peer_id);

    tokio::spawn(async move {
        if let Err(err) = runner.run().await {
            warn!("node loop terminated: {err:?}");
        }
    });

    handle
        .send(NodeCommand::SetEnergyState(parse_energy_state(
            &cli.energy_state,
        )?))
        .await?;

    let app_storage = Arc::new(AppStorage::open(&format!("{}/app", cli.db))?);
    let (app_node, _inbox) = AppNode::new(
        handle.clone(),
        local_peer_id.clone(),
        cli.name.clone(),
        app_storage.clone(),
        Arc::new(NoopNotifier),
        None,
    );
    let app_node = Arc::new(app_node);
    app_node.clone().start_incoming_task();
    start_api_server(app_node.clone(), app_storage.clone(), cli.api_port).await?;

    info!("api server listening on 127.0.0.1:{}", cli.api_port);
    info!("commands: /peers | /chat <peer_id> <msg> | /chat:history <peer_id> | /bulletin <scope> <title> | <body> | /bulletin:list <scope> | /mail <peer_id> <subject> | <body> | /mail:inbox | /mail:read <mail_id> | /name <display_name>");
    let stdin = io::BufReader::new(io::stdin());
    let mut lines = stdin.lines();
    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "/peers" {
            handle.send(NodeCommand::ListPeers).await?;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("/chat ") {
            let mut parts = rest.splitn(2, ' ');
            let peer = parts.next().unwrap_or_default().to_string();
            let body = parts.next().unwrap_or_default().to_string();
            if peer.is_empty() || body.is_empty() {
                warn!("usage: /chat <peer_id> <text>");
                continue;
            }
            app_node.send_chat(Some(peer), body).await?;
            continue;
        }

        if let Some(peer) = trimmed.strip_prefix("/chat:history ") {
            let history = app_storage.chat_history(peer.trim(), 50)?;
            for m in history {
                println!("[{}] {}", m.from_display_name, m.body);
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("/bulletin ") {
            let mut parts = rest.splitn(2, ' ');
            let scope = parts.next().unwrap_or_default().to_string();
            let content = parts.next().unwrap_or_default();
            let mut split = content.splitn(2, '|');
            let title = split.next().unwrap_or_default().trim().to_string();
            let body = split.next().unwrap_or_default().trim().to_string();
            if scope.is_empty() || title.is_empty() || body.is_empty() {
                warn!("usage: /bulletin <scope> <title> | <body>");
                continue;
            }
            app_node.post_bulletin(scope, title, body, 86_400).await?;
            continue;
        }

        if let Some(scope) = trimmed.strip_prefix("/bulletin:list ") {
            let posts = app_storage.bulletins_for_scope(scope.trim())?;
            for p in posts {
                println!("[{}] {} - {}", p.scope, p.title, p.body);
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("/mail ") {
            let mut parts = rest.splitn(2, ' ');
            let to_peer = parts.next().unwrap_or_default().to_string();
            let content = parts.next().unwrap_or_default();
            let mut split = content.splitn(2, '|');
            let subject = split.next().unwrap_or_default().trim().to_string();
            let body = split.next().unwrap_or_default().trim().to_string();
            if to_peer.is_empty() || subject.is_empty() || body.is_empty() {
                warn!("usage: /mail <peer_id> <subject> | <body>");
                continue;
            }
            app_node.send_mail(to_peer, subject, body, vec![]).await?;
            continue;
        }

        if trimmed == "/mail:inbox" {
            let inbox = app_storage.inbox(50)?;
            for m in inbox {
                println!("{} {} {}", m.id, m.from_display_name, m.subject);
            }
            continue;
        }

        if let Some(mail_id) = trimmed.strip_prefix("/mail:read ") {
            let id = uuid::Uuid::parse_str(mail_id.trim()).context("invalid mail id")?;
            app_storage.mark_read(&id)?;
            println!("marked as read: {}", id);
            continue;
        }

        if let Some(name) = trimmed.strip_prefix("/name ") {
            app_node.set_display_name(name.trim().to_string()).await;
            println!("name set");
            continue;
        }
        warn!("unknown command");
    }

    Ok(())
}

fn parse_energy_state(input: &str) -> anyhow::Result<NodeState> {
    match input.to_ascii_lowercase().as_str() {
        "active" => Ok(NodeState::Active),
        "intermittent" => Ok(NodeState::Intermittent),
        "passive" => Ok(NodeState::Passive),
        _ => Err(anyhow::anyhow!(
            "invalid --energy-state, expected active|intermittent|passive"
        )),
    }
}
