//! Per-address outbound velocity windows (sled-backed).

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

const MAX_STORED_EVENTS: usize = 64;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct VelocityWindow {
    events: VecDeque<(u64, u64)>,
}

impl VelocityWindow {
    fn push(&mut self, timestamp_ms: u64, amount_muon: u64) {
        self.events.push_back((timestamp_ms, amount_muon));
        while self.events.len() > MAX_STORED_EVENTS {
            self.events.pop_front();
        }
    }

    fn recent(&self) -> Vec<(u64, u64)> {
        self.events.iter().copied().collect()
    }
}

#[derive(Debug, Clone)]
pub struct VelocityTracker {
    tree: sled::Tree,
}

impl VelocityTracker {
    pub fn open(db: &sled::Db) -> anyhow::Result<Self> {
        Ok(Self {
            tree: db.open_tree("settlement_velocity")?,
        })
    }

    pub fn recent_for(&self, address: &str) -> anyhow::Result<Vec<(u64, u64)>> {
        match self.tree.get(address.as_bytes())? {
            Some(bytes) => {
                let w: VelocityWindow = bincode::deserialize(&bytes)?;
                Ok(w.recent())
            }
            None => Ok(Vec::new()),
        }
    }

    pub fn record_outbound(
        &self,
        address: &str,
        timestamp_ms: u64,
        amount_muon: u64,
    ) -> anyhow::Result<()> {
        let mut w = match self.tree.get(address.as_bytes())? {
            Some(bytes) => bincode::deserialize::<VelocityWindow>(&bytes)?,
            None => VelocityWindow::default(),
        };
        w.push(timestamp_ms, amount_muon);
        self.tree
            .insert(address.as_bytes(), bincode::serialize(&w)?)?;
        Ok(())
    }
}
