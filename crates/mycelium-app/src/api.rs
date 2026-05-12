use crate::envelope::Attachment;
use crate::node::AppNode;
use crate::storage::AppStorage;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use mycelium_core::energy::NodeState;
use mycelium_node::NodeCommand;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

#[derive(Clone)]
struct ApiState {
    app: Arc<AppNode>,
    storage: Arc<AppStorage>,
}

#[derive(Debug, Serialize)]
struct StatusResponse {
    peer_id: String,
    display_name: String,
    peers: Vec<String>,
    metrics: mycelium_node::NodeMetrics,
}

#[derive(Debug, Deserialize)]
struct ChatBody {
    body: String,
}

#[derive(Debug, Deserialize)]
struct BulletinBody {
    title: String,
    body: String,
    ttl_secs: u64,
}

#[derive(Debug, Deserialize)]
struct MailBody {
    subject: String,
    body: String,
    attachments: Option<Vec<Attachment>>,
}

#[derive(Debug, Deserialize)]
struct NameBody {
    display_name: String,
}

#[derive(Debug, Deserialize)]
struct EnergyBody {
    state: String,
}

#[derive(Debug, Deserialize)]
struct AddPeerBody {
    multiaddr: String,
}

#[derive(Debug, Deserialize)]
struct ScopeBody {
    scope: String,
}

#[derive(Debug, Serialize)]
struct StoreStatsResponse {
    count: usize,
    oldest_ms: u64,
    size_estimate_kb: u64,
}

pub async fn start_api_server(
    app_node: Arc<AppNode>,
    app_storage: Arc<AppStorage>,
    port: u16,
) -> anyhow::Result<()> {
    let state = ApiState {
        app: app_node,
        storage: app_storage,
    };
    let app = Router::new()
        .route("/api/v1/status", get(get_status))
        .route("/api/v1/peers", get(get_peers))
        .route("/api/v1/chat/:peer_id", get(get_chat).post(post_chat))
        .route("/api/v1/chat/broadcast", post(post_chat_broadcast))
        .route(
            "/api/v1/bulletin/:scope",
            get(get_bulletin).post(post_bulletin),
        )
        .route("/api/v1/mail/inbox", get(get_mail_inbox))
        .route("/api/v1/mail/sent", get(get_mail_sent))
        .route("/api/v1/mail/:peer_id", post(post_mail))
        .route("/api/v1/mail/:id/read", put(put_mail_read))
        .route("/api/v1/settings/name", put(put_name))
        .route("/api/v1/settings", get(get_settings))
        .route("/api/v1/settings/energy", put(put_energy))
        .route("/api/v1/store/stats", get(get_store_stats))
        .route("/api/v1/store/gc", post(post_store_gc))
        .route("/api/v1/peers/:peer_id/reputation", get(get_peer_reputation))
        .route("/api/v1/peers/add", post(post_peer_add))
        .route("/api/v1/scopes", get(get_scopes))
        .route("/api/v1/scopes/subscribe", post(post_scope_subscribe))
        .route("/api/v1/scopes/:scope", delete(delete_scope))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok(())
}

async fn get_status(State(state): State<ApiState>) -> Json<StatusResponse> {
    let metrics = state.app.node_handle().metrics().await;
    Json(StatusResponse {
        peer_id: state.app.local_peer_id().to_string(),
        display_name: state.app.display_name().await,
        peers: Vec::new(),
        metrics,
    })
}

async fn get_peers() -> Json<Vec<serde_json::Value>> {
    Json(Vec::new())
}

async fn get_chat(
    Path(peer_id): Path<String>,
    State(state): State<ApiState>,
) -> Result<Json<Vec<crate::envelope::ChatMessage>>, (StatusCode, String)> {
    state
        .storage
        .chat_history(&peer_id, 200)
        .map(Json)
        .map_err(internal_err)
}

async fn post_chat(
    Path(peer_id): Path<String>,
    State(state): State<ApiState>,
    Json(body): Json<ChatBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .app
        .send_chat(Some(peer_id), body.body)
        .await
        .map(|_| StatusCode::OK)
        .map_err(internal_err)
}

async fn post_chat_broadcast(
    State(state): State<ApiState>,
    Json(body): Json<ChatBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .app
        .send_chat(None, body.body)
        .await
        .map(|_| StatusCode::OK)
        .map_err(internal_err)
}

async fn get_bulletin(
    Path(scope): Path<String>,
    State(state): State<ApiState>,
) -> Result<Json<Vec<crate::envelope::BulletinPost>>, (StatusCode, String)> {
    state
        .storage
        .bulletins_for_scope(&scope)
        .map(Json)
        .map_err(internal_err)
}

