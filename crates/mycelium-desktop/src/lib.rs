// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use async_trait::async_trait;
use mycelium_app::contacts::{Contact, ContactStatus};
use mycelium_app::envelope::AppMessage;
use mycelium_app::groups::Group;
use mycelium_app::node::{AppInbox, AppNode};
use mycelium_app::notify::NotificationSink;
use mycelium_app::storage::AppStorage;
use mycelium_coin::{address_from_keypair, CoinNode, CoinTransport, LocalLedger};
use mycelium_core::bootstrap::{load_custom_bootstrap_peers, save_custom_bootstrap_peers};
use mycelium_core::energy::NodeState;
use mycelium_node::{NodeCommand, NodeConfig, NodeHandle, NodeRunner};
use serde_json::json;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;

pub struct AppState {
    handle: NodeHandle,
    app_node: Arc<AppNode>,
    app_storage: Arc<AppStorage>,
    app_store: Arc<mycelium_app::miniapp::AppStore>,
    coin_node: Arc<CoinNode>,
    local_peer_id: String,
    coin_identity_path: String,
    db_path: String,
    cap_mac_key: [u8; 32],
    energy_state: NodeState,
}

type SharedState = Arc<RwLock<Option<AppState>>>;

struct DesktopNotifier {
    app: AppHandle,
}

impl NotificationSink for DesktopNotifier {
    fn on_chat_received(&self, _from: &str, _preview: &str) {}

    fn on_mail_received(&self, _from: &str, _subject: &str) {}

    fn on_bulletin_posted(&self, _scope: &str, _title: &str) {}

    fn on_contact_request(&self, peer_id: &str, display_name: &str) {
        let _ = self.app.emit(
            "contacts-updated",
            json!({ "peer_id": peer_id, "display_name": display_name }),
        );
    }
}

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
fn get_default_db_path() -> String {
    default_db_path()
}

fn default_db_path() -> String {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("mycelium")
        .to_string_lossy()
        .to_string()
}

/// Delete local Mycelium data (identity, messages, keys). Safe before first successful start.
#[tauri::command]
fn reset_local_data(db_path: Option<String>) -> Result<(), String> {
    let path = db_path.unwrap_or_else(default_db_path);
    mycelium_core::at_rest::clear_keyring_master(&path);
    if std::path::Path::new(&path).exists() {
        std::fs::remove_dir_all(&path).map_err(|e| format!("failed to reset local data: {e}"))?;
        tracing::info!("reset local data at {path}");
    }
    Ok(())
}

#[tauri::command]
async fn start_node(
    app: AppHandle,
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
        display_name: Some(display_name.clone()),
        storage_key: None,
        max_relay_fanout: 3,
    };
    let (runner, handle) = NodeRunner::new(config).map_err(|e| e.to_string())?;
    let local_peer_id = runner.local_peer_id().to_string();
    tauri::async_runtime::spawn(async move {
        let _ = runner.run().await;
    });

    let app_storage =
        Arc::new(AppStorage::open(&format!("{db_path}/app")).map_err(|e| e.to_string())?);
    let app_store = Arc::new(
        mycelium_app::miniapp::AppStore::open(&format!("{db_path}/miniapp"))
            .map_err(|e| e.to_string())?,
    );
    let coin_ledger =
        Arc::new(LocalLedger::open(&format!("{db_path}/coin")).map_err(|e| e.to_string())?);
    let identity_path = format!("{db_path}/identity");
    let coin_keypair =
        mycelium_node::load_or_create_keypair(&identity_path).map_err(|e| e.to_string())?;
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

    let (app_node, inbox) = AppNode::new(
        handle.clone(),
        local_peer_id.clone(),
        display_name,
        app_storage.clone(),
        Arc::new(DesktopNotifier { app: app.clone() }),
        Some(coin_node.clone()),
        Some(app_store.clone()),
    );
    let app_node = Arc::new(app_node);
    app_node.clone().start_incoming_task();
    setup_inbox_emitter(app, inbox);

    let cap_mac_key = mycelium_app::miniapp::mac_key_from_db_path(&db_path);
    *state.write().await = Some(AppState {
        handle,
        app_node,
        app_storage,
        app_store,
        coin_node,
        local_peer_id: local_peer_id.clone(),
        coin_identity_path: format!("{db_path}/identity"),
        db_path: db_path.clone(),
        cap_mac_key,
        energy_state: NodeState::Active,
    });

    Ok(local_peer_id)
}

