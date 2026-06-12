#![allow(clippy::empty_line_after_doc_comments)]
// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project

use async_trait::async_trait;
use mycelium_app::contacts::{Contact, ContactStatus as AppContactStatus};
use mycelium_app::envelope::{
    AppMessage, BulletinPost as AppBulletinPost, ChatMessage as AppChatMessage,
    MailMessage as AppMailMessage,
};
use mycelium_app::groups::Group;
use mycelium_app::node::AppNode;
use mycelium_app::notify::NotificationSink;
use mycelium_app::storage::AppStorage;
use mycelium_coin::{
    address_from_keypair, CoinNode, CoinTransport, HotWallet,
    HotWalletConfig as RustHotWalletConfig, LocalLedger, PaymentRequest,
};
use mycelium_core::energy::NodeState as RustEnergyState;
use mycelium_core::transport::ConnectivityMode as NetConnectivityMode;
use mycelium_node::{
    ConnectivityMonitor, NodeCommand, NodeConfig as RustNodeConfig, NodeHandle, NodeRunner,
};
use once_cell::sync::{Lazy, OnceCell};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use tokio::sync::RwLock;

/// Matches `[Error] interface MyceliumException` in `mycelium.udl`.
#[derive(Debug)]
pub enum MyceliumException {
    InstallError { detail: String },
    ChatError { detail: String },
}

impl std::fmt::Display for MyceliumException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MyceliumException::InstallError { detail } => write!(f, "{detail}"),
            MyceliumException::ChatError { detail } => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for MyceliumException {}

uniffi::include_scaffolding!("mycelium");

#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub db_path: String,
    pub listen_addr: String,
    pub display_name: String,
    /// Empty: [`NodeRunner::new`] loads peers via [`mycelium_core::bootstrap::load_bootstrap_peers`].
    pub bootstrap_peers: Vec<String>,
    /// Optional 64-char hex master key for at-rest encryption (Android).
    pub storage_key_hex: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NodeMetrics {
    pub messages_forwarded: u64,
    pub messages_dropped_ttl: u64,
    pub messages_dropped_queue: u64,
    pub messages_delivered_local: u64,
    pub pending_queue_size: u64,
    pub seen_cache_size: u64,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub id: String,
    pub from_peer: String,
    pub from_display_name: String,
    pub body: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone)]
pub struct BulletinPost {
    pub id: String,
    pub from_display_name: String,
    pub title: String,
    pub body: String,
    pub scope: String,
    pub timestamp_ms: u64,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone)]
pub struct MailMessage {
    pub id: String,
    pub from_peer: String,
    pub from_display_name: String,
    pub to_peer: String,
    pub subject: String,
    pub body: String,
    pub timestamp_ms: u64,
    pub is_read: bool,
}

#[derive(Debug, Clone)]
pub struct GroupInfo {
    pub id: String,
    pub name: String,
    pub member_count: u32,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone)]
pub enum ContactStatus {
    Pending,
    Accepted,
}

#[derive(Debug, Clone)]
pub struct ContactInfo {
    pub peer_id: String,
    pub display_name: String,
    pub added_at_ms: u64,
    pub status: ContactStatus,
}

fn contact_to_info(c: Contact) -> ContactInfo {
    let status = match c.status {
        AppContactStatus::Pending => ContactStatus::Pending,
        AppContactStatus::Accepted => ContactStatus::Accepted,
    };
    ContactInfo {
        peer_id: c.peer_id,
        display_name: c.display_name,
        added_at_ms: c.added_at_ms,
        status,
    }
}

#[derive(Debug, Clone)]
pub struct WalletBalance {
    pub confirmed_muon: u64,
    pub pending_muon: u64,
}

