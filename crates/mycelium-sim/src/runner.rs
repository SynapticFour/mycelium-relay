// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use crate::clock::{DeterministicClock, VirtualClock};
use crate::scheduler::{EventScheduler, LinkProfile};
use crate::transport::{SimAction, SimTransport};
use mycelium_core::transport::{TransportEvent, WireMessage};
use mycelium_node::{NodeConfig, NodeHandle, NodeRunner};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct NodeAvailability {
    pub online: bool,
}

impl Default for NodeAvailability {
    fn default() -> Self {
        Self { online: true }
    }
}

#[derive(Debug, Clone)]
pub struct SimNodeSpec {
    pub peer_id: String,
    pub availability: NodeAvailability,
    pub bandwidth_bytes_per_sec: u64,
}

#[derive(Debug)]
pub enum ScheduledAction {
    Deliver {
        to_peer: String,
        event: TransportEvent,
        bytes: usize,
    },
    Availability {
        peer_id: String,
        online: bool,
    },
}

#[derive(Debug, Default, Clone)]
pub struct SimulationMetrics {
    pub delivered: u64,
    pub duplicates: u64,
    pub dropped_loss: u64,
    pub dropped_offline: u64,
    pub dropped_bandwidth: u64,
    pub sent_bytes: u64,
    pub latencies_ms: Vec<u64>,
}

pub struct SimulationRunner {
    pub clock: DeterministicClock,
    pub scheduler: EventScheduler<ScheduledAction>,
    pub links: Arc<Mutex<HashMap<(String, String), LinkProfile>>>,
    pub nodes: HashMap<String, SimNodeSpec>,
    pub event_txs: HashMap<String, mpsc::UnboundedSender<TransportEvent>>,
    pub bandwidth_usage: HashMap<(String, u64), u64>,
    pub delivered_ids: HashMap<String, u64>,
    pub metrics: SimulationMetrics,
    rng_state: u64,
    /// Isolates sled DB paths per simulation run (avoids stale encrypted state in /tmp).
    sim_run_id: u64,
    pub action_tx: mpsc::UnboundedSender<SimAction>,
    pub action_rx: mpsc::UnboundedReceiver<SimAction>,
}

pub struct SimNode {
    pub handle: NodeHandle,
    pub runner_task: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl SimulationRunner {
    pub fn new(start_ms: u64, seed: u64) -> Self {
        let (action_tx, action_rx) = mpsc::unbounded_channel();
        Self {
            clock: DeterministicClock::new(start_ms),
            scheduler: EventScheduler::default(),
            links: Arc::new(Mutex::new(HashMap::new())),
            nodes: HashMap::new(),
            event_txs: HashMap::new(),
            bandwidth_usage: HashMap::new(),
            delivered_ids: HashMap::new(),
            metrics: SimulationMetrics::default(),
            rng_state: seed.max(1),
            sim_run_id: seed,
            action_tx,
            action_rx,
        }
    }

    pub fn register_node(
        &mut self,
        peer_id: String,
        bandwidth_bytes_per_sec: u64,
    ) -> mpsc::UnboundedReceiver<TransportEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.event_txs.insert(peer_id.clone(), tx);
        self.nodes.insert(
            peer_id.clone(),
            SimNodeSpec {
                peer_id,
                availability: NodeAvailability::default(),
                bandwidth_bytes_per_sec,
            },
        );
        rx
    }

    pub fn build_transport(
        &mut self,
        peer_id: String,
        bandwidth_bytes_per_sec: u64,
    ) -> SimTransport {
        let event_rx = self.register_node(peer_id.clone(), bandwidth_bytes_per_sec);
        SimTransport::new(
            peer_id,
            self.links.clone(),
            event_rx,
            self.action_tx.clone(),
        )
    }

    pub fn set_link(&mut self, a: String, b: String, profile: LinkProfile) {
        let mut links = self.links.lock().expect("links lock poisoned");
        links.insert((a.clone(), b.clone()), profile.clone());
        links.insert((b, a), profile);
    }

    pub fn schedule_availability(&mut self, peer_id: String, at_ms: u64, online: bool) {
        self.scheduler
            .push(at_ms, ScheduledAction::Availability { peer_id, online });
    }

