use async_trait::async_trait;
use mycelium_app::envelope::AppMessage;
use mycelium_app::node::AppNode;
use mycelium_app::notify::NoopNotifier;
use mycelium_app::storage::AppStorage;
use mycelium_coin::{address_from_keypair, CoinNode, CoinTransport, LocalLedger};
use mycelium_node::{NodeCommand, NodeConfig, NodeHandle, NodeRunner};
use serde_json::json;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;

pub struct AppState {
    handle: NodeHandle,
    app_node: Arc<AppNode>,
    app_storage: Arc<AppStorage>,
    coin_node: Arc<CoinNode>,
    local_peer_id: String,
    coin_identity_path: String,
}

type SharedState = Arc<RwLock<Option<AppState>>>;

#[derive(Clone)]
struct DesktopCoinTransport {
    handle: NodeHandle,
}

#[async_trait]
impl CoinTransport for DesktopCoinTransport {
    async fn broadcast_coin_inner(&self, coin_inner: Vec<u8>) -> anyhow::Result<()> {
        let payload = AppMessage::encode_coin_payload(&coin_inner)?;
        self.handle
            .send(NodeCommand::BroadcastPayload {
                scope: "mycelium/coin".to_string(),
                body: "coin:broadcast".to_string(),
                payload,
            })
            .await
    }

    async fn send_direct_coin_inner(
        &self,
        to_peer: String,
        coin_inner: Vec<u8>,
    ) -> anyhow::Result<()> {
        let payload = AppMessage::encode_coin_payload(&coin_inner)?;
        self.handle
            .send(NodeCommand::SendDirectPayload {
                to_peer,
                body: "coin:direct".to_string(),
                payload,
            })
            .await
    }
}

#[tauri::command]
async fn start_node(
    state: State<'_, SharedState>,
    db_path: String,
    display_name: String,
    bootstrap_peers: Vec<String>,
) -> Result<String, String> {
    {
        let guard = state.read().await;
        if let Some(existing) = guard.as_ref() {
            return Ok(existing.local_peer_id.clone());
        }
    }

    let config = NodeConfig {
        listen_addr: "/ip4/0.0.0.0/tcp/0".parse().expect("valid listen addr"),
        db_path: db_path.clone(),
        keypair_path: Some(format!("{db_path}/identity")),
        forwarding_interval_ms: 500,
        sync_interval_secs: 30,
        bootstrap_peers,
        connectivity_rx: None,
    };
    let (runner, handle) = NodeRunner::new(config).map_err(|e| e.to_string())?;
    let local_peer_id = runner.local_peer_id().to_string();
    tauri::async_runtime::spawn(async move {
        let _ = runner.run().await;
    });

    let app_storage = Arc::new(AppStorage::open(&format!("{db_path}/app")).map_err(|e| e.to_string())?);
    let coin_ledger = Arc::new(LocalLedger::open(&format!("{db_path}/coin")).map_err(|e| e.to_string())?);
    let identity_path = format!("{db_path}/identity");
    let coin_keypair = mycelium_node::load_or_create_keypair(&identity_path).map_err(|e| e.to_string())?;
    let coin_addr = address_from_keypair(&coin_keypair);
    let coin_transport = Arc::new(DesktopCoinTransport {
        handle: handle.clone(),
    });
    let coin_node = Arc::new(CoinNode::new(
        coin_ledger,
        coin_transport,
        coin_addr,
        local_peer_id.clone(),
        identity_path,
    ));

    let (app_node, _inbox) = AppNode::new(
        handle.clone(),
        local_peer_id.clone(),
        display_name,
        app_storage.clone(),
        Arc::new(NoopNotifier),
        Some(coin_node.clone()),
    );
    let app_node = Arc::new(app_node);
    app_node.clone().start_incoming_task();

    *state.write().await = Some(AppState {
        handle,
        app_node,
        app_storage,
        coin_node,
        local_peer_id: local_peer_id.clone(),
        coin_identity_path: format!("{db_path}/identity"),
    });

    Ok(local_peer_id)
}

#[tauri::command]
async fn get_peers(state: State<'_, SharedState>) -> Result<Vec<String>, String> {
    let guard = state.read().await;
    let Some(s) = guard.as_ref() else {
        return Ok(vec![]);
    };
    Ok(s.handle.known_peers().await)
}

#[tauri::command]
async fn get_metrics(state: State<'_, SharedState>) -> Result<serde_json::Value, String> {
    let guard = state.read().await;
    let Some(s) = guard.as_ref() else {
        return Ok(json!({}));
    };
    let m = s.handle.metrics().await;
    Ok(json!({
        "messages_forwarded": m.messages_forwarded,
        "messages_dropped_ttl": m.messages_dropped_ttl,
        "messages_dropped_queue": m.messages_dropped_queue,
        "messages_delivered_local": m.messages_delivered_local,
        "pending_queue_size": m.pending_queue_size,
        "seen_cache_size": m.seen_cache_size,
    }))
}

