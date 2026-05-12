use crate::forwarding::{
    EnergyPolicy, ForwardDecision, ForwardingPolicy, IngestPipeline, ProbabilisticForwardingPolicy,
    SeenCache, SimpleEnergyPolicy,
};
use crate::storage::SledMessageStore;
use crate::transport::Libp2pTransport;
use libp2p::{identity, Multiaddr, PeerId};
use mycelium_core::bootstrap;
use mycelium_core::data::{now_ms, Priority};
use mycelium_core::energy::NodeState;
use mycelium_core::sync::BloomFilter;
use mycelium_core::transport::{
    ConnectivityMode, DirectMessage, MeshTransport, MessageStore, Scope, TransportEvent,
    WireMessage,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio::time::{interval, MissedTickBehavior};
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub listen_addr: Multiaddr,
    pub db_path: String,
    pub keypair_path: Option<String>,
    pub forwarding_interval_ms: u64,
    pub sync_interval_secs: u64,
    /// Multiaddrs (e.g. `/ip4/host/tcp/4001/p2p/<peer>`) dialed when [`ConnectivityMode::Internet`].
    pub bootstrap_peers: Vec<String>,
    /// When set, the libp2p transport emits [`TransportEvent::ConnectivityChanged`] on mode flips.
    pub connectivity_rx: Option<tokio::sync::watch::Receiver<ConnectivityMode>>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            listen_addr: "/ip4/0.0.0.0/tcp/0"
                .parse()
                .expect("default listen addr must parse"),
            db_path: ".mycelium-node".to_string(),
            keypair_path: Some(".mycelium-node/identity".to_string()),
            forwarding_interval_ms: 500,
            sync_interval_secs: 30,
            bootstrap_peers: bootstrap::default_peer_multiaddrs(),
            connectivity_rx: None,
        }
    }
}