    pub fn run_for(&mut self, duration_ms: u64) {
        let target = self.clock.now_ms().saturating_add(duration_ms);
        while self.clock.now_ms() < target {
            self.step_until_next_event();
            if self.scheduler.next_timestamp().is_none() && self.action_rx.is_empty() {
                let next = self.clock.now_ms().saturating_add(1);
                self.clock.advance_to(next.min(target));
            }
        }
    }

    pub async fn run_for_async(&mut self, duration_ms: u64) {
        let target = self.clock.now_ms().saturating_add(duration_ms);
        while self.clock.now_ms() < target {
            self.step_until_next_event();
            tokio::task::yield_now().await;
            if self.scheduler.next_timestamp().is_none() && self.action_rx.is_empty() {
                let next = self.clock.now_ms().saturating_add(1);
                self.clock.advance_to(next.min(target));
            }
        }
    }

    pub fn step_until_next_event(&mut self) {
        while let Ok(action) = self.action_rx.try_recv() {
            self.schedule_action(action);
        }

        if let Some(next_at) = self.scheduler.next_timestamp() {
            self.clock.advance_to(next_at);
        }
        while let Some(event) = self.scheduler.pop_ready(self.clock.now_ms()) {
            match event.payload {
                ScheduledAction::Availability { peer_id, online } => {
                    if let Some(node) = self.nodes.get_mut(&peer_id) {
                        node.availability.online = online;
                    }
                }
                ScheduledAction::Deliver {
                    to_peer,
                    event,
                    bytes,
                } => {
                    if !self.consume_bandwidth(&to_peer, bytes as u64) {
                        self.metrics.dropped_bandwidth += 1;
                        continue;
                    }
                    self.metrics.sent_bytes += bytes as u64;
                    if let Some(tx) = self.event_txs.get(&to_peer) {
                        let _ = tx.send(event.clone());
                    }
                    if let TransportEvent::DirectReceived { message, .. } = event {
                        self.record_delivery(&to_peer, &message);
                    }
                }
            }
        }
    }

    pub fn metrics(&self) -> SimulationMetrics {
        self.metrics.clone()
    }

    fn schedule_action(&mut self, action: SimAction) {
        match action {
            SimAction::SendDirect {
                from_peer,
                to_peer,
                message,
            } => {
                if !self.is_online(&from_peer) || !self.is_online(&to_peer) {
                    self.metrics.dropped_offline += 1;
                    return;
                }
                let Some(profile) = self
                    .links
                    .lock()
                    .expect("links lock poisoned")
                    .get(&(from_peer.clone(), to_peer.clone()))
                    .cloned()
                else {
                    self.metrics.dropped_offline += 1;
                    return;
                };
                if self.roll_loss(profile.loss_per_mille) {
                    self.metrics.dropped_loss += 1;
                    return;
                }
                let at = self.clock.now_ms().saturating_add(profile.latency_ms);
                let bytes = wire_message_size(&message);
                self.scheduler.push(
                    at,
                    ScheduledAction::Deliver {
                        to_peer: to_peer.clone(),
                        event: TransportEvent::DirectReceived { from_peer, message },
                        bytes,
                    },
                );
            }
            SimAction::PublishScoped {
                from_peer,
                scope,
                payload,
            } => {
                for to_peer in self.nodes.keys().cloned().collect::<Vec<_>>() {
                    if to_peer == from_peer {
                        continue;
                    }
                    if !self.is_online(&from_peer) || !self.is_online(&to_peer) {
                        self.metrics.dropped_offline += 1;
                        continue;
                    }
                    let Some(profile) = self
                        .links
                        .lock()
                        .expect("links lock poisoned")
                        .get(&(from_peer.clone(), to_peer.clone()))
                        .cloned()
                    else {
                        continue;
                    };
                    if self.roll_loss(profile.loss_per_mille) {
                        self.metrics.dropped_loss += 1;
                        continue;
                    }
                    self.scheduler.push(
                        self.clock.now_ms().saturating_add(profile.latency_ms),
                        ScheduledAction::Deliver {
                            to_peer,
                            event: TransportEvent::ScopedReceived {
                                from_peer: from_peer.clone(),
                                scope: scope.clone(),
                                payload: payload.clone(),
                            },
                            bytes: payload.len() + 32,
                        },
                    );
                }
            }
        }
    }