#[tauri::command]
async fn send_chat(state: State<'_, SharedState>, to_peer: String, body: String) -> Result<(), String> {
    let guard = state.read().await;
    let s = guard.as_ref().ok_or_else(|| "node not started".to_string())?;
    s.app_node.send_chat(Some(to_peer), body).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_history(
    state: State<'_, SharedState>,
    peer_id: String,
    limit: u32,
) -> Result<Vec<serde_json::Value>, String> {
    let guard = state.read().await;
    let Some(s) = guard.as_ref() else {
        return Ok(vec![]);
    };
    Ok(s.app_storage
        .chat_history(&peer_id, limit as usize)
        .unwrap_or_default()
        .into_iter()
        .map(|m| {
            json!({
                "id": m.id.to_string(),
                "from_display_name": m.from_display_name,
                "body": m.body,
                "timestamp_ms": m.timestamp_ms,
            })
        })
        .collect())
}

#[tauri::command]
async fn send_mail(
    state: State<'_, SharedState>,
    to_peer: String,
    subject: String,
    body: String,
) -> Result<(), String> {
    let guard = state.read().await;
    let s = guard.as_ref().ok_or_else(|| "node not started".to_string())?;
    s.app_node
        .send_mail(to_peer, subject, body, vec![])
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn mail_inbox(state: State<'_, SharedState>, limit: u32) -> Result<Vec<serde_json::Value>, String> {
    let guard = state.read().await;
    let Some(s) = guard.as_ref() else {
        return Ok(vec![]);
    };
    Ok(s.app_storage
        .inbox(limit as usize)
        .unwrap_or_default()
        .into_iter()
        .map(|m| {
            json!({
                "id": m.id.to_string(),
                "from_peer": m.from_peer,
                "from_display_name": m.from_display_name,
                "subject": m.subject,
                "body": m.body,
                "timestamp_ms": m.timestamp_ms,
                "is_read": s.app_storage.is_read(&m.id).unwrap_or(false),
            })
        })
        .collect())
}

#[tauri::command]
async fn wallet_balance(state: State<'_, SharedState>) -> Result<serde_json::Value, String> {
    let guard = state.read().await;
    let Some(s) = guard.as_ref() else {
        return Ok(json!({"confirmed_muon": 0u64, "pending_muon": 0u64}));
    };
    let (confirmed, pending) = s.coin_node.balance().unwrap_or((0, 0));
    Ok(json!({"confirmed_muon": confirmed, "pending_muon": pending}))
}

#[tauri::command]
async fn wallet_address(state: State<'_, SharedState>) -> Result<String, String> {
    let guard = state.read().await;
    Ok(guard
        .as_ref()
        .map(|s| s.coin_node.local_address().to_string())
        .unwrap_or_default())
}

#[tauri::command]
async fn post_bulletin(
    state: State<'_, SharedState>,
    scope: String,
    title: String,
    body: String,
    ttl_secs: u64,
) -> Result<(), String> {
    let guard = state.read().await;
    let s = guard.as_ref().ok_or_else(|| "node not started".to_string())?;
    s.app_node
        .post_bulletin(scope, title, body, ttl_secs)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn bulletins_for_scope(
    state: State<'_, SharedState>,
    scope: String,
) -> Result<Vec<serde_json::Value>, String> {
    let guard = state.read().await;
    let Some(s) = guard.as_ref() else {
        return Ok(vec![]);
    };
    Ok(s.app_storage
        .bulletins_for_scope(&scope)
        .unwrap_or_default()
        .into_iter()
        .map(|p| {
            json!({
                "id": p.id.to_string(),
                "from_display_name": p.from_display_name,
                "title": p.title,
                "body": p.body,
                "scope": p.scope,
                "timestamp_ms": p.timestamp_ms,
                "expires_at_ms": p.expires_at_ms,
            })
        })
        .collect())
}

#[tauri::command]
async fn add_peer(state: State<'_, SharedState>, multiaddr: String) -> Result<(), String> {
    let guard = state.read().await;
    let s = guard.as_ref().ok_or_else(|| "node not started".to_string())?;
    s.handle
        .send(NodeCommand::AddBootstrapPeer { multiaddr })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn wallet_send(
    state: State<'_, SharedState>,
    to_address: String,
    amount_muon: u64,
    fee_muon: u64,
    memo: Option<String>,
) -> Result<(), String> {
    let guard = state.read().await;
    let s = guard.as_ref().ok_or_else(|| "node not started".to_string())?;
    s.coin_node
        .submit_transfer_from_identity_path(
            &s.coin_identity_path,
            to_address,
            amount_muon,
            fee_muon,
            memo,
        )
        .await
        .map_err(|e| e.to_string())
}

fn setup_event_emitter(app_handle: AppHandle, state: SharedState) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let guard = state.read().await;
            let Some(s) = guard.as_ref() else {
                continue;
            };
            let metrics = s.handle.metrics().await;
            let peers = s.handle.known_peers().await.len();
            let _ = app_handle.emit(
                "metrics-updated",
                json!({
                    "forwarded": metrics.messages_forwarded,
                    "queue": metrics.pending_queue_size,
                    "peers": peers,
                }),
            );
        }
    });
}

pub fn run() {
    let shared_state: SharedState = Arc::new(RwLock::new(None));

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(shared_state.clone())
        .setup(move |app| {
            setup_event_emitter(app.handle().clone(), shared_state.clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_node,
            get_peers,
            get_metrics,
            send_chat,
            chat_history,
            send_mail,
            mail_inbox,
            wallet_balance,
            wallet_address,
            wallet_send,
            post_bulletin,
            bulletins_for_scope,
            add_peer,
        ])
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}