#[derive(Debug)]
pub enum NodeCommand {
    SendDirect {
        to_peer: String,
        body: String,
    },
    SendDirectPayload {
        to_peer: String,
        body: String,
        payload: Vec<u8>,
    },
    Broadcast {
        scope: String,
        body: String,
    },
    BroadcastPayload {
        scope: String,
        body: String,
        payload: Vec<u8>,
    },
    AddBootstrapPeer {
        multiaddr: String,
    },
    SubscribeScope(String),
    UnsubscribeScope(String),
    GcNow,
    StoreStats,
    ListPeers,
    SetEnergyState(NodeState),
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct NodeMetrics {
    pub messages_forwarded: u64,
    pub messages_dropped_ttl: u64,
    pub messages_dropped_hops: u64,
    pub messages_dropped_queue: u64,
    pub messages_delivered_local: u64,
    pub pending_queue_size: usize,
    pub seen_cache_size: usize,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PeerReputationSnapshot {
    pub strikes: u8,
    pub last_strike_ms: u64,
    pub throttled_until_ms: u64,
}

#[derive(Clone)]
pub struct NodeHandle {
    tx: mpsc::Sender<NodeCommand>,
    metrics: Arc<RwLock<NodeMetrics>>,
    incoming_tx: broadcast::Sender<DirectMessage>,
    peers: Arc<RwLock<Vec<String>>>,
    reputations: Arc<Mutex<HashMap<String, PeerReputationSnapshot>>>,
    scopes: Arc<RwLock<HashSet<String>>>,
}

impl NodeHandle {
    pub async fn send(&self, cmd: NodeCommand) -> anyhow::Result<()> {
        self.tx.send(cmd).await?;
        Ok(())
    }

    pub async fn metrics(&self) -> NodeMetrics {
        self.metrics.read().await.clone()
    }

    pub fn subscribe_incoming(&self) -> broadcast::Receiver<DirectMessage> {
        self.incoming_tx.subscribe()
    }

    pub async fn known_peers(&self) -> Vec<String> {
        self.peers.read().await.clone()
    }

    pub async fn peer_reputation(&self, peer_id: &str) -> Option<PeerReputationSnapshot> {
        self.reputations
            .lock()
            .expect("reputation lock")
            .get(peer_id)
            .cloned()
    }

    pub async fn subscribed_scopes(&self) -> Vec<String> {
        self.scopes.read().await.iter().cloned().collect()
    }
}

pub struct NodeRunner {
    transport: Box<dyn MeshTransport>,
    keypair: Option<identity::Keypair>,
    local_peer_id: String,
    store: Arc<dyn MessageStore>,
    ingest: IngestPipeline,
    seen_cache: BasicSeenCache,
    node_state: NodeState,
    forwarding_policy: ProbabilisticForwardingPolicy,
    energy_policy: SimpleEnergyPolicy,
    pending: VecDeque<PendingMessage>,
    pending_cap: usize,
    forwarding_interval_ms: u64,
    sync_interval_secs: u64,
    metrics: Arc<RwLock<NodeMetrics>>,
    incoming_tx: broadcast::Sender<DirectMessage>,
    peers: Arc<RwLock<Vec<String>>>,
    subscribed_scopes: Arc<RwLock<HashSet<String>>>,
    peer_scopes: HashMap<String, HashSet<String>>,
    peer_reputations: Arc<Mutex<HashMap<String, PeerReputationSnapshot>>>,
    cmd_rx: mpsc::Receiver<NodeCommand>,
}

impl NodeRunner {
    pub fn new(config: NodeConfig) -> anyhow::Result<(Self, NodeHandle)> {
        let keypair_path = config
            .keypair_path
            .clone()
            .or_else(|| Some(format!("{}/identity", config.db_path)));
        let transport = Libp2pTransport::new(
            config.listen_addr.clone(),
            keypair_path,
            config.bootstrap_peers.clone(),
            config.connectivity_rx.clone(),
        )?;
        Self::new_with_transport(config, Box::new(transport))
    }

    pub fn new_with_transport(
        config: NodeConfig,
        transport: Box<dyn MeshTransport>,
    ) -> anyhow::Result<(Self, NodeHandle)> {
        let keypair = transport.local_keypair();
        let local_peer_id = transport.local_peer_id();
        let store = Arc::new(SledMessageStore::open(&config.db_path)?);
        let ingest = IngestPipeline::new(store.clone());
        let metrics = Arc::new(RwLock::new(NodeMetrics::default()));
        let peers = Arc::new(RwLock::new(Vec::new()));
        let reputations = Arc::new(Mutex::new(HashMap::new()));
        let scopes = Arc::new(RwLock::new(HashSet::from(["mycelium/chat".to_string()])));
        let (incoming_tx, _) = broadcast::channel(512);
        let (tx, cmd_rx) = mpsc::channel(64);
        Ok((
            Self {
                local_peer_id,
                transport,
                keypair,
                store,
                ingest,
                seen_cache: BasicSeenCache::new(16_384),
                node_state: NodeState::Active,
                forwarding_policy: ProbabilisticForwardingPolicy,
                energy_policy: SimpleEnergyPolicy,
                pending: VecDeque::new(),
                pending_cap: 1024,
                forwarding_interval_ms: config.forwarding_interval_ms,
                sync_interval_secs: config.sync_interval_secs,
                metrics: metrics.clone(),
                incoming_tx: incoming_tx.clone(),
                peers: peers.clone(),
                subscribed_scopes: scopes.clone(),
                peer_scopes: HashMap::new(),
                peer_reputations: reputations.clone(),
                cmd_rx,
            },
            NodeHandle {
                tx,
                metrics,
                incoming_tx,
                peers,
                reputations,
                scopes,
            },
        ))
    }

    pub fn local_peer_id(&self) -> &str {
        &self.local_peer_id
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        info!("node started with peer_id={}", self.local_peer_id);
        let mut forward_tick =
            interval(Duration::from_millis(self.config_forwarding_interval_ms()));
        forward_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut sync_tick = interval(Duration::from_secs(self.config_sync_interval_secs()));
        sync_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut gc_tick = interval(Duration::from_secs(6 * 60 * 60));
        gc_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                transport_event = self.transport.next_event() => {
                    self.handle_transport_event(transport_event?).await?;
                }
                Some(cmd) = self.cmd_rx.recv() => {
                    self.handle_command(cmd).await?;
                }
                _ = forward_tick.tick() => {
                    self.forward_once().await?;
                }
                _ = sync_tick.tick() => {
                    self.sync_known_peers().await?;
                }
                _ = gc_tick.tick() => {
                    let deleted = self.store.gc_expired().await?;
                    if deleted > 0 {
                        info!("GC: deleted {} expired messages", deleted);
                    }
                }
            }
        }
    }