    fn is_online(&self, peer_id: &str) -> bool {
        self.nodes
            .get(peer_id)
            .map(|n| n.availability.online)
            .unwrap_or(false)
    }

    fn consume_bandwidth(&mut self, peer_id: &str, bytes: u64) -> bool {
        let now_sec = self.clock.now_ms() / 1000;
        let key = (peer_id.to_string(), now_sec);
        let used = self.bandwidth_usage.entry(key).or_insert(0);
        let budget = self
            .nodes
            .get(peer_id)
            .map(|n| n.bandwidth_bytes_per_sec)
            .unwrap_or(0);
        if used.saturating_add(bytes) > budget {
            return false;
        }
        *used = used.saturating_add(bytes);
        true
    }

    fn roll_loss(&mut self, loss_per_mille: u16) -> bool {
        if loss_per_mille == 0 {
            return false;
        }
        self.rng_state = self
            .rng_state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1);
        (self.rng_state % 1000) < loss_per_mille as u64
    }

    fn record_delivery(&mut self, to_peer: &str, message: &WireMessage) {
        let Some((id, created_at_ms)) = delivered_key_fields(message, to_peer) else {
            return;
        };
        let key = format!("{to_peer}:{id}");
        if self
            .delivered_ids
            .insert(key, self.clock.now_ms())
            .is_some()
        {
            self.metrics.duplicates += 1;
            return;
        }
        self.metrics.delivered += 1;
        self.metrics
            .latencies_ms
            .push(self.clock.now_ms().saturating_sub(created_at_ms));
    }

    pub async fn spawn_sim_node(
        &mut self,
        peer_id: String,
        bandwidth: u64,
    ) -> anyhow::Result<SimNode> {
        let transport = self.build_transport(peer_id.clone(), bandwidth);
        let db_path = sim_db_path(self.sim_run_id, &peer_id);
        let _ = std::fs::remove_dir_all(&db_path);
        std::fs::create_dir_all(&db_path)?;
        let config = NodeConfig {
            listen_addr: "/ip4/0.0.0.0/tcp/0"
                .parse()
                .map_err(|e| anyhow::anyhow!("failed to parse listen addr: {e}"))?,
            db_path: db_path.to_string_lossy().to_string(),
            keypair_path: None,
            forwarding_interval_ms: 10,
            sync_interval_secs: 2,
            bootstrap_peers: Vec::new(),
            connectivity_rx: None,
            display_name: None,
            storage_key: None,
            max_relay_fanout: 3,
        };
        let (node_runner, handle) = NodeRunner::new_with_transport(config, Box::new(transport))?;
        let runner_task = tokio::spawn(async move { node_runner.run().await });
        Ok(SimNode {
            handle,
            runner_task,
        })
    }
}

fn sim_db_path(run_id: u64, peer_id: &str) -> PathBuf {
    std::env::temp_dir().join(format!("mycelium-sim-{run_id}-{peer_id}"))
}

fn wire_message_size(message: &WireMessage) -> usize {
    match message {
        WireMessage::Data(msg) => msg.body.len() + msg.envelope.payload.len() + 64,
        WireMessage::SyncIds { ids } | WireMessage::SyncRequest { ids } => {
            ids.iter().map(String::len).sum::<usize>() + 32
        }
        WireMessage::SyncBloom { bloom, .. } => bloom.len() + 32,
        WireMessage::SyncData { messages } => {
            messages.iter().map(|m| m.body.len()).sum::<usize>() + 64
        }
        WireMessage::ScopeAnnounce { scopes } => scopes.iter().map(String::len).sum::<usize>() + 32,
        WireMessage::EncryptedDirect {
            encrypted_payload, ..
        } => encrypted_payload.len() + 64 + 96,
        WireMessage::EncryptedGroup {
            encrypted_payload, ..
        } => encrypted_payload.len() + 32,
        WireMessage::PeerInfo { enc_pubkey_hex, .. } => enc_pubkey_hex.len() + 32,
    }
}

