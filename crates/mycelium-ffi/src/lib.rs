#![allow(clippy::empty_line_after_doc_comments)]

use async_trait::async_trait;
use mycelium_app::envelope::{
    AppMessage, BulletinPost as AppBulletinPost, ChatMessage as AppChatMessage,
    MailMessage as AppMailMessage,
};
use mycelium_app::node::AppNode;
use mycelium_app::notify::NoopNotifier;
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

uniffi::include_scaffolding!("mycelium");

#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub db_path: String,
    pub listen_addr: String,
    pub display_name: String,
    /// Empty: [`NodeRunner::new`] loads peers via [`mycelium_core::bootstrap::load_bootstrap_peers`].
    pub bootstrap_peers: Vec<String>,
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

pub trait NodeEventCallback: Send + Sync {
    fn on_peer_discovered(&self, _peer_id: String) {}
    fn on_peer_lost(&self, _peer_id: String) {}
    fn on_chat_received(&self, _message: ChatMessage) {}
    fn on_mail_received(&self, _message: MailMessage) {}
    fn on_bulletin_received(&self, _post: BulletinPost) {}
    fn on_connectivity_changed(&self, _mode: ConnectivityMode) {}
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
    coin_node: Arc<CoinNode>,
    coin_identity_path: String,
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
            let _ = tracing_android::init("mycelium");
        }

        let connectivity = ConnectivityMonitor::new();
        ConnectivityMonitor::spawn_monitor(connectivity.mode_tx.clone());
        let connectivity_rx_node = connectivity.mode_rx.clone();
        let mut connectivity_rx_bg = connectivity.mode_rx.clone();

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
        };

        let (runner, handle) =
            NodeRunner::new(rust_config).expect("failed to initialize node runner");
        let local_peer_id = runner.local_peer_id().to_string();
        let runner_task = tokio::spawn(async move { runner.run().await });

        let app_storage = Arc::new(
            AppStorage::open(&format!("{}/app", config.db_path))
                .expect("failed to open app storage"),
        );
        let coin_identity_path = format!("{}/identity", config.db_path);
        let coin_addr = address_from_keypair(
            &mycelium_node::load_or_create_keypair(&coin_identity_path)
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
            Arc::new(NoopNotifier),
            Some(coin_node.clone()),
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
                }
            }
        });

        let state = Arc::new(RwLock::new(NodeState {
            handle,
            app_node,
            coin_node,
            coin_identity_path,
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

pub fn send_chat_direct(to_peer: String, body: String) {
    let state = state_arc();
    runtime().block_on(async {
        let state = state.read().await;
        let _ = state.app_node.send_chat(Some(to_peer), body).await;
    });
}

pub fn send_chat_broadcast(body: String) {
    let state = state_arc();
    runtime().block_on(async {
        let state = state.read().await;
        let _ = state.app_node.send_chat(None, body).await;
    });
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
    PaymentRequest::from_uri(&uri)
        .ok()
        .map(|p| PaymentRequestData {
            to_address: p.to_address,
            amount_muon: p.amount_muon,
            memo: p.memo,
            expires_at_ms: p.expires_at_ms,
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