#[derive(Debug, Clone)]
pub struct TxInfo {
    pub id: String,
    pub from_address: String,
    pub to_address: String,
    pub amount_muon: u64,
    pub fee_muon: u64,
    pub timestamp_ms: u64,
    pub memo: Option<String>,
    pub witness_count: u32,
    pub confirmed: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum EnergyState {
    Active,
    Intermittent,
    Passive,
}

#[derive(Debug, Clone, Copy)]
pub enum ConnectivityMode {
    Internet,
    MeshOnly,
}

impl From<NetConnectivityMode> for ConnectivityMode {
    fn from(m: NetConnectivityMode) -> Self {
        match m {
            NetConnectivityMode::Internet => Self::Internet,
            NetConnectivityMode::MeshOnly => Self::MeshOnly,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HotWalletConfig {
    pub max_cache_muon: u64,
    pub refill_threshold_muon: u64,
    pub refill_amount_muon: u64,
    pub cold_wallet_address: Option<String>,
}

impl From<RustHotWalletConfig> for HotWalletConfig {
    fn from(c: RustHotWalletConfig) -> Self {
        Self {
            max_cache_muon: c.max_cache_muon,
            refill_threshold_muon: c.refill_threshold_muon,
            refill_amount_muon: c.refill_amount_muon,
            cold_wallet_address: c.cold_wallet_address,
        }
    }
}

impl From<HotWalletConfig> for RustHotWalletConfig {
    fn from(c: HotWalletConfig) -> Self {
        Self {
            max_cache_muon: c.max_cache_muon,
            refill_threshold_muon: c.refill_threshold_muon,
            refill_amount_muon: c.refill_amount_muon,
            cold_wallet_address: c.cold_wallet_address,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HotWalletStatus {
    pub confirmed_muon: u64,
    pub pending_muon: u64,
    pub max_cache_muon: u64,
    pub auto_refill_enabled: bool,
    pub needs_refill: bool,
    pub connectivity: ConnectivityMode,
}

#[derive(Debug, Clone)]
pub struct PaymentRequestData {
    pub to_address: String,
    pub amount_muon: u64,
    pub memo: Option<String>,
    pub expires_at_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct MiniAppInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub developer: String,
    pub entry: String,
    pub accepts_payments: bool,
    pub payment_address: Option<String>,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MiniAppPolicyData {
    pub app_id: String,
    pub reputation_score: u8,
    pub safe_mode_active: bool,
    pub safe_mode_forced: bool,
    pub safe_mode_suggested: bool,
    pub user_safe_mode: bool,
    pub revoked: bool,
    pub strict_csp_eligible: bool,
}

#[derive(Debug, Clone)]
pub struct MiniAppInstallPreview {
    pub manifest: MiniAppInfo,
    pub bundle_hash: String,
    pub trust_level: String,
    pub listing_signature_ok: bool,
    pub installed_version: Option<String>,
    pub is_downgrade: bool,
    pub has_inline_script: bool,
    pub strict_csp_eligible: bool,
    pub reproducible_attested: bool,
}

#[derive(Debug, Clone)]
pub struct AppStoreListing {
    pub manifest: MiniAppInfo,
    pub bundle_hash: String,
    pub updated_at_ms: u64,
    pub signature_valid: bool,
}

pub trait NodeEventCallback: Send + Sync {
    fn on_peer_discovered(&self, _peer_id: String) {}
    fn on_peer_lost(&self, _peer_id: String) {}
    fn on_chat_received(&self, _message: ChatMessage) {}
    fn on_mail_received(&self, _message: MailMessage) {}
    fn on_bulletin_received(&self, _post: BulletinPost) {}
    fn on_app_store_listing(&self, _listing: AppStoreListing) {}
    fn on_connectivity_changed(&self, _mode: ConnectivityMode) {}
    fn on_contact_request(&self, _peer_id: String, _display_name: String) {}
}

struct FfiNotifier;

impl NotificationSink for FfiNotifier {
    fn on_chat_received(&self, _from: &str, _preview: &str) {}

    fn on_mail_received(&self, _from: &str, _subject: &str) {}

    fn on_bulletin_posted(&self, _scope: &str, _title: &str) {}

    fn on_contact_request(&self, peer_id: &str, display_name: &str) {
        if let Some(cb) = CALLBACK.lock().expect("callback lock").clone() {
            cb.on_contact_request(peer_id.to_string(), display_name.to_string());
        }
    }
}

static RUNTIME: OnceCell<Runtime> = OnceCell::new();
static NODE: Lazy<Mutex<Option<Arc<RwLock<NodeState>>>>> = Lazy::new(|| Mutex::new(None));
static CALLBACK: Lazy<Mutex<Option<Arc<dyn NodeEventCallback>>>> = Lazy::new(|| Mutex::new(None));

struct FfiCoinTransport {
    handle: NodeHandle,
}

#[async_trait]
impl CoinTransport for FfiCoinTransport {
    async fn broadcast_coin_inner(&self, coin_inner: Vec<u8>) -> anyhow::Result<()> {
        let payload = AppMessage::encode_coin_payload(&coin_inner)?;
        self.handle
            .send(NodeCommand::BroadcastPayload {
                scope: "mycelium/coin/v1".into(),
                body: "[coin]".into(),
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
                body: "[coin]".into(),
                payload,
            })
            .await
    }
}

struct NodeState {
    handle: NodeHandle,
    app_node: Arc<AppNode>,
    app_store: Arc<mycelium_app::miniapp::AppStore>,
    coin_node: Arc<CoinNode>,
    coin_identity_path: String,
    cap_mac_key: [u8; 32],
    local_peer_id: String,
    display_name: Arc<RwLock<String>>,
    runner_task: tokio::task::JoinHandle<anyhow::Result<()>>,
    peer_watch_task: tokio::task::JoinHandle<()>,
    connectivity_task: tokio::task::JoinHandle<()>,
    hot_wallet: Arc<HotWallet>,
    connectivity_mode: Arc<Mutex<NetConnectivityMode>>,
}

pub fn init_node(config: NodeConfig) {
    let runtime = RUNTIME.get_or_init(|| Runtime::new().expect("tokio runtime init failed"));
    runtime.block_on(async move {
        #[cfg(target_os = "android")]
        {
            use tracing_subscriber::layer::SubscriberExt;
            use tracing_subscriber::util::SubscriberInitExt;
            if let Ok(android_layer) = tracing_android::layer("mycelium") {
                let _ = tracing_subscriber::registry()
                    .with(android_layer)
                    .try_init();
            }
        }

        let connectivity = ConnectivityMonitor::new();
        ConnectivityMonitor::spawn_monitor(connectivity.mode_tx.clone());
        let connectivity_rx_node = connectivity.mode_rx.clone();
        let mut connectivity_rx_bg = connectivity.mode_rx.clone();

        let storage_key = config
            .storage_key_hex
            .as_deref()
            .map(mycelium_node::parse_storage_key_hex)
            .transpose()
            .expect("invalid storage_key_hex");

        let rust_config = RustNodeConfig {
            listen_addr: config
                .listen_addr
                .parse()
                .expect("invalid listen address for node"),
            db_path: config.db_path.clone(),
            keypair_path: Some(format!("{}/identity", config.db_path)),
            forwarding_interval_ms: 500,
            sync_interval_secs: 30,
            bootstrap_peers: config.bootstrap_peers.clone(),
            connectivity_rx: Some(connectivity_rx_node),
            display_name: Some(config.display_name.clone()),
            storage_key,
            max_relay_fanout: 3,
        };

        let (runner, handle) =
            NodeRunner::new(rust_config).expect("failed to initialize node runner");
        let local_peer_id = runner.local_peer_id().to_string();
        let runner_task = tokio::spawn(async move { runner.run().await });

        let app_storage = Arc::new(
            AppStorage::open_with_key(&format!("{}/app", config.db_path), storage_key)
                .expect("failed to open app storage"),
        );
        let app_store = Arc::new(
            mycelium_app::miniapp::AppStore::open(&format!("{}/miniapp", config.db_path))
                .expect("failed to open miniapp store"),
        );
        let coin_identity_path = format!("{}/identity", config.db_path);
        let coin_addr = address_from_keypair(
            &mycelium_node::secrets::load_or_create_keypair(&coin_identity_path, storage_key)
                .expect("failed to load node identity for coin"),
        );
        let coin_ledger = Arc::new(
            LocalLedger::open(&format!("{}/coin", config.db_path))
                .expect("failed to open coin ledger"),
        );
        let coin_transport = Arc::new(FfiCoinTransport {
            handle: handle.clone(),
        });
        let coin_node = Arc::new(CoinNode::new(
            coin_ledger,
            coin_transport,
            coin_addr,
            local_peer_id.clone(),
            coin_identity_path.clone(),
        ));
        let hot_wallet = Arc::new(HotWallet::new(
            RustHotWalletConfig::default(),
            coin_node.clone(),
            coin_identity_path.clone(),
        ));
        let connectivity_mode = Arc::new(Mutex::new(*connectivity_rx_bg.borrow()));
        let cm_task = connectivity_mode.clone();
        let hw_task = hot_wallet.clone();
        let connectivity_task = tokio::spawn(async move {
            let mode0 = *connectivity_rx_bg.borrow();
            if let Ok(mut g) = cm_task.lock() {
                *g = mode0;
            }
            if mode0 == NetConnectivityMode::Internet {
                let _ = hw_task.maybe_refill().await;
                let _ = hw_task.enforce_cap().await;
            }
            loop {
                if connectivity_rx_bg.changed().await.is_err() {
                    break;
                }
                let mode = *connectivity_rx_bg.borrow();
                if let Ok(mut g) = cm_task.lock() {
                    *g = mode;
                }
                if let Some(cb) = CALLBACK.lock().expect("callback lock").clone() {
                    cb.on_connectivity_changed(mode.into());
                }
                if mode == NetConnectivityMode::Internet {
                    if let Err(e) = hw_task.maybe_refill().await {
                        tracing::warn!("hot wallet refill check failed: {e:#}");
                    }
                    if let Err(e) = hw_task.enforce_cap().await {
                        tracing::warn!("hot wallet cap enforcement failed: {e:#}");
                    }
                }
            }
        });

        let (app_node, mut inbox) = AppNode::new(
            handle.clone(),
            local_peer_id.clone(),
            config.display_name.clone(),
            app_storage.clone(),
            Arc::new(FfiNotifier),
            Some(coin_node.clone()),
            Some(app_store.clone()),
        );
        let app_node = Arc::new(app_node);
        app_node.clone().start_incoming_task();

        let peer_handle = handle.clone();
        let peer_watch_task = tokio::spawn(async move {
            let mut known = HashSet::<String>::new();
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
            loop {
                tick.tick().await;
                let peers = peer_handle.known_peers().await;
                let current: HashSet<String> = peers.into_iter().collect();
                if let Some(cb) = CALLBACK.lock().expect("callback lock").clone() {
                    for peer in current.difference(&known) {
                        cb.on_peer_discovered(peer.clone());
                    }
                    for peer in known.difference(&current) {
                        cb.on_peer_lost(peer.clone());
                    }
                }
                known = current;
            }
        });

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(chat) = inbox.chat_rx.recv() => {
                        if let Some(cb) = CALLBACK.lock().expect("callback lock").clone() {
                            cb.on_chat_received(to_chat_message(chat, String::new()));
                        }
                    }
                    Ok(mail) = inbox.mail_rx.recv() => {
                        if let Some(cb) = CALLBACK.lock().expect("callback lock").clone() {
                            cb.on_mail_received(to_mail_message(mail, false));
                        }
                    }
                    Ok(post) = inbox.bulletin_rx.recv() => {
                        if let Some(cb) = CALLBACK.lock().expect("callback lock").clone() {
                            cb.on_bulletin_received(to_bulletin(post));
                        }
                    }
                    Ok(listing) = inbox.appstore_rx.recv() => {
                        if let Some(cb) = CALLBACK.lock().expect("callback lock").clone() {
                            cb.on_app_store_listing(to_app_store_listing(listing));
                        }
                    }
                }
            }
        });

        let cap_mac_key = mycelium_app::miniapp::mac_key_from_db_path(&config.db_path);
        let state = Arc::new(RwLock::new(NodeState {
            handle,
            app_node,
            app_store,
            coin_node,
            coin_identity_path,
            cap_mac_key,
            local_peer_id,
            display_name: Arc::new(RwLock::new(config.display_name)),
            runner_task,
            peer_watch_task,
            connectivity_task,
            hot_wallet,
            connectivity_mode,
        }));
        *NODE.lock().expect("node lock") = Some(state);
    });
}

pub fn stop_node() {
    let Some(runtime) = RUNTIME.get() else { return };
    let state = NODE.lock().expect("node lock").take();
    runtime.block_on(async {
        if let Some(state) = state {
            let guard = state.write().await;
            guard.runner_task.abort();
            guard.peer_watch_task.abort();
            guard.connectivity_task.abort();
        }
    });
}

pub fn local_peer_id() -> String {
    with_state(|s| s.local_peer_id.clone())
}

pub fn known_peers() -> Vec<String> {
    let state = state_arc();
    runtime().block_on(async {
        let guard = state.read().await;
        guard.handle.known_peers().await
    })
}

pub fn metrics() -> NodeMetrics {
    let state = state_arc();
    runtime().block_on(async {
        let state = state.read().await;
        let m = state.handle.metrics().await;
        NodeMetrics {
            messages_forwarded: m.messages_forwarded,
            messages_dropped_ttl: m.messages_dropped_ttl,
            messages_dropped_queue: m.messages_dropped_queue,
            messages_delivered_local: m.messages_delivered_local,
            pending_queue_size: m.pending_queue_size as u64,
            seen_cache_size: m.seen_cache_size as u64,
        }
    })
}

pub fn send_chat_direct(to_peer: String, body: String) -> Result<(), MyceliumException> {
    let state = state_arc();
    runtime().block_on(async {
        let state = state.read().await;
        state
            .app_node
            .send_chat(Some(to_peer), body)
            .await
            .map(|_| ())
            .map_err(|e| MyceliumException::ChatError {
                detail: e.to_string(),
            })
    })
}

pub fn send_chat_broadcast(body: String) -> Result<(), MyceliumException> {
    let state = state_arc();
    runtime().block_on(async {
        let state = state.read().await;
        state
            .app_node
            .send_chat(None, body)
            .await
            .map(|_| ())
            .map_err(|e| MyceliumException::ChatError {
                detail: e.to_string(),
            })
    })
}

pub fn local_enc_pubkey() -> String {
    with_state(|s| s.handle.local_enc_pubkey_hex())
}

pub fn send_chat_encrypted(to_peer: String, body: String) -> Result<(), MyceliumException> {
    let state = state_arc();
    runtime().block_on(async {
        let state = state.read().await;
        state
            .app_node
            .send_chat_encrypted(to_peer, body)
            .await
            .map(|_| ())
            .map_err(|e| MyceliumException::ChatError {
                detail: e.to_string(),
            })
    })
}

pub fn has_enc_key_for(peer_id: String) -> bool {
    let state = state_arc();
    runtime().block_on(async {
        let state = state.read().await;
        state.handle.has_enc_key_for(&peer_id).await
    })
}

pub fn list_contacts() -> Vec<ContactInfo> {
    with_state(|state| {
        state
            .app_node
            .list_contacts()
            .unwrap_or_default()
            .into_iter()
            .map(contact_to_info)
            .collect()
    })
}

pub fn list_accepted_contacts() -> Vec<ContactInfo> {
    with_state(|state| {
        state
            .app_node
            .list_accepted_contacts()
            .unwrap_or_default()
            .into_iter()
            .map(contact_to_info)
            .collect()
    })
}

pub fn list_pending_contacts() -> Vec<ContactInfo> {
    with_state(|state| {
        state
            .app_node
            .list_pending_contacts()
            .unwrap_or_default()
            .into_iter()
            .map(contact_to_info)
            .collect()
    })
}

pub fn add_contact(peer_id: String, display_name: String, accepted: bool) -> ContactInfo {
    with_state(|state| {
        contact_to_info(
            state
                .app_node
                .add_contact(&peer_id, &display_name, accepted)
                .expect("add contact"),
        )
    })
}

pub fn accept_contact(peer_id: String) -> ContactInfo {
    with_state(|state| {
        contact_to_info(
            state
                .app_node
                .accept_contact(&peer_id)
                .expect("accept contact"),
        )
    })
}

pub fn reject_contact(peer_id: String) {
    with_state(|state| {
        let _ = state.app_node.reject_contact(&peer_id);
    })
}

pub fn remove_contact(peer_id: String) {
    with_state(|state| {
        let _ = state.app_node.remove_contact(&peer_id);
    })
}

pub fn list_groups() -> Vec<GroupInfo> {
    with_state(|state| {
        state
            .app_node
            .storage()
            .all_groups()
            .unwrap_or_default()
            .into_iter()
            .map(|g| GroupInfo {
                id: g.id.clone(),
                name: g.name.clone(),
                member_count: g.members.len() as u32,
                created_at_ms: g.created_at_ms,
            })
            .collect()
    })
}

pub fn create_group(name: String) -> GroupInfo {
    let state = state_arc();
    runtime().block_on(async {
        let state = state.read().await;
        let g = Group::new(name);
        state.app_node.storage().save_group(&g).expect("save group");
        GroupInfo {
            id: g.id.clone(),
            name: g.name.clone(),
            member_count: g.members.len() as u32,
            created_at_ms: g.created_at_ms,
        }
    })
}

pub fn import_group_invite(json: String) -> Option<GroupInfo> {
    let state = state_arc();
    runtime().block_on(async {
        let state = state.read().await;
        let g = Group::from_invite(&json).ok()?;
        state.app_node.storage().save_group(&g).ok()?;
        Some(GroupInfo {
            id: g.id.clone(),
            name: g.name.clone(),
            member_count: g.members.len() as u32,
            created_at_ms: g.created_at_ms,
        })
    })
}

pub fn export_group_invite(group_id: String) -> String {
    with_state(|state| {
        state
            .app_node
            .storage()
            .group_by_id(&group_id)
            .ok()
            .flatten()
            .map(|g| g.export_invite())
            .unwrap_or_default()
    })
}

pub fn send_group_message(group_id: String, body: String) {
    let state = state_arc();
    runtime().block_on(async {
        let state = state.read().await;
        let Some(g) = state
            .app_node
            .storage()
            .group_by_id(&group_id)
            .ok()
            .flatten()
        else {
            return;
        };
        let _ = state.app_node.send_group_message(&g, body).await;
    });
}

pub fn delete_group(group_id: String) {
    with_state(|state| {
        let _ = state.app_node.storage().delete_group(&group_id);
    });
}

pub fn group_chat_history(group_id: String, limit: u32) -> Vec<ChatMessage> {
    let state = state_arc();
    runtime().block_on(async {
        let state = state.read().await;
        let gid = format!("group:{group_id}");
        state
            .app_node
            .storage()
            .group_chat_history(&group_id, limit as usize)
            .unwrap_or_default()
            .into_iter()
            .map(|m| to_chat_message(m, gid.clone()))
            .collect()
    })
}

pub fn chat_history(peer_id: String, limit: u32) -> Vec<ChatMessage> {
    let state = state_arc();
    runtime().block_on(async {
        let state = state.read().await;
        state
            .app_node
            .chat_history(&peer_id, limit as usize)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|m| to_chat_message(m, peer_id.clone()))
            .collect()
    })
}

pub fn post_bulletin(scope: String, title: String, body: String, ttl_secs: u64) {
    let state = state_arc();
    runtime().block_on(async {
        let state = state.read().await;
        let _ = state
            .app_node
            .post_bulletin(scope, title, body, ttl_secs)
            .await;
    });
}

pub fn bulletins_for_scope(scope: String) -> Vec<BulletinPost> {
    with_state(|state| {
        state
            .app_node
            .bulletins_for_scope(&scope)
            .unwrap_or_default()
            .into_iter()
            .map(to_bulletin)
            .collect()
    })
}

pub fn send_mail(to_peer: String, subject: String, body: String) {
    let state = state_arc();
    runtime().block_on(async {
        let state = state.read().await;
        let _ = state
            .app_node
            .send_mail(to_peer, subject, body, vec![])
            .await;
    });
}

pub fn mail_inbox(limit: u32) -> Vec<MailMessage> {
    with_state(|state| {
        state
            .app_node
            .mail_inbox(limit as usize)
            .unwrap_or_default()
            .into_iter()
            .map(|m| {
                let is_read = state.app_node.is_mail_read(&m.id).unwrap_or(false);
                to_mail_message(m, is_read)
            })
            .collect()
    })
}

pub fn mail_sent(limit: u32) -> Vec<MailMessage> {
    with_state(|state| {
        state
            .app_node
            .mail_sent(limit as usize)
            .unwrap_or_default()
            .into_iter()
            .map(|m| to_mail_message(m, false))
            .collect()
    })
}

pub fn mark_mail_read(mail_id: String) {
    if let Ok(id) = uuid::Uuid::parse_str(&mail_id) {
        with_state(|state| {
            let _ = state.app_node.mark_mail_read(&id);
        });
    }
}

pub fn set_display_name(name: String) {
    let state = state_arc();
    runtime().block_on(async {
        let state = state.read().await;
        state.app_node.set_display_name(name.clone()).await;
        *state.display_name.write().await = name;
    });
}

pub fn display_name() -> String {
    let state = state_arc();
    runtime().block_on(async {
        let state = state.read().await;
        let value = state.display_name.read().await.clone();
        value
    })
}

pub fn set_energy_state(state: EnergyState) {
    let cmd = match state {
        EnergyState::Active => NodeCommand::SetEnergyState(RustEnergyState::Active),
        EnergyState::Intermittent => NodeCommand::SetEnergyState(RustEnergyState::Intermittent),
        EnergyState::Passive => NodeCommand::SetEnergyState(RustEnergyState::Passive),
    };
    let state = state_arc();
    runtime().block_on(async {
        let st = state.read().await;
        let _ = st.handle.send(cmd).await;
    });
}

pub fn add_bootstrap_peer(multiaddr: String) {
    let state = state_arc();
    runtime().block_on(async {
        let state = state.read().await;
        let _ = state
            .handle
            .send(NodeCommand::AddBootstrapPeer { multiaddr })
            .await;
    });
}

pub fn set_event_callback(callback: Box<dyn NodeEventCallback>) {
    *CALLBACK.lock().expect("callback lock") = Some(Arc::from(callback));
}

pub fn wallet_address() -> String {
    with_state(|s| s.coin_node.local_address().to_string())
}

pub fn wallet_balance() -> WalletBalance {
    with_state(|s| match s.coin_node.balance() {
        Ok((c, p)) => WalletBalance {
            confirmed_muon: c,
            pending_muon: p,
        },
        Err(_) => WalletBalance {
            confirmed_muon: 0,
            pending_muon: 0,
        },
    })
}

pub fn send_transaction(to_address: String, amount_muon: u64, memo: Option<String>) {
    let state = state_arc();
    let res = runtime().block_on(async {
        let st = state.read().await;
        if !mycelium_coin::validate_address(&to_address) {
            anyhow::bail!("invalid MeshCoin address");
        }
        st.coin_node
            .submit_transfer_from_identity_path(
                &st.coin_identity_path,
                to_address,
                amount_muon,
                1000,
                memo,
            )
            .await
    });
    if let Err(e) = res {
        tracing::warn!("send_transaction failed: {e:#}");
    }
}

pub fn wallet_recent_transactions(limit: u32) -> Vec<TxInfo> {
    with_state(|s| {
        s.coin_node
            .recent_transactions(limit as usize)
            .unwrap_or_default()
            .into_iter()
            .map(tx_to_info)
            .collect()
    })
}

pub fn request_faucet() {
    with_state(|s| {
        if let Err(e) = s.coin_node.request_faucet_coins() {
            tracing::warn!("request_faucet failed: {e:#}");
        }
    });
}

pub fn get_hot_wallet_config() -> HotWalletConfig {
    let state = state_arc();
    runtime().block_on(async {
        let st = state.read().await;
        st.hot_wallet.config().await.into()
    })
}

pub fn set_hot_wallet_config(config: HotWalletConfig) {
    let c: RustHotWalletConfig = config.into();
    let state = state_arc();
    runtime().block_on(async {
        let st = state.read().await;
        st.hot_wallet.update_config(c).await;
    });
}

pub fn set_cold_wallet_address(address: Option<String>) {
    let state = state_arc();
    runtime().block_on(async {
        let st = state.read().await;
        let mut c = st.hot_wallet.config().await;
        c.cold_wallet_address = address;
        st.hot_wallet.update_config(c).await;
    });
}

pub fn hot_wallet_status() -> HotWalletStatus {
    let state = state_arc();
    runtime().block_on(async {
        let st = state.read().await;
        let (conf, pend) = st.coin_node.balance().unwrap_or((0, 0));
        let cfg = st.hot_wallet.config().await;
        let net = *st.connectivity_mode.lock().expect("connectivity lock");
        let needs_refill = cfg.cold_wallet_address.is_some() && conf < cfg.refill_threshold_muon;
        HotWalletStatus {
            confirmed_muon: conf,
            pending_muon: pend,
            max_cache_muon: cfg.max_cache_muon,
            auto_refill_enabled: cfg.cold_wallet_address.is_some(),
            needs_refill,
            connectivity: net.into(),
        }
    })
}

pub fn current_connectivity_mode() -> ConnectivityMode {
    with_state(|s| {
        let net = *s.connectivity_mode.lock().expect("connectivity lock");
        net.into()
    })
}

pub fn build_payment_request_uri(
    to_address: String,
    amount_muon: u64,
    memo: Option<String>,
) -> String {
    PaymentRequest::new(to_address, amount_muon, memo).to_uri()
}

pub fn parse_payment_request_uri(uri: String) -> Option<PaymentRequestData> {
    let p = PaymentRequest::from_uri(&uri).ok()?;
    if p.is_expired() {
        return None;
    }
    Some(PaymentRequestData {
        to_address: p.to_address,
        amount_muon: p.amount_muon,
        memo: p.memo,
        expires_at_ms: p.expires_at_ms,
    })
}

pub fn miniapp_issue_bridge_session(app_id: String) -> String {
    mycelium_app::miniapp::issue_session(&app_id)
}

pub fn miniapp_revoke_bridge_session(app_id: String) {
    mycelium_app::miniapp::revoke_session(&app_id);
}

pub fn miniapp_issue_capability(
    app_id: String,
    permission: String,
    session_token: String,
) -> String {
    with_state(|s| {
        let perm = mycelium_app::miniapp::parse_permission_name(&permission)
            .unwrap_or_else(|| panic!("unknown permission: {permission}"));
        mycelium_app::miniapp::issue_capability(
            &s.cap_mac_key,
            &app_id,
            &perm,
            &session_token,
            mycelium_app::miniapp::DEFAULT_CAP_TTL_MS,
        )
    })
}

pub fn miniapp_publish_revocation(app_id: String, reason: String) {
    runtime().block_on(async {
        let st = state_arc();
        let guard = st.read().await;
        let keypair = mycelium_node::load_or_create_keypair(&guard.coin_identity_path)
            .expect("load identity keypair for revocation");
        let entry = guard
            .app_store
            .build_revocation_gossip(&app_id, &reason, &guard.local_peer_id, &keypair)
            .expect("build revocation gossip");
        guard
            .app_node
            .publish_app_revocation(&entry)
            .await
            .expect("publish revocation gossip");
    });
}

pub fn miniapp_get_policy(app_id: String) -> MiniAppPolicyData {
    with_state(|s| {
        let p = s
            .app_store
            .policy_snapshot(&app_id)
            .expect("miniapp policy");
        MiniAppPolicyData {
            app_id: p.app_id,
            reputation_score: p.reputation_score,
            safe_mode_active: p.safe_mode_active,
            safe_mode_forced: p.safe_mode_forced,
            safe_mode_suggested: p.safe_mode_suggested,
            user_safe_mode: p.user_safe_mode,
            revoked: p.revoked,
            strict_csp_eligible: p.strict_csp_eligible,
        }
    })
}

pub fn miniapp_set_safe_mode(app_id: String, enabled: bool) {
    with_state(|s| {
        s.app_store
            .set_user_safe_mode(&app_id, enabled)
            .expect("set safe mode");
    });
}

pub fn miniapp_report_app(app_id: String) {
    with_state(|s| {
        s.app_store.record_user_report(&app_id).expect("report app");
    });
}

fn mini_app_manifest_to_info(m: mycelium_app::miniapp::MiniAppManifest) -> MiniAppInfo {
    MiniAppInfo {
        id: m.id,
        name: m.name,
        description: m.description,
        version: m.version,
        developer: m.developer,
        entry: m.entry,
        accepts_payments: m.accepts_payments,
        payment_address: m.payment_address,
        permissions: m.permissions.iter().map(|p| format!("{p:?}")).collect(),
    }
}

pub fn list_installed_apps() -> Vec<MiniAppInfo> {
    with_state(|s| {
        s.app_store
            .installed_apps()
            .unwrap_or_default()
            .into_iter()
            .map(mini_app_manifest_to_info)
            .collect()
    })
}

fn trust_level_name(level: mycelium_app::miniapp::InstallTrustLevel) -> String {
    use mycelium_app::miniapp::InstallTrustLevel;
    match level {
        InstallTrustLevel::VerifiedListing => "verified_listing".to_string(),
        InstallTrustLevel::MatchingListingHash => "matching_listing_hash".to_string(),
        InstallTrustLevel::SideloadOnly => "sideload_only".to_string(),
        InstallTrustLevel::HashMismatch => "hash_mismatch".to_string(),
    }
}

pub fn preview_miniapp_install(
    bundle_data: Vec<u8>,
) -> Result<MiniAppInstallPreview, MyceliumException> {
    with_state(|s| {
        let p = s.app_store.preview_install(&bundle_data).map_err(|e| {
            MyceliumException::InstallError {
                detail: e.to_string(),
            }
        })?;
        Ok(MiniAppInstallPreview {
            manifest: mini_app_manifest_to_info(p.manifest),
            bundle_hash: p.bundle_hash,
            trust_level: trust_level_name(p.trust_level),
            listing_signature_ok: p.listing_signature_ok,
            installed_version: p.installed_version,
            is_downgrade: p.is_downgrade,
            has_inline_script: p.has_inline_script,
            strict_csp_eligible: p.strict_csp_eligible,
            reproducible_attested: p.reproducible_attested,
        })
    })
}

pub fn install_app(
    bundle_data: Vec<u8>,
    listing_app_id: Option<String>,
    allow_sideload: bool,
    allow_downgrade: bool,
) -> Result<MiniAppInfo, MyceliumException> {
    with_state(|s| {
        let trust = if listing_app_id.is_some() {
            mycelium_app::miniapp::InstallTrust::VerifiedListing
        } else if allow_sideload {
            mycelium_app::miniapp::InstallTrust::SideloadAcknowledged
        } else {
            return Err(MyceliumException::InstallError {
                detail: "listing_app_id or allow_sideload required".into(),
            });
        };
        if let Some(ref id) = listing_app_id {
            let preview = s.app_store.preview_install(&bundle_data).map_err(|e| {
                MyceliumException::InstallError {
                    detail: e.to_string(),
                }
            })?;
            if preview.manifest.id != *id {
                return Err(MyceliumException::InstallError {
                    detail: "bundle app id does not match listing_app_id".into(),
                });
            }
        }
        let m = s
            .app_store
            .install_verified(&bundle_data, trust, allow_downgrade)
            .map_err(|e| MyceliumException::InstallError {
                detail: e.to_string(),
            })?;
        Ok(mini_app_manifest_to_info(m))
    })
}

pub fn uninstall_app(app_id: String) {
    mycelium_app::miniapp::revoke_session(&app_id);
    with_state(|s| {
        s.app_store.uninstall(&app_id).expect("uninstall mini-app");
        let _ = s.app_node.storage().miniapp_clear_all_for_app(&app_id);
    });
}

pub fn get_app_file(app_id: String, path: String) -> Option<Vec<u8>> {
    with_state(|s| s.app_store.get_file(&app_id, &path).ok().flatten())
}

pub fn browse_app_store() -> Vec<AppStoreListing> {
    with_state(|s| {
        s.app_store
            .browse_listings()
            .unwrap_or_default()
            .into_iter()
            .map(|row| {
                let signature_valid = row.verify_signature().unwrap_or(false);
                AppStoreListing {
                    manifest: mini_app_manifest_to_info(row.manifest),
                    bundle_hash: row.bundle_hash,
                    updated_at_ms: row.updated_at_ms,
                    signature_valid,
                }
            })
            .collect()
    })
}

pub fn miniapp_storage_get(app_id: String, key: String) -> Option<String> {
    with_state(|s| {
        let sk = format!("app:{app_id}:{key}");
        s.app_node.storage().miniapp_get(&sk).ok().flatten()
    })
}

pub fn miniapp_storage_set(app_id: String, key: String, value: String) {
    use mycelium_app::miniapp::storage_quota::{validate_key, validate_value};
    with_state(|s| {
        if let Err(e) = validate_key(&key) {
            panic!("miniapp storage set: {e}");
        }
        if let Err(e) = validate_value(&value) {
            panic!("miniapp storage set: {e}");
        }
        let sk = format!("app:{app_id}:{key}");
        s.app_node
            .storage()
            .miniapp_set(&sk, &value)
            .expect("miniapp storage set");
    });
}

pub fn miniapp_storage_delete(app_id: String, key: String) {
    with_state(|s| {
        let sk = format!("app:{app_id}:{key}");
        s.app_node
            .storage()
            .miniapp_delete(&sk)
            .expect("miniapp storage delete");
    });
}

pub fn miniapp_bridge_call(app_id: String, method: String, args_json: String) -> String {
    use serde_json::json;
    use serde_json::Value;
    runtime().block_on(async {
        let st = state_arc();
        let guard = st.read().await;
        let args: Value = serde_json::from_str(&args_json).unwrap_or(Value::Null);
        let manifest = guard.app_store.get_manifest(&app_id).ok().flatten();
        let permissions = manifest.map(|m| m.permissions).unwrap_or_default();
        let enc = guard.app_node.node_handle().local_enc_pubkey_hex();
        let safe_mode = guard
            .app_store
            .effective_safe_mode(&app_id)
            .unwrap_or(false);
        let host = mycelium_app::miniapp::bridge_host::BridgeHost::new(
            app_id,
            permissions,
            guard.app_node.clone(),
            guard.app_node.storage(),
            guard.app_store.clone(),
            Some(guard.coin_node.clone()),
            guard.local_peer_id.clone(),
            enc,
            safe_mode,
            guard.cap_mac_key,
        );
        match host.handle(&method, &args).await {
            Ok(v) => serde_json::to_string(&v).unwrap_or_else(|_| "{}".to_string()),
            Err(e) => {
                serde_json::to_string(&json!({ "error": e })).unwrap_or_else(|_| "{}".to_string())
            }
        }
    })
}

fn tx_to_info(t: mycelium_coin::Transaction) -> TxInfo {
    let witness_count = t.witnesses.len() as u32;
    let confirmed = witness_count >= 3;
    TxInfo {
        id: t.id,
        from_address: t.from,
        to_address: t.to,
        amount_muon: t.amount_muon,
        fee_muon: t.fee_muon,
        timestamp_ms: t.timestamp_ms,
        memo: t.memo,
        witness_count,
        confirmed,
    }
}

fn with_state<T>(f: impl FnOnce(&NodeState) -> T) -> T {
    runtime().block_on(async {
        let state = state_arc();
        let guard = state.read().await;
        f(&guard)
    })
}

fn runtime() -> &'static Runtime {
    RUNTIME.get().expect("runtime not initialized")
}

fn state_arc() -> Arc<RwLock<NodeState>> {
    NODE.lock()
        .expect("node lock")
        .as_ref()
        .cloned()
        .expect("node not initialized")
}

fn to_chat_message(m: AppChatMessage, from_peer: String) -> ChatMessage {
    ChatMessage {
        id: m.id.to_string(),
        from_peer,
        from_display_name: m.from_display_name,
        body: m.body,
        timestamp_ms: m.timestamp_ms,
    }
}

fn to_app_store_listing(l: mycelium_app::miniapp::store::AppStoreListing) -> AppStoreListing {
    let signature_valid = l.verify_signature().unwrap_or(false);
    AppStoreListing {
        manifest: mini_app_manifest_to_info(l.manifest),
        bundle_hash: l.bundle_hash,
        updated_at_ms: l.updated_at_ms,
        signature_valid,
    }
}

fn to_bulletin(p: AppBulletinPost) -> BulletinPost {
    BulletinPost {
        id: p.id.to_string(),
        from_display_name: p.from_display_name,
        title: p.title,
        body: p.body,
        scope: p.scope,
        timestamp_ms: p.timestamp_ms,
        expires_at_ms: p.expires_at_ms,
    }
}

fn to_mail_message(m: AppMailMessage, is_read: bool) -> MailMessage {
    MailMessage {
        id: m.id.to_string(),
        from_peer: m.from_peer,
        from_display_name: m.from_display_name,
        to_peer: m.to_peer,
        subject: m.subject,
        body: m.body,
        timestamp_ms: m.timestamp_ms,
        is_read,
    }
}
