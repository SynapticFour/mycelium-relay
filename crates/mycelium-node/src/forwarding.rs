// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use mycelium_core::data::{now_ms, Priority};
use mycelium_core::energy::NodeState;
use mycelium_core::transport::{DirectMessage, MessageStore};
use std::sync::Arc;

pub trait SeenCache: Send {
    fn contains(&self, message_id: &str) -> bool;
    fn insert(&mut self, message_id: String);
}

#[derive(Debug, Clone)]
pub enum ForwardDecision {
    DeliverLocal,
    ForwardTo(Vec<String>),
    Hold,
    Drop(&'static str),
}

pub trait ForwardingPolicy: Send + Sync {
    fn decide(
        &self,
        message: &DirectMessage,
        local_peer: &str,
        known_peers: &[String],
        node_state: NodeState,
    ) -> ForwardDecision;
}

pub trait EnergyPolicy: Send + Sync {
    fn can_forward(&self, state: NodeState, priority: mycelium_core::data::Priority) -> bool;
}

pub struct SimpleEnergyPolicy;

impl EnergyPolicy for SimpleEnergyPolicy {
    fn can_forward(&self, state: NodeState, priority: Priority) -> bool {
        match state {
            NodeState::Active => true,
            NodeState::Intermittent => !matches!(priority, Priority::Low),
            NodeState::Passive => matches!(priority, Priority::High),
        }
    }
}

pub struct ProbabilisticForwardingPolicy;

impl ForwardingPolicy for ProbabilisticForwardingPolicy {
    fn decide(
        &self,
        message: &DirectMessage,
        local_peer: &str,
        known_peers: &[String],
        node_state: NodeState,
    ) -> ForwardDecision {
        if message
            .envelope
            .to_peer
            .as_deref()
            .is_some_and(|to| to == local_peer)
        {
            return ForwardDecision::DeliverLocal;
        }
        if message.is_expired(now_ms()) {
            return ForwardDecision::Drop("expired");
        }
        let fanout = base_fanout(node_state, message.envelope.priority);
        if fanout == 0 || known_peers.is_empty() {
            return ForwardDecision::Hold;
        }

        let mut selected = Vec::new();
        for peer in known_peers {
            if should_forward_to(peer, message, node_state) {
                selected.push(peer.clone());
                if selected.len() >= fanout {
                    break;
                }
            }
        }
        if selected.is_empty() {
            ForwardDecision::Hold
        } else {
            ForwardDecision::ForwardTo(selected)
        }
    }
}

fn base_fanout(state: NodeState, priority: Priority) -> usize {
    match state {
        NodeState::Active => match priority {
            Priority::High => 4,
            Priority::Normal => 3,
            Priority::Low => 2,
        },
        NodeState::Intermittent => match priority {
            Priority::High => 2,
            Priority::Normal => 1,
            Priority::Low => 0,
        },
        NodeState::Passive => match priority {
            Priority::High => 1,
            _ => 0,
        },
    }
}

fn should_forward_to(peer: &str, msg: &DirectMessage, state: NodeState) -> bool {
    let seed = format!("{}:{}:{:?}", msg.envelope.id.0, peer, state);
    let h = blake3::hash(seed.as_bytes());
    let r = (u16::from_le_bytes([h.as_bytes()[0], h.as_bytes()[1]]) % 1000) as u16;
    let threshold = match state {
        NodeState::Active => 850,
        NodeState::Intermittent => 550,
        NodeState::Passive => 250,
    };
    r < threshold
}

pub struct IngestPipeline {
    store: Arc<dyn MessageStore>,
}

impl IngestPipeline {
    pub fn new(store: Arc<dyn MessageStore>) -> Self {
        Self { store }
    }