    async fn handle_command(&mut self, cmd: NodeCommand) -> anyhow::Result<()> {
        match cmd {
            NodeCommand::SendDirect { to_peer, body } => {
                self.send_direct_internal(to_peer, body.clone(), body.into_bytes())
                    .await?;
            }
            NodeCommand::SendDirectPayload {
                to_peer,
                body,
                payload,
            } => {
                self.send_direct_internal(to_peer, body, payload).await?;
            }
            NodeCommand::Broadcast { scope, body } => {
                self.broadcast_scoped(scope, body.clone(), body.into_bytes())
                    .await?;
            }
            NodeCommand::BroadcastPayload {
                scope,
                body,
                payload,
            } => {
                self.broadcast_scoped(scope, body, payload).await?;
            }
            NodeCommand::ListPeers => {
                let peers = self.transport.known_peers();
                *self.peers.write().await = peers.clone();
                info!("known peers: {}", peers.len());
                for peer in peers {
                    info!("peer {}", peer);
                }
            }
            NodeCommand::AddBootstrapPeer { multiaddr } => {
                self.transport.dial_peer(multiaddr).await?;
            }
            NodeCommand::SubscribeScope(scope) => {
                self.subscribed_scopes.write().await.insert(scope);
            }
            NodeCommand::UnsubscribeScope(scope) => {
                self.subscribed_scopes.write().await.remove(&scope);
            }
            NodeCommand::GcNow => {
                let deleted = self.store.gc_expired().await?;
                info!("manual GC deleted {} messages", deleted);
            }
            NodeCommand::StoreStats => {
                let stats = self.store.stats().await?;
                info!(
                    "store stats: count={} oldest_ms={}",
                    stats.count, stats.oldest_ms
                );
            }
            NodeCommand::SetEnergyState(state) => {
                self.node_state = state;
                info!("energy state updated to {:?}", self.node_state);
            }
        }
        Ok(())
    }

    async fn send_direct_internal(
        &mut self,
        to_peer: String,
        body: String,
        payload: Vec<u8>,
    ) -> anyhow::Result<()> {
        let mut envelope = mycelium_core::data::Envelope::new(
            self.local_peer_id.clone(),
            Some(to_peer.clone()),
            payload,
        );
        if let Some(keypair) = &self.keypair {
            let _ = envelope.sign(keypair);
        }
        let msg = DirectMessage {
            envelope,
            body: body.clone(),
        };
        let _ = self
            .ingest
            .ingest(&mut self.seen_cache, &msg, false)
            .await?;
        let peers = self.transport.known_peers();
        if peers.iter().any(|p| p == &to_peer) {
            self.enqueue(to_peer.clone(), msg).await?;
            info!("direct message queued for {}", to_peer);
        } else if peers.is_empty() {
            warn!("no relay candidates available for {}", to_peer);
        } else {
            for peer in peers {
                self.enqueue(peer, msg.clone()).await?;
            }
            info!("message queued for relay toward {}", to_peer);
        }
        Ok(())
    }

    async fn broadcast_scoped(
        &mut self,
        scope: String,
        body: String,
        payload: Vec<u8>,
    ) -> anyhow::Result<()> {
        let targets = self.targets_for_scope(&scope);
        if targets.is_empty() {
            self.transport.publish_scoped(scope, payload).await?;
            return Ok(());
        }
        for to_peer in targets {
            let mut envelope = mycelium_core::data::Envelope::new(
                self.local_peer_id.clone(),
                None,
                payload.clone(),
            );
            if let Some(keypair) = &self.keypair {
                let _ = envelope.sign(keypair);
            }
            let msg = DirectMessage {
                envelope,
                body: body.clone(),
            };
            let _ = self
                .ingest
                .ingest(&mut self.seen_cache, &msg, false)
                .await?;
            self.enqueue(to_peer, msg).await?;
        }
        Ok(())
    }