#[tauri::command]
async fn get_peers(state: State<'_, SharedState>) -> Result<Vec<String>, String> {
    let guard = state.read().await;
    let Some(s) = guard.as_ref() else {
        return Ok(vec![]);
    };
    let relay = mycelium_core::bootstrap::RELAY_PEER_ID;
    Ok(s.handle
        .known_peers()
        .await
        .into_iter()
        .filter(|p| p != relay)
        .collect())
}

#[tauri::command]
fn get_relay_status() -> Result<serde_json::Value, String> {
    let status = mycelium_core::relay_status::fetch_relay_status(None);
    serde_json::to_value(status).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_shareable_multiaddrs(state: State<'_, SharedState>) -> Result<Vec<String>, String> {
    let guard = state.read().await;
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    let listen = s.handle.listen_addrs().await;
    Ok(mycelium_core::bootstrap::shareable_dial_multiaddrs(
        &s.local_peer_id,
        &listen,
    ))
}

#[tauri::command]
async fn connect_peer_id(state: State<'_, SharedState>, peer_id: String) -> Result<(), String> {
    let peer_id = peer_id.trim().to_string();
    {
        let guard = state.read().await;
        let s = guard
            .as_ref()
            .ok_or_else(|| "node not started".to_string())?;
        let _ = s
            .app_node
            .add_contact(&peer_id, "", true)
            .map_err(|e| e.to_string())?;
    }
    let addr = mycelium_core::bootstrap::relay_circuit_multiaddr(&peer_id)
        .ok_or_else(|| "invalid peer id".to_string())?;
    add_peer(state, addr).await
}

fn contact_json(c: Contact) -> serde_json::Value {
    let status = match c.status {
        ContactStatus::Pending => "pending",
        ContactStatus::Accepted => "accepted",
    };
    json!({
        "peer_id": c.peer_id,
        "display_name": c.display_name,
        "added_at_ms": c.added_at_ms,
        "status": status,
    })
}

#[tauri::command]
async fn list_contacts(state: State<'_, SharedState>) -> Result<Vec<serde_json::Value>, String> {
    let guard = state.read().await;
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    Ok(s.app_node
        .list_contacts()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(contact_json)
        .collect())
}

#[tauri::command]
async fn list_accepted_contacts(
    state: State<'_, SharedState>,
) -> Result<Vec<serde_json::Value>, String> {
    let guard = state.read().await;
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    Ok(s.app_node
        .list_accepted_contacts()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(contact_json)
        .collect())
}

#[tauri::command]
async fn list_pending_contacts(
    state: State<'_, SharedState>,
) -> Result<Vec<serde_json::Value>, String> {
    let guard = state.read().await;
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    Ok(s.app_node
        .list_pending_contacts()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(contact_json)
        .collect())
}

#[tauri::command]
async fn add_contact(
    app: AppHandle,
    state: State<'_, SharedState>,
    peer_id: String,
    display_name: String,
    accepted: bool,
) -> Result<serde_json::Value, String> {
    let guard = state.read().await;
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    let c = s
        .app_node
        .add_contact(&peer_id, &display_name, accepted)
        .map_err(|e| e.to_string())?;
    let _ = app.emit("contacts-updated", ());
    Ok(contact_json(c))
}

#[tauri::command]
async fn accept_contact(
    app: AppHandle,
    state: State<'_, SharedState>,
    peer_id: String,
) -> Result<serde_json::Value, String> {
    let guard = state.read().await;
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    let c = s
        .app_node
        .accept_contact(&peer_id)
        .map_err(|e| e.to_string())?;
    let _ = app.emit("contacts-updated", ());
    Ok(contact_json(c))
}

#[tauri::command]
async fn reject_contact(
    app: AppHandle,
    state: State<'_, SharedState>,
    peer_id: String,
) -> Result<(), String> {
    let guard = state.read().await;
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    s.app_node
        .reject_contact(&peer_id)
        .map_err(|e| e.to_string())?;
    let _ = app.emit("contacts-updated", ());
    Ok(())
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
        "messages_dropped_hops": m.messages_dropped_hops,
        "messages_dropped_queue": m.messages_dropped_queue,
        "messages_dropped_invalid_sig": m.messages_dropped_invalid_sig,
        "messages_dropped_no_sig": m.messages_dropped_no_sig,
        "messages_delivered_local": m.messages_delivered_local,
        "pending_queue_size": m.pending_queue_size,
        "seen_cache_size": m.seen_cache_size,
    }))
}

