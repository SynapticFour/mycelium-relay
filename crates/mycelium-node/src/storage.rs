// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use anyhow::Context;
use mycelium_core::data::now_ms;
use mycelium_core::transport::{DirectMessage, MessageStore, StoreStats};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

pub struct SledMessageStore {
    db: sled::Db,
    order: Arc<Mutex<Vec<(u64, String)>>>,
}

impl SledMessageStore {
    pub fn open(path: &str) -> anyhow::Result<Self> {
        let db = sled::Config::new()
            .path(path)
            .flush_every_ms(Some(5000))
            .open()
            .with_context(|| format!("opening sled db at {path}"))?;
        Ok(Self {
            db,
            order: Arc::new(Mutex::new(Vec::new())),
        })
    }

    pub async fn gc_expired(&self) -> anyhow::Result<usize> {
        let now = now_ms();
        let mut deleted = 0usize;
        let mut order = self.order.lock().await;
        let mut to_delete = Vec::new();
        order.retain(|(ts, id)| {
            if now.saturating_sub(*ts) > 7 * 24 * 60 * 60 * 1000 {
                to_delete.push(id.clone());
                false
            } else {
                true
            }
        });
        for id in to_delete {
            self.db.remove(id.as_bytes())?;
            deleted += 1;
        }
        Ok(deleted)
    }

    pub async fn stats(&self) -> StoreStats {
        let order = self.order.lock().await;
        StoreStats {
            count: order.len(),
            oldest_ms: order.first().map(|(ts, _)| *ts).unwrap_or(0),
        }
    }
}

#[async_trait::async_trait]
impl MessageStore for SledMessageStore {
    async fn persist(&self, message: &DirectMessage) -> anyhow::Result<()> {
        let key = message.envelope.id.0.clone();
        let encoded = bincode::serialize(message)?;
        let ts = now_ms();
        self.db.insert(key.as_bytes(), encoded)?;
        self.order.lock().await.push((ts, key));
        Ok(())
    }

    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<DirectMessage>> {
        let keys = self.order.lock().await.clone();
        let mut out = Vec::new();
        for (_ts, key) in keys.into_iter().rev().take(limit) {
            if let Some(value) = self.db.get(key.as_bytes())? {
                let msg: DirectMessage = bincode::deserialize(&value)?;
                out.push(msg);
            }
        }
        Ok(out)
    }

    async fn contains(&self, message_id: &str) -> anyhow::Result<bool> {
        Ok(self.db.contains_key(message_id.as_bytes())?)
    }

    async fn load_by_id(&self, message_id: &str) -> anyhow::Result<Option<DirectMessage>> {
        let value = self.db.get(message_id.as_bytes())?;
        if let Some(bytes) = value {
            let msg: DirectMessage = bincode::deserialize(&bytes)?;
            Ok(Some(msg))
        } else {
            Ok(None)
        }
    }

    async fn list_ids_window(&self, window: Duration) -> anyhow::Result<Vec<String>> {
        let cutoff = now_ms().saturating_sub(window.as_millis() as u64);
        let keys = self.order.lock().await;
        Ok(keys
            .iter()
            .filter(|(ts, _)| *ts >= cutoff)
            .map(|(_, id)| id.clone())
            .collect())
    }

    async fn gc_expired(&self) -> anyhow::Result<usize> {
        SledMessageStore::gc_expired(self).await
    }

    async fn stats(&self) -> anyhow::Result<StoreStats> {
        Ok(SledMessageStore::stats(self).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mycelium_core::data::Envelope;

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

    #[tokio::test]
    async fn store_persist_and_load_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SledMessageStore::open(dir.path().to_str().expect("path")).expect("open store");
        let msg = mk_msg("roundtrip");
        let id = msg.envelope.id.0.clone();
        store.persist(&msg).await.expect("persist");
        assert!(store.contains(&id).await.expect("contains"));
        let loaded = store.load_by_id(&id).await.expect("load").expect("some");
        assert_eq!(loaded.body, "roundtrip");
    }

    #[tokio::test]
    async fn store_recent_returns_newest_first() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SledMessageStore::open(dir.path().to_str().expect("path")).expect("open store");
        store.persist(&mk_msg("one")).await.expect("persist1");
        store.persist(&mk_msg("two")).await.expect("persist2");
        let recent = store.recent(2).await.expect("recent");
        assert_eq!(recent.first().expect("first").body, "two");
    }

    #[tokio::test]
    async fn list_ids_window_filters_old_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SledMessageStore::open(dir.path().to_str().expect("path")).expect("open store");
        let old = mk_msg("old");
        let old_id = old.envelope.id.0.clone();
        store.persist(&old).await.expect("persist old");
        {
            let mut order = store.order.lock().await;
            if let Some((ts, _)) = order.last_mut() {
                *ts = now_ms().saturating_sub(10_000);
            }
        }
        let new = mk_msg("new");
        let new_id = new.envelope.id.0.clone();
        store.persist(&new).await.expect("persist new");
        let ids = store
            .list_ids_window(Duration::from_secs(1))
            .await
            .expect("window");
        assert!(ids.contains(&new_id));
        assert!(!ids.contains(&old_id));
    }
}