    async fn handle_transport_event(&mut self, event: TransportEvent) -> anyhow::Result<()> {
        match event {
            TransportEvent::Listening { address } => {
                info!("listening on {address}");
            }
            TransportEvent::PeerUp { peer_id } => {
                info!("discovered peer={peer_id}");
                {
                    let mut peers = self.peers.write().await;
                    if !peers.iter().any(|p| p == &peer_id) {
                        peers.push(peer_id.clone());
                    }
                }
                self.send_scope_announce(peer_id.clone()).await?;
                self.send_sync_bloom(peer_id).await?;
            }
            TransportEvent::PeerDown { peer_id } => {
                info!("expired peer={peer_id}");
                {
                    let mut peers = self.peers.write().await;
                    peers.retain(|p| p != &peer_id);
                }
                self.peer_scopes.remove(&peer_id);
            }
            TransportEvent::DirectReceived { from_peer, message } => {
                self.handle_incoming_direct(from_peer, message).await?;
            }
            TransportEvent::DirectAck { from_peer, ack } => {
                info!(
                    "delivery ack from {} message_id={} accepted={}",
                    from_peer, ack.message_id, ack.accepted
                );
            }
            TransportEvent::SendFailure { to_peer, reason } => {
                warn!("direct send to {} failed: {}", to_peer, reason);
            }
            TransportEvent::ConnectivityChanged { mode } => {
                info!("connectivity mode from transport: {:?}", mode);
            }
            TransportEvent::ScopedReceived {
                from_peer,
                scope,
                payload,
            } => {
                let scope_obj = Scope(scope.clone());
                let scopes = self.subscribed_scopes.read().await;
                if !scopes.iter().any(|pattern| scope_obj.matches(pattern)) {
                    return Ok(());
                }
                if let Ok(body) = String::from_utf8(payload.clone()) {
                    let mut envelope =
                        mycelium_core::data::Envelope::new(from_peer.clone(), None, payload);
                    if let Some(keypair) = &self.keypair {
                        let _ = envelope.sign(keypair);
                    }
                    let stored = DirectMessage {
                        envelope,
                        body: body.clone(),
                    };
                    let should_process = self
                        .ingest
                        .ingest(&mut self.seen_cache, &stored, false)
                        .await?;
                    if should_process {
                        info!("gossip recv from {} [{}]: {}", from_peer, scope, body);
                    }
                }
            }
        }
        Ok(())
    }

    async fn handle_incoming_direct(
        &mut self,
        from_peer: String,
        message: WireMessage,
    ) -> anyhow::Result<()> {
        if self.is_throttled(&from_peer) {
            return Ok(());
        }
        match message {
            WireMessage::Data(mut message) => {
                if message.envelope.signature.is_some() {
                    let Ok(from_peer_id) = from_peer.parse::<PeerId>() else {
                        self.register_strike(&from_peer);
                        return Ok(());
                    };
                    if !message.envelope.verify(&from_peer_id) {
                        warn!("dropping unsigned/invalid message from {}", from_peer);
                        self.register_strike(&from_peer);
                        return Ok(());
                    }
                }
                if message.envelope.hop_count >= message.envelope.max_hops {
                    let mut metrics = self.metrics.write().await;
                    metrics.messages_dropped_hops += 1;
                    return Ok(());
                }
                let should_process = self
                    .ingest
                    .ingest(&mut self.seen_cache, &message, false)
                    .await?;
                if !should_process {
                    self.register_strike(&from_peer);
                    return Ok(());
                }
                self.reward_peer(&from_peer);
                let _ = self.incoming_tx.send(message.clone());
                info!("direct recv from {}: {}", from_peer, message.body);

                if message
                    .envelope
                    .to_peer
                    .as_deref()
                    .is_some_and(|to| to == self.local_peer_id)
                {
                    let mut metrics = self.metrics.write().await;
                    metrics.messages_delivered_local += 1;
                    return Ok(());
                }
                if message.is_expired(now_ms()) {
                    let mut metrics = self.metrics.write().await;
                    metrics.messages_dropped_ttl += 1;
                    return Ok(());
                }
                if !self
                    .energy_policy
                    .can_forward(self.node_state, message.envelope.priority)
                {
                    return Ok(());
                }

                let mut peers = self.transport.known_peers();
                peers.retain(|peer| peer != &from_peer);
                if let Some(target) = message.envelope.to_peer.clone() {
                    if peers.iter().any(|p| p == &target) {
                        message.envelope.hop_count = message.envelope.hop_count.saturating_add(1);
                        self.enqueue(target, message).await?;
                        return Ok(());
                    }
                }
                let decision = self.forwarding_policy.decide(
                    &message,
                    &self.local_peer_id,
                    &peers,
                    self.node_state,
                );
                debug!(
                    "forward decision for message_id={} is {:?}",
                    message.envelope.id.0, decision
                );
                match decision {
                    ForwardDecision::ForwardTo(targets) => {
                        for target in targets {
                            let mut forwarded = message.clone();
                            forwarded.envelope.hop_count =
                                forwarded.envelope.hop_count.saturating_add(1);
                            self.enqueue(target, forwarded).await?;
                        }
                    }
                    ForwardDecision::Drop(reason) => {
                        warn!(
                            "dropping message_id={} reason={}",
                            message.envelope.id.0, reason
                        );
                    }
                    _ => {}
                }
            }
            WireMessage::SyncBloom { bloom, count } => {
                let Some(remote_bloom) = BloomFilter::from_bytes(&bloom) else {
                    return Ok(());
                };
                let own_ids = self.store.list_ids_window(Duration::from_secs(600)).await?;
                let mut missing_remote = Vec::new();
                let mut missing_local = Vec::new();
                for id in &own_ids {
                    if !remote_bloom.contains(id) {
                        missing_remote.push(id.clone());
                    }
                }
                if own_ids.len() as f64 > (count as f64 * 0.8) && !missing_remote.is_empty() {
                    self.transport
                        .send_direct(
                            from_peer.clone(),
                            WireMessage::SyncIds {
                                ids: missing_remote,
                            },
                        )
                        .await?;
                }
                for id in own_ids.iter().take(256) {
                    if !self.store.contains(id).await? {
                        missing_local.push(id.clone());
                    }
                }
                if !missing_local.is_empty() {
                    self.transport
                        .send_direct(from_peer, WireMessage::SyncRequest { ids: missing_local })
                        .await?;
                }
            }
            WireMessage::SyncIds { ids } => {
                let mut missing = Vec::new();
                for id in ids.iter().take(256) {
                    if !self.store.contains(id).await? {
                        missing.push(id.clone());
                    }
                }
                if !missing.is_empty() {
                    self.transport
                        .send_direct(from_peer, WireMessage::SyncRequest { ids: missing })
                        .await?;
                }
            }
            WireMessage::SyncRequest { ids } => {
                let mut messages = Vec::new();
                for id in ids.iter().take(128) {
                    if let Some(missing) = self.store.load_by_id(id).await? {
                        messages.push(missing);
                    }
                }
                if !messages.is_empty() {
                    self.transport
                        .send_direct(from_peer, WireMessage::SyncData { messages })
                        .await?;
                }
            }
            WireMessage::SyncData { messages } => {
                for message in messages {
                    let _ = self
                        .ingest
                        .ingest(&mut self.seen_cache, &message, false)
                        .await?;
                }
            }
            WireMessage::ScopeAnnounce { scopes } => {
                self.peer_scopes
                    .insert(from_peer, scopes.into_iter().collect::<HashSet<_>>());
            }
        }
        Ok(())
    }