#[tauri::command]
async fn send_chat(
    state: State<'_, SharedState>,
    to_peer: String,
    body: String,
) -> Result<(), String> {
    let guard = state.read().await;
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    s.app_node
        .send_chat(Some(to_peer), body)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn peer_has_enc_key(state: State<'_, SharedState>, peer_id: String) -> Result<bool, String> {
    let guard = state.read().await;
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    Ok(s.handle.has_enc_key_for(&peer_id).await)
}

#[tauri::command]
async fn send_chat_encrypted(
    state: State<'_, SharedState>,
    to_peer: String,
    body: String,
) -> Result<String, String> {
    let guard = state.read().await;
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    s.app_node
        .send_chat_encrypted(to_peer, body)
        .await
        .map(|_| "encrypted".to_string())
        .map_err(|e| e.to_string())
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
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    s.app_node
        .send_mail(to_peer, subject, body, vec![])
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn mail_inbox(
    state: State<'_, SharedState>,
    limit: u32,
) -> Result<Vec<serde_json::Value>, String> {
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
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
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
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    s.handle
        .send(NodeCommand::AddBootstrapPeer { multiaddr })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_groups(state: State<'_, SharedState>) -> Result<Vec<serde_json::Value>, String> {
    let guard = state.read().await;
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    let groups = s
        .app_node
        .storage()
        .all_groups()
        .map_err(|e| e.to_string())?;
    Ok(groups
        .into_iter()
        .map(|g| {
            json!({
                "id": g.id,
                "name": g.name,
                "member_count": g.members.len(),
                "created_at_ms": g.created_at_ms,
            })
        })
        .collect())
}

#[tauri::command]
async fn create_group(
    state: State<'_, SharedState>,
    name: String,
) -> Result<serde_json::Value, String> {
    let guard = state.read().await;
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    let g = Group::new(name);
    s.app_node
        .storage()
        .save_group(&g)
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "id": g.id,
        "name": g.name,
        "member_count": g.members.len(),
        "created_at_ms": g.created_at_ms,
    }))
}

#[tauri::command]
async fn import_group_invite(
    state: State<'_, SharedState>,
    json_str: String,
) -> Result<serde_json::Value, String> {
    let guard = state.read().await;
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    let g = Group::from_invite(&json_str).map_err(|e| e.to_string())?;
    s.app_node
        .storage()
        .save_group(&g)
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "id": g.id,
        "name": g.name,
        "member_count": g.members.len(),
        "created_at_ms": g.created_at_ms,
    }))
}

#[tauri::command]
async fn export_group_invite(
    state: State<'_, SharedState>,
    group_id: String,
) -> Result<String, String> {
    let guard = state.read().await;
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    Ok(s.app_node
        .storage()
        .group_by_id(&group_id)
        .map_err(|e| e.to_string())?
        .map(|g| g.export_invite())
        .unwrap_or_default())
}