    pub async fn ingest(
        &self,
        seen_cache: &mut dyn SeenCache,
        msg: &DirectMessage,
        verify: bool,
    ) -> anyhow::Result<bool> {
        if msg.is_expired(now_ms()) {
            return Ok(false);
        }
        if verify {
            let author = msg.envelope.signature_author_peer();
            let Ok(peer_id) = author.parse::<libp2p::PeerId>() else {
                return Ok(false);
            };
            if !msg.envelope.verify_or_transition(&peer_id) {
                return Ok(false);
            }
        }
        let message_id = msg.envelope.id.0.as_str();
        if seen_cache.contains(message_id) || self.store.contains(message_id).await? {
            return Ok(false);
        }
        seen_cache.insert(msg.envelope.id.0.clone());
        self.store.persist(msg).await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use mycelium_core::data::Envelope;
    use mycelium_core::transport::{MessageStore, StoreStats};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    struct TestSeenCache {
        seen: std::collections::HashSet<String>,
    }

    impl TestSeenCache {
        fn new() -> Self {
            Self {
                seen: std::collections::HashSet::new(),
            }
        }
    }

    impl SeenCache for TestSeenCache {
        fn contains(&self, message_id: &str) -> bool {
            self.seen.contains(message_id)
        }

        fn insert(&mut self, message_id: String) {
            let _ = self.seen.insert(message_id);
        }
    }

    #[derive(Default)]
    struct MemStore {
        inner: Mutex<HashMap<String, DirectMessage>>,
    }

    #[async_trait]
    impl MessageStore for MemStore {
        async fn persist(&self, message: &DirectMessage) -> anyhow::Result<()> {
            self.inner
                .lock()
                .expect("lock")
                .insert(message.envelope.id.0.clone(), message.clone());
            Ok(())
        }
        async fn recent(&self, _limit: usize) -> anyhow::Result<Vec<DirectMessage>> {
            Ok(self.inner.lock().expect("lock").values().cloned().collect())
        }
        async fn contains(&self, message_id: &str) -> anyhow::Result<bool> {
            Ok(self.inner.lock().expect("lock").contains_key(message_id))
        }
        async fn load_by_id(&self, message_id: &str) -> anyhow::Result<Option<DirectMessage>> {
            Ok(self.inner.lock().expect("lock").get(message_id).cloned())
        }
        async fn list_ids_window(&self, _window: Duration) -> anyhow::Result<Vec<String>> {
            Ok(self.inner.lock().expect("lock").keys().cloned().collect())
        }
        async fn gc_expired(&self) -> anyhow::Result<usize> {
            Ok(0)
        }
        async fn stats(&self) -> anyhow::Result<StoreStats> {
            Ok(StoreStats {
                count: self.inner.lock().expect("lock").len(),
                oldest_ms: 0,
            })
        }
    }

    fn mk_msg(body: &str) -> DirectMessage {
        DirectMessage {
            envelope: Envelope::new(
                "a".to_string(),
                Some("b".to_string()),
                body.as_bytes().to_vec(),
            ),
            body: body.to_string(),
        }
    }

    #[test]
    fn energy_policy_applies_state_rules() {
        let p = SimpleEnergyPolicy;
        assert!(p.can_forward(NodeState::Active, Priority::Low));
        assert!(!p.can_forward(NodeState::Intermittent, Priority::Low));
        assert!(p.can_forward(NodeState::Passive, Priority::High));
        assert!(!p.can_forward(NodeState::Passive, Priority::Normal));
    }

    #[test]
    fn forwarding_policy_delivers_local_target() {
        let policy = ProbabilisticForwardingPolicy;
        let mut msg = mk_msg("hello");
        msg.envelope.to_peer = Some("local".to_string());
        let d = policy.decide(&msg, "local", &["p1".to_string()], NodeState::Active);
        assert!(matches!(d, ForwardDecision::DeliverLocal));
    }

    #[tokio::test]
    async fn ingest_pipeline_deduplicates_seen_messages() {
        let store = Arc::new(MemStore::default());
        let pipeline = IngestPipeline::new(store);
        let mut seen = TestSeenCache::new();
        let msg = mk_msg("x");
        assert!(pipeline
            .ingest(&mut seen, &msg, false)
            .await
            .expect("ingest1"));
        assert!(!pipeline
            .ingest(&mut seen, &msg, false)
            .await
            .expect("ingest2"));
    }
}
