// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use super::presence::PresenceSignal;
use std::collections::HashMap;
use uuid::Uuid;

pub struct ProximityStore {
    signals: HashMap<Uuid, PresenceSignal>,
    max_size: usize,
}

impl ProximityStore {
    pub fn new() -> Self {
        Self {
            signals: HashMap::new(),
            max_size: 500,
        }
    }

    pub fn insert(&mut self, signal: PresenceSignal) {
        if self.signals.len() >= self.max_size {
            if let Some(oldest_id) = self
                .signals
                .values()
                .min_by_key(|s| s.created_at_ms)
                .map(|s| s.ephemeral_id)
            {
                self.signals.remove(&oldest_id);
            }
        }
        self.signals.insert(signal.ephemeral_id, signal);
    }

    pub fn active_signals(&self) -> Vec<&PresenceSignal> {
        let mut active: Vec<_> = self.signals.values().filter(|s| !s.is_expired()).collect();
        active.sort_by_key(|s| std::cmp::Reverse(s.created_at_ms));
        active
    }

    pub fn prune(&mut self) -> usize {
        let before = self.signals.len();
        self.signals.retain(|_, s| !s.is_expired());
        before - self.signals.len()
    }

    pub fn count(&self) -> usize {
        self.signals.values().filter(|s| !s.is_expired()).count()
    }

    pub fn clear(&mut self) {
        self.signals.clear();
    }
}

impl Default for ProximityStore {
    fn default() -> Self {
        Self::new()
    }
}