#[tauri::command]
async fn send_group_message(
    state: State<'_, SharedState>,
    group_id: String,
    body: String,
) -> Result<(), String> {
    let guard = state.read().await;
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    let g = s
        .app_node
        .storage()
        .group_by_id(&group_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "unknown group".to_string())?;
    s.app_node
        .send_group_message(&g, body)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_group(state: State<'_, SharedState>, group_id: String) -> Result<(), String> {
    let guard = state.read().await;
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    s.app_node
        .storage()
        .delete_group(&group_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn group_chat_history(
    state: State<'_, SharedState>,
    group_id: String,
    limit: u32,
) -> Result<Vec<serde_json::Value>, String> {
    let guard = state.read().await;
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    let gid = format!("group:{group_id}");
    let messages = s
        .app_node
        .storage()
        .group_chat_history(&group_id, limit as usize)
        .map_err(|e| e.to_string())?;
    Ok(messages
        .into_iter()
        .map(|m| {
            json!({
                "id": m.id.to_string(),
                "from_peer": &gid,
                "from_display_name": m.from_display_name,
                "body": m.body,
                "timestamp_ms": m.timestamp_ms,
            })
        })
        .collect())
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
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
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

#[tauri::command]
async fn miniapp_list_installed(
    state: State<'_, SharedState>,
) -> Result<Vec<serde_json::Value>, String> {
    let g = state.read().await;
    let s = g.as_ref().ok_or_else(|| "node not started".to_string())?;
    let apps = s.app_store.installed_apps().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for m in apps {
        out.push(serde_json::to_value(&m).map_err(|e| e.to_string())?);
    }
    Ok(out)
}

fn miniapp_trust_level_name(level: mycelium_app::miniapp::InstallTrustLevel) -> &'static str {
    use mycelium_app::miniapp::InstallTrustLevel;
    match level {
        InstallTrustLevel::VerifiedListing => "verified_listing",
        InstallTrustLevel::MatchingListingHash => "matching_listing_hash",
        InstallTrustLevel::SideloadOnly => "sideload_only",
        InstallTrustLevel::HashMismatch => "hash_mismatch",
    }
}

#[tauri::command]
async fn miniapp_preview_install(
    state: State<'_, SharedState>,
    bundle_base64: String,
) -> Result<serde_json::Value, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(bundle_base64.trim())
        .map_err(|e| e.to_string())?;
    let g = state.read().await;
    let s = g.as_ref().ok_or_else(|| "node not started".to_string())?;
    let p = s
        .app_store
        .preview_install(&bytes)
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "manifest": p.manifest,
        "bundle_hash": p.bundle_hash,
        "trust_level": miniapp_trust_level_name(p.trust_level),
        "listing_signature_ok": p.listing_signature_ok,
        "installed_version": p.installed_version,
        "is_downgrade": p.is_downgrade,
        "has_inline_script": p.has_inline_script,
        "strict_csp_eligible": p.strict_csp_eligible,
        "reproducible_attested": p.reproducible_attested,
    }))
}

#[tauri::command]
async fn miniapp_install(
    state: State<'_, SharedState>,
    bundle_base64: String,
    listing_app_id: Option<String>,
    allow_sideload: bool,
    allow_downgrade: bool,
) -> Result<serde_json::Value, String> {
    use base64::Engine;
    use mycelium_app::miniapp::InstallTrust;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(bundle_base64.trim())
        .map_err(|e| e.to_string())?;
    let trust = if listing_app_id.is_some() {
        InstallTrust::VerifiedListing
    } else if allow_sideload {
        InstallTrust::SideloadAcknowledged
    } else {
        return Err("listing_app_id or allow_sideload required".into());
    };
    let g = state.read().await;
    let s = g.as_ref().ok_or_else(|| "node not started".to_string())?;
    if let Some(ref id) = listing_app_id {
        let preview = s
            .app_store
            .preview_install(&bytes)
            .map_err(|e| e.to_string())?;
        if preview.manifest.id != *id {
            return Err("bundle app id does not match listing_app_id".into());
        }
    }
    let m = s
        .app_store
        .install_verified(&bytes, trust, allow_downgrade)
        .map_err(|e| e.to_string())?;
    serde_json::to_value(&m).map_err(|e| e.to_string())
}