async fn post_bulletin(
    Path(scope): Path<String>,
    State(state): State<ApiState>,
    Json(body): Json<BulletinBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .app
        .post_bulletin(scope, body.title, body.body, body.ttl_secs)
        .await
        .map(|_| StatusCode::OK)
        .map_err(internal_err)
}

async fn get_mail_inbox(
    State(state): State<ApiState>,
) -> Result<Json<Vec<crate::envelope::MailMessage>>, (StatusCode, String)> {
    state.storage.inbox(200).map(Json).map_err(internal_err)
}

async fn get_mail_sent(
    State(state): State<ApiState>,
) -> Result<Json<Vec<crate::envelope::MailMessage>>, (StatusCode, String)> {
    state.storage.sent(200).map(Json).map_err(internal_err)
}

async fn post_mail(
    Path(peer_id): Path<String>,
    State(state): State<ApiState>,
    Json(body): Json<MailBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .app
        .send_mail(
            peer_id,
            body.subject,
            body.body,
            body.attachments.unwrap_or_default(),
        )
        .await
        .map(|_| StatusCode::OK)
        .map_err(internal_err)
}

async fn put_mail_read(
    Path(id): Path<String>,
    State(state): State<ApiState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let uuid = uuid::Uuid::parse_str(&id).map_err(internal_err)?;
    state
        .storage
        .mark_read(&uuid)
        .map(|_| StatusCode::OK)
        .map_err(internal_err)
}

async fn put_name(
    State(state): State<ApiState>,
    Json(body): Json<NameBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    state.app.set_display_name(body.display_name).await;
    Ok(StatusCode::OK)
}

async fn get_settings(State(state): State<ApiState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "display_name": state.app.display_name().await,
        "energy_state": "active"
    }))
}

async fn put_energy(
    State(state): State<ApiState>,
    Json(body): Json<EnergyBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    let cmd = match body.state.as_str() {
        "active" => NodeCommand::SetEnergyState(NodeState::Active),
        "intermittent" => NodeCommand::SetEnergyState(NodeState::Intermittent),
        "passive" => NodeCommand::SetEnergyState(NodeState::Passive),
        _ => return Err((StatusCode::BAD_REQUEST, "invalid energy state".to_string())),
    };
    state.app.node_handle().send(cmd).await.map_err(internal_err)?;
    Ok(StatusCode::OK)
}

fn internal_err<E: std::fmt::Display>(err: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

async fn get_store_stats(
    State(state): State<ApiState>,
) -> Result<Json<StoreStatsResponse>, (StatusCode, String)> {
    let inbox = state.storage.inbox(10_000).map_err(internal_err)?;
    let sent = state.storage.sent(10_000).map_err(internal_err)?;
    let count = inbox.len() + sent.len();
    let oldest_ms = inbox
        .iter()
        .chain(sent.iter())
        .map(|m| m.timestamp_ms)
        .min()
        .unwrap_or(0);
    Ok(Json(StoreStatsResponse {
        count,
        oldest_ms,
        size_estimate_kb: ((count as u64) * 2).max(1),
    }))
}

async fn post_store_gc(State(state): State<ApiState>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let deleted = state.storage.prune_expired_bulletins().map_err(internal_err)?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

async fn get_peer_reputation(
    Path(peer_id): Path<String>,
    State(state): State<ApiState>,
) -> Json<serde_json::Value> {
    let rep = state.app.node_handle().peer_reputation(&peer_id).await;
    match rep {
        Some(rep) => Json(serde_json::json!({
            "peer_id": peer_id,
            "strikes": rep.strikes,
            "throttled": mycelium_core::data::now_ms() < rep.throttled_until_ms,
            "throttled_until_ms": rep.throttled_until_ms
        })),
        None => Json(serde_json::json!({
            "peer_id": peer_id,
            "strikes": 0,
            "throttled": false,
            "throttled_until_ms": 0
        })),
    }
}

async fn post_peer_add(
    State(state): State<ApiState>,
    Json(body): Json<AddPeerBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .app
        .node_handle()
        .send(NodeCommand::AddBootstrapPeer {
            multiaddr: body.multiaddr,
        })
        .await
        .map_err(internal_err)?;
    Ok(StatusCode::OK)
}

async fn get_scopes(State(state): State<ApiState>) -> Json<Vec<String>> {
    Json(state.app.node_handle().subscribed_scopes().await)
}

async fn post_scope_subscribe(
    State(state): State<ApiState>,
    Json(body): Json<ScopeBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .app
        .node_handle()
        .send(NodeCommand::SubscribeScope(body.scope))
        .await
        .map_err(internal_err)?;
    Ok(StatusCode::OK)
}

async fn delete_scope(
    Path(scope): Path<String>,
    State(state): State<ApiState>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .app
        .node_handle()
        .send(NodeCommand::UnsubscribeScope(scope))
        .await
        .map_err(internal_err)?;
    Ok(StatusCode::OK)
}
