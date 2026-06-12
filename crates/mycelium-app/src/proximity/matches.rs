// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use std::collections::{HashSet, VecDeque};

#[derive(Default)]
pub struct ProximityMatchState {
    outgoing: HashSet<String>,
    incoming: HashSet<String>,
}

impl ProximityMatchState {
    pub fn record_outgoing(&mut self, enc_pubkey_hex: &str) {
        self.outgoing.insert(enc_pubkey_hex.to_string());
    }

    pub fn record_incoming(&mut self, enc_pubkey_hex: &str) {
        self.incoming.insert(enc_pubkey_hex.to_string());
    }

    pub fn interest_sent(&self, enc_pubkey_hex: &str) -> bool {
        self.outgoing.contains(enc_pubkey_hex)
    }

    pub fn interest_received(&self, enc_pubkey_hex: &str) -> bool {
        self.incoming.contains(enc_pubkey_hex)
    }

    pub fn is_mutual(&self, enc_pubkey_hex: &str) -> bool {
        self.interest_sent(enc_pubkey_hex) && self.interest_received(enc_pubkey_hex)
    }

    pub fn clear(&mut self) {
        self.outgoing.clear();
        self.incoming.clear();
    }
}

#[derive(Debug, Clone)]
pub struct ProximityReceivedMessage {
    pub from_enc_pubkey_hex: String,
    pub body: String,
    pub received_at_ms: u64,
}

pub struct ProximityInbox {
    messages: VecDeque<ProximityReceivedMessage>,
    max_size: usize,
}

impl ProximityInbox {
    pub fn new() -> Self {
        Self {
            messages: VecDeque::new(),
            max_size: 200,
        }
    }

    pub fn push(&mut self, msg: ProximityReceivedMessage) {
        if self.messages.len() >= self.max_size {
            self.messages.pop_front();
        }
        self.messages.push_back(msg);
    }

    pub fn since(&self, since_ms: u64) -> Vec<ProximityReceivedMessage> {
        self.messages
            .iter()
            .filter(|m| m.received_at_ms > since_ms)
            .cloned()
            .collect()
    }

    pub fn all(&self) -> Vec<ProximityReceivedMessage> {
        self.messages.iter().cloned().collect()
    }

    pub fn clear(&mut self) {
        self.messages.clear();
    }
}

impl Default for ProximityInbox {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct ProximityNearbyEntry {
    pub signal: super::presence::PresenceSignal,
    pub interest_sent: bool,
    pub interest_received: bool,
    pub is_mutual: bool,
}