#[tauri::command]
async fn miniapp_uninstall(state: State<'_, SharedState>, app_id: String) -> Result<(), String> {
    let g = state.read().await;
    let s = g.as_ref().ok_or_else(|| "node not started".to_string())?;
    s.app_store.uninstall(&app_id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn miniapp_browse_store(
    state: State<'_, SharedState>,
) -> Result<Vec<serde_json::Value>, String> {
    let g = state.read().await;
    let s = g.as_ref().ok_or_else(|| "node not started".to_string())?;
    let rows = s.app_store.browse_listings().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for l in rows {
        out.push(serde_json::to_value(&l).map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
async fn miniapp_get_entry_html(
    state: State<'_, SharedState>,
    app_id: String,
    script_nonce: String,
) -> Result<String, String> {
    let g = state.read().await;
    let s = g.as_ref().ok_or_else(|| "node not started".to_string())?;
    let m = s
        .app_store
        .get_manifest(&app_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "unknown app".to_string())?;
    let bytes = s
        .app_store
        .get_file(&app_id, &m.entry)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "entry missing".to_string())?;
    let body = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    let session = mycelium_app::miniapp::issue_session(&app_id);
    let bridge = mycelium_app::miniapp::bridge_api_js_source();
    let network_block = format!(
        r#"<script nonce="{script_nonce}">
window.fetch = function() {{ return Promise.reject(new Error('Network blocked')); }};
window.XMLHttpRequest = function() {{ throw new Error('Network blocked'); }};
window.WebSocket = function() {{ throw new Error('WebSocket blocked'); }};
if (navigator.sendBeacon) {{ navigator.sendBeacon = function() {{ return false; }}; }}
</script>"#
    );
    let shim = format!(
        r#"<script nonce="{script_nonce}">
window.__mycelium_session = {session_json};
window.addEventListener("message", function (ev) {{
  if (ev.data && ev.data.__mycelium_resolve && window.__mycelium_resolve) {{
    var r = ev.data.__mycelium_resolve;
    window.__mycelium_resolve(r.id, r.result, r.error);
  }}
}});
window.__mycelium_native_call = function (json) {{
  window.parent.postMessage({{ __mycelium_call: json }}, window.location.origin);
}};
</script><script nonce="{script_nonce}">
"#,
        session_json = serde_json::to_string(&session).map_err(|e| e.to_string())?
    );
    Ok(format!("{network_block}\n{shim}{bridge}</script>\n{body}"))
}

#[tauri::command]
fn miniapp_revoke_bridge_session(app_id: String) {
    mycelium_app::miniapp::revoke_session(&app_id);
}

#[tauri::command]
async fn miniapp_issue_capability(
    state: State<'_, SharedState>,
    app_id: String,
    permission: String,
    session_token: String,
) -> Result<String, String> {
    let g = state.read().await;
    let s = g.as_ref().ok_or_else(|| "node not started".to_string())?;
    let perm = mycelium_app::miniapp::parse_permission_name(&permission)
        .ok_or_else(|| format!("unknown permission: {permission}"))?;
    Ok(mycelium_app::miniapp::issue_capability(
        &s.cap_mac_key,
        &app_id,
        &perm,
        &session_token,
        mycelium_app::miniapp::DEFAULT_CAP_TTL_MS,
    ))
}

#[tauri::command]
async fn miniapp_publish_revocation(
    state: State<'_, SharedState>,
    app_id: String,
    reason: String,
) -> Result<(), String> {
    let g = state.read().await;
    let s = g.as_ref().ok_or_else(|| "node not started".to_string())?;
    let keypair =
        mycelium_node::load_or_create_keypair(&s.coin_identity_path).map_err(|e| e.to_string())?;
    let entry = s
        .app_store
        .build_revocation_gossip(&app_id, &reason, &s.local_peer_id, &keypair)
        .map_err(|e| e.to_string())?;
    s.app_node
        .publish_app_revocation(&entry)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn miniapp_get_policy(
    state: State<'_, SharedState>,
    app_id: String,
) -> Result<serde_json::Value, String> {
    let g = state.read().await;
    let s = g.as_ref().ok_or_else(|| "node not started".to_string())?;
    let p = s
        .app_store
        .policy_snapshot(&app_id)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "app_id": p.app_id,
        "reputation_score": p.reputation_score,
        "safe_mode_active": p.safe_mode_active,
        "safe_mode_forced": p.safe_mode_forced,
        "safe_mode_suggested": p.safe_mode_suggested,
        "user_safe_mode": p.user_safe_mode,
        "revoked": p.revoked,
        "strict_csp_eligible": p.strict_csp_eligible,
    }))
}

#[tauri::command]
async fn miniapp_set_safe_mode(
    state: State<'_, SharedState>,
    app_id: String,
    enabled: bool,
) -> Result<(), String> {
    let g = state.read().await;
    let s = g.as_ref().ok_or_else(|| "node not started".to_string())?;
    s.app_store
        .set_user_safe_mode(&app_id, enabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn miniapp_report_app(state: State<'_, SharedState>, app_id: String) -> Result<(), String> {
    let g = state.read().await;
    let s = g.as_ref().ok_or_else(|| "node not started".to_string())?;
    s.app_store
        .record_user_report(&app_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn miniapp_bridge_call(
    state: State<'_, SharedState>,
    app_id: String,
    method: String,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let g = state.read().await;
    let s = g.as_ref().ok_or_else(|| "node not started".to_string())?;
    let manifest = s
        .app_store
        .get_manifest(&app_id)
        .map_err(|e| e.to_string())?;
    let permissions = manifest.map(|m| m.permissions).unwrap_or_default();
    let enc = s.handle.local_enc_pubkey_hex();
    let safe_mode = s.app_store.effective_safe_mode(&app_id).unwrap_or(false);
    let host = mycelium_app::miniapp::bridge_host::BridgeHost::new(
        app_id,
        permissions,
        s.app_node.clone(),
        s.app_storage.clone(),
        s.app_store.clone(),
        Some(s.coin_node.clone()),
        s.local_peer_id.clone(),
        enc,
        safe_mode,
        s.cap_mac_key,
    );
    host.handle(&method, &args).await
}

fn node_state_label(state: NodeState) -> &'static str {
    match state {
        NodeState::Active => "Active",
        NodeState::Intermittent => "Intermittent",
        NodeState::Passive => "Passive",
    }
}

fn parse_energy_state(energy_state: &str) -> Result<NodeState, String> {
    match energy_state {
        "Active" => Ok(NodeState::Active),
        "Intermittent" => Ok(NodeState::Intermittent),
        "Passive" => Ok(NodeState::Passive),
        other => Err(format!("unknown energy state: {other}")),
    }
}

#[tauri::command]
async fn get_settings(state: State<'_, SharedState>) -> Result<serde_json::Value, String> {
    let guard = state.read().await;
    let Some(s) = guard.as_ref() else {
        return Ok(json!({}));
    };
    let display_name = s.app_node.display_name().await;
    Ok(json!({
        "display_name": display_name,
        "energy_state": node_state_label(s.energy_state),
        "local_peer_id": s.local_peer_id,
    }))
}

#[tauri::command]
async fn set_display_name(state: State<'_, SharedState>, name: String) -> Result<(), String> {
    let guard = state.read().await;
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    s.app_node.set_display_name(name).await;
    Ok(())
}

#[tauri::command]
async fn set_energy_state(
    state: State<'_, SharedState>,
    energy_state: String,
) -> Result<(), String> {
    let ns = parse_energy_state(&energy_state)?;
    let mut guard = state.write().await;
    let s = guard
        .as_mut()
        .ok_or_else(|| "node not started".to_string())?;
    s.handle
        .send(NodeCommand::SetEnergyState(ns))
        .await
        .map_err(|e| e.to_string())?;
    s.energy_state = ns;
    Ok(())
}

#[tauri::command]
async fn get_store_stats(state: State<'_, SharedState>) -> Result<serde_json::Value, String> {
    let guard = state.read().await;
    let Some(s) = guard.as_ref() else {
        return Ok(json!({"count": 0, "oldest_ms": 0}));
    };
    let stats = s.handle.store_stats().await.map_err(|e| e.to_string())?;
    Ok(json!({
        "count": stats.count,
        "oldest_ms": stats.oldest_ms,
    }))
}

#[tauri::command]
async fn run_gc(state: State<'_, SharedState>) -> Result<u64, String> {
    let guard = state.read().await;
    let s = guard
        .as_ref()
        .ok_or_else(|| "node not started".to_string())?;
    let deleted = s.handle.gc_now().await.map_err(|e| e.to_string())?;
    Ok(deleted as u64)
}

#[tauri::command]
async fn get_custom_bootstrap_peers(state: State<'_, SharedState>) -> Result<Vec<String>, String> {
    let guard = state.read().await;
    let Some(s) = guard.as_ref() else {
        return Ok(vec![]);
    };
    Ok(load_custom_bootstrap_peers(&s.db_path))
}

#[tauri::command]
async fn set_custom_bootstrap_peers(
    state: State<'_, SharedState>,
    peers: Vec<String>,
) -> Result<(), String> {
    let mut guard = state.write().await;
    let s = guard
        .as_mut()
        .ok_or_else(|| "node not started".to_string())?;
    save_custom_bootstrap_peers(&s.db_path, &peers).map_err(|e| e.to_string())?;
    for multiaddr in &peers {
        if let Err(e) = s
            .handle
            .send(NodeCommand::AddBootstrapPeer {
                multiaddr: multiaddr.clone(),
            })
            .await
        {
            tracing::warn!("failed to dial bootstrap peer {multiaddr}: {e}");
        }
    }
    Ok(())
}

/// Immediately deletes all local data and restarts the app.
#[tauri::command]
async fn panic_wipe(app: AppHandle, state: State<'_, SharedState>) -> Result<(), String> {
    let db_path = {
        let guard = state.read().await;
        guard.as_ref().map(|s| s.db_path.clone())
    };

    {
        let mut guard = state.write().await;
        *guard = None;
    }

    if let Some(path) = db_path {
        if std::path::Path::new(&path).exists() {
            std::fs::remove_dir_all(&path).map_err(|e| format!("failed to wipe data: {e}"))?;
            tracing::info!("PANIC WIPE: deleted {path}");
        }
    }

    app.restart();
}

fn setup_inbox_emitter(app: AppHandle, mut inbox: AppInbox) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                result = inbox.chat_rx.recv() => {
                    match result {
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            let _ = app.emit("chat-updated", ());
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                result = inbox.mail_rx.recv() => {
                    match result {
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            let _ = app.emit("mail-updated", ());
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                result = inbox.bulletin_rx.recv() => {
                    match result {
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            let _ = app.emit("bulletin-updated", ());
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                result = inbox.appstore_rx.recv() => {
                    match result {
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            let _ = app.emit("appstore-updated", ());
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    });
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
            get_default_db_path,
            reset_local_data,
            start_node,
            get_peers,
            get_relay_status,
            get_shareable_multiaddrs,
            connect_peer_id,
            list_contacts,
            list_accepted_contacts,
            list_pending_contacts,
            add_contact,
            accept_contact,
            reject_contact,
            get_metrics,
            send_chat,
            peer_has_enc_key,
            send_chat_encrypted,
            chat_history,
            send_mail,
            mail_inbox,
            wallet_balance,
            wallet_address,
            wallet_send,
            post_bulletin,
            bulletins_for_scope,
            add_peer,
            list_groups,
            create_group,
            import_group_invite,
            export_group_invite,
            send_group_message,
            delete_group,
            group_chat_history,
            miniapp_list_installed,
            miniapp_preview_install,
            miniapp_install,
            miniapp_uninstall,
            miniapp_browse_store,
            miniapp_get_entry_html,
            miniapp_bridge_call,
            miniapp_revoke_bridge_session,
            miniapp_issue_capability,
            miniapp_publish_revocation,
            miniapp_get_policy,
            miniapp_set_safe_mode,
            miniapp_report_app,
            get_settings,
            set_display_name,
            set_energy_state,
            get_store_stats,
            run_gc,
            get_custom_bootstrap_peers,
            set_custom_bootstrap_peers,
            panic_wipe,
        ])
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}