fn delivered_key_fields(message: &WireMessage, to_peer: &str) -> Option<(String, u64)> {
    match message {
        WireMessage::Data(msg) => {
            if msg
                .envelope
                .to_peer
                .as_deref()
                .is_some_and(|target| target == to_peer)
            {
                Some((msg.envelope.id.0.clone(), msg.envelope.created_at_ms))
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::SimAction;
    use mycelium_core::data::Envelope;
    use mycelium_core::transport::{DirectMessage, WireMessage};

    fn msg(from: &str, to: &str, body: &str) -> DirectMessage {
        DirectMessage {
            envelope: Envelope::new(
                from.to_string(),
                Some(to.to_string()),
                body.as_bytes().to_vec(),
            ),
            body: body.to_string(),
        }
    }

    #[test]
    fn simulation_is_deterministic_with_same_seed() {
        let mut a = SimulationRunner::new(0, 1337);
        let _ = a.register_node("n1".to_string(), 1024 * 64);
        let _ = a.register_node("n2".to_string(), 1024 * 64);
        a.set_link("n1".to_string(), "n2".to_string(), LinkProfile::default());
        let _ = a.action_tx.send(SimAction::SendDirect {
            from_peer: "n1".to_string(),
            to_peer: "n2".to_string(),
            message: WireMessage::Data(msg("n1", "n2", "hello")),
        });
        a.run_for(100);
        let ma = a.metrics();

        let mut b = SimulationRunner::new(0, 1337);
        let _ = b.register_node("n1".to_string(), 1024 * 64);
        let _ = b.register_node("n2".to_string(), 1024 * 64);
        b.set_link("n1".to_string(), "n2".to_string(), LinkProfile::default());
        let _ = b.action_tx.send(SimAction::SendDirect {
            from_peer: "n1".to_string(),
            to_peer: "n2".to_string(),
            message: WireMessage::Data(msg("n1", "n2", "hello")),
        });
        b.run_for(100);
        let mb = b.metrics();

        assert_eq!(ma.delivered, mb.delivered);
        assert_eq!(ma.dropped_loss, mb.dropped_loss);
        assert_eq!(ma.sent_bytes, mb.sent_bytes);
    }

    #[test]
    fn offline_nodes_drop_messages() {
        let mut sim = SimulationRunner::new(0, 1);
        let _ = sim.register_node("a".to_string(), 1024 * 64);
        let _ = sim.register_node("b".to_string(), 1024 * 64);
        sim.set_link("a".to_string(), "b".to_string(), LinkProfile::default());
        sim.nodes.get_mut("b").expect("node b").availability.online = false;
        let _ = sim.action_tx.send(SimAction::SendDirect {
            from_peer: "a".to_string(),
            to_peer: "b".to_string(),
            message: WireMessage::Data(msg("a", "b", "x")),
        });
        sim.run_for(50);
        assert!(sim.metrics().dropped_offline > 0);
    }

    #[tokio::test]
    async fn three_hop_delivery() {
        let mut sim = SimulationRunner::new(0, 42);
        let _ = sim.register_node("A".to_string(), 64 * 1024);
        let _ = sim.register_node("B".to_string(), 64 * 1024);
        let _ = sim.register_node("C".to_string(), 64 * 1024);
        let a = sim
            .spawn_sim_node("A".to_string(), 64 * 1024)
            .await
            .expect("spawn a");
        let b = sim
            .spawn_sim_node("B".to_string(), 64 * 1024)
            .await
            .expect("spawn b");
        let c = sim
            .spawn_sim_node("C".to_string(), 64 * 1024)
            .await
            .expect("spawn c");

        sim.set_link("A".to_string(), "B".to_string(), LinkProfile::default());
        sim.set_link("B".to_string(), "C".to_string(), LinkProfile::default());

        a.handle
            .send(mycelium_node::NodeCommand::SendDirect {
                to_peer: "C".to_string(),
                body: "hello from A".to_string(),
            })
            .await
            .expect("send");

        sim.run_for_async(5_000).await;
        let metrics = sim.metrics();
        assert_eq!(metrics.delivered, 1);
        assert_eq!(metrics.duplicates, 0);

        a.runner_task.abort();
        b.runner_task.abort();
        c.runner_task.abort();
    }
}