    async fn forward_once(&mut self) -> anyhow::Result<()> {
        self.evict_expired();
        let budget = match self.node_state {
            NodeState::Active => 10,
            NodeState::Intermittent => 4,
            NodeState::Passive => 1,
        };
        for _ in 0..budget {
            let Some(pending) = self.pending.pop_front() else {
                break;
            };
            if pending.message.is_expired(now_ms()) {
                continue;
            }
            if !self
                .energy_policy
                .can_forward(self.node_state, pending.message.envelope.priority)
            {
                continue;
            }
            if let Err(err) = self
                .transport
                .send_direct(
                    pending.to_peer.clone(),
                    WireMessage::Data(pending.message.clone()),
                )
                .await
            {
                warn!("forward send failed to {}: {}", pending.to_peer, err);
            } else {
                let mut metrics = self.metrics.write().await;
                metrics.messages_forwarded += 1;
            }
        }
        let mut metrics = self.metrics.write().await;
        metrics.pending_queue_size = self.pending.len();
        metrics.seen_cache_size = self.seen_cache.len();
        Ok(())
    }

    async fn sync_known_peers(&mut self) -> anyhow::Result<()> {
        let peers = self.transport.known_peers();
        for peer in peers {
            self.send_sync_bloom(peer).await?;
        }
        Ok(())
    }

    async fn send_sync_bloom(&mut self, peer_id: String) -> anyhow::Result<()> {
        let ids = self.store.list_ids_window(Duration::from_secs(600)).await?;
        if ids.is_empty() {
            return Ok(());
        }
        let mut bloom = BloomFilter::new();
        for id in &ids {
            bloom.insert(id);
        }
        self.transport
            .send_direct(
                peer_id,
                WireMessage::SyncBloom {
                    bloom: bloom.to_bytes(),
                    count: ids.len() as u64,
                },
            )
            .await
    }

    async fn send_scope_announce(&mut self, peer_id: String) -> anyhow::Result<()> {
        let scopes = self
            .subscribed_scopes
            .read()
            .await
            .iter()
            .cloned()
            .collect();
        self.transport
            .send_direct(peer_id, WireMessage::ScopeAnnounce { scopes })
            .await
    }

    async fn enqueue(&mut self, to_peer: String, message: DirectMessage) -> anyhow::Result<()> {
        if message.is_expired(now_ms()) {
            return Ok(());
        }
        if self.pending.len() >= self.pending_cap {
            self.evict_one();
        }
        if self.pending.len() >= self.pending_cap {
            let mut metrics = self.metrics.write().await;
            metrics.messages_dropped_queue += 1;
            return Err(anyhow::anyhow!("pending queue is full"));
        }
        self.pending.push_back(PendingMessage { to_peer, message });
        let mut metrics = self.metrics.write().await;
        metrics.pending_queue_size = self.pending.len();
        metrics.seen_cache_size = self.seen_cache.len();
        Ok(())
    }

    fn evict_expired(&mut self) {
        let now = now_ms();
        self.pending.retain(|item| !item.message.is_expired(now));
    }

    fn evict_one(&mut self) {
        self.evict_expired();
        if self.pending.len() < self.pending_cap {
            return;
        }

        let mut candidate_idx = None;
        let mut candidate_rank = 3u8;
        for (idx, item) in self.pending.iter().enumerate() {
            let rank = priority_rank(item.message.envelope.priority);
            if rank < candidate_rank {
                candidate_rank = rank;
                candidate_idx = Some(idx);
                if rank == 0 {
                    break;
                }
            }
        }
        if let Some(idx) = candidate_idx {
            let _ = self.pending.remove(idx);
        } else {
            let _ = self.pending.pop_front();
        }
    }
}

impl NodeRunner {
    fn config_forwarding_interval_ms(&self) -> u64 {
        self.forwarding_interval_ms
    }

    fn config_sync_interval_secs(&self) -> u64 {
        self.sync_interval_secs
    }

    fn targets_for_scope(&self, scope: &str) -> Vec<String> {
        let scope_obj = Scope(scope.to_string());
        self.peer_scopes
            .iter()
            .filter_map(|(peer, scopes)| {
                if scopes.iter().any(|pattern| scope_obj.matches(pattern)) {
                    Some(peer.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    fn is_throttled(&self, peer: &str) -> bool {
        let now = now_ms();
        let mut reps = self.peer_reputations.lock().expect("reputation lock");
        if let Some(rep) = reps.get_mut(peer) {
            if now.saturating_sub(rep.last_strike_ms) > 10 * 60 * 1000 {
                rep.strikes = 0;
            }
            return now < rep.throttled_until_ms;
        }
        false
    }

    fn register_strike(&self, peer: &str) {
        let now = now_ms();
        let mut reps = self.peer_reputations.lock().expect("reputation lock");
        let rep = reps.entry(peer.to_string()).or_default();
        if now.saturating_sub(rep.last_strike_ms) > 60 * 1000 {
            rep.strikes = 0;
        }
        rep.strikes = rep.strikes.saturating_add(1);
        rep.last_strike_ms = now;
        if rep.strikes >= 3 {
            rep.throttled_until_ms = now + 5 * 60 * 1000;
        }
    }

    fn reward_peer(&self, peer: &str) {
        let mut reps = self.peer_reputations.lock().expect("reputation lock");
        if let Some(rep) = reps.get_mut(peer) {
            rep.strikes /= 2;
        }
    }
}

struct BasicSeenCache {
    cap: usize,
    order: VecDeque<String>,
    set: HashSet<String>,
}

impl BasicSeenCache {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            order: VecDeque::new(),
            set: HashSet::new(),
        }
    }

    fn len(&self) -> usize {
        self.set.len()
    }
}

impl SeenCache for BasicSeenCache {
    fn contains(&self, message_id: &str) -> bool {
        self.set.contains(message_id)
    }

    fn insert(&mut self, message_id: String) {
        if self.set.insert(message_id.clone()) {
            self.order.push_back(message_id);
        }
        while self.order.len() > self.cap {
            if let Some(evicted) = self.order.pop_front() {
                self.set.remove(&evicted);
            }
        }
    }
}

#[derive(Clone)]
struct PendingMessage {
    to_peer: String,
    message: DirectMessage,
}

fn priority_rank(priority: Priority) -> u8 {
    match priority {
        Priority::Low => 0,
        Priority::Normal => 1,
        Priority::High => 2,
    }
}
