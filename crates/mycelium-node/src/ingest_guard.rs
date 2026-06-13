// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! SD-032 (per-author rate limits) and SD-060 (replay window) at ingest.

use mycelium_core::transport::DirectMessage;
use std::collections::{HashMap, HashSet, VecDeque};

/// Max gossip/bulletin payload bytes per scoped publish (SD-032).
pub const MAX_GOSSIP_PAYLOAD_BYTES: usize = 65_536;

/// Reject envelopes older than this at ingest (SD-060).
pub const REPLAY_MAX_AGE_MS: u64 = 30 * 60 * 1000;

/// Allow modest clock skew into the future (SD-060).
pub const REPLAY_MAX_FUTURE_SKEW_MS: u64 = 5 * 60 * 1000;

/// Direct `WireMessage::Data` per author per minute (SD-032).
pub const DIRECT_RATE_PER_MINUTE: u32 = 120;

/// Scoped gossip publishes per relay peer per minute (SD-032).
pub const GOSSIP_RATE_PER_MINUTE: u32 = 60;

const RATE_WINDOW_MS: u64 = 60 * 1000;

/// Per-author id window retained after global seen-cache eviction (SD-060).
const AUTHOR_ID_WINDOW: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestReject {
    Stale,
    Replay,
    RateLimit,
    Oversize,
}

pub fn check_envelope_freshness(created_at_ms: u64, now_ms: u64) -> bool {
    if created_at_ms > now_ms.saturating_add(REPLAY_MAX_FUTURE_SKEW_MS) {
        return false;
    }
    now_ms.saturating_sub(created_at_ms) <= REPLAY_MAX_AGE_MS
}

struct FixedWindowRateLimiter {
    limit: u32,
    window_ms: u64,
    buckets: HashMap<String, (u32, u64)>,
}

impl FixedWindowRateLimiter {
    fn new(limit: u32, window_ms: u64) -> Self {
        Self {
            limit,
            window_ms,
            buckets: HashMap::new(),
        }
    }

    fn allow(&mut self, key: &str, now_ms: u64) -> bool {
        let entry = self.buckets.entry(key.to_string()).or_insert((0, now_ms));
        if now_ms.saturating_sub(entry.1) >= self.window_ms {
            *entry = (0, now_ms);
        }
        if entry.0 >= self.limit {
            return false;
        }
        entry.0 += 1;
        true
    }
}

struct AuthorReplayWindow {
    max_per_author: usize,
    by_author: HashMap<String, (HashSet<String>, VecDeque<String>)>,
}

impl AuthorReplayWindow {
    fn new(max_per_author: usize) -> Self {
        Self {
            max_per_author,
            by_author: HashMap::new(),
        }
    }

    /// Returns `true` when this `(author, message_id)` was already seen recently.
    fn is_duplicate(&mut self, author: &str, message_id: &str) -> bool {
        let entry = self
            .by_author
            .entry(author.to_string())
            .or_insert_with(|| (HashSet::new(), VecDeque::new()));
        if entry.0.contains(message_id) {
            return true;
        }
        entry.0.insert(message_id.to_string());
        entry.1.push_back(message_id.to_string());
        while entry.1.len() > self.max_per_author {
            if let Some(old) = entry.1.pop_front() {
                entry.0.remove(&old);
            }
        }
        false
    }
}

pub struct IngestGuard {
    rate_direct: FixedWindowRateLimiter,
    rate_gossip: FixedWindowRateLimiter,
    replay: AuthorReplayWindow,
}

impl IngestGuard {
    pub fn new() -> Self {
        Self {
            rate_direct: FixedWindowRateLimiter::new(DIRECT_RATE_PER_MINUTE, RATE_WINDOW_MS),
            rate_gossip: FixedWindowRateLimiter::new(GOSSIP_RATE_PER_MINUTE, RATE_WINDOW_MS),
            replay: AuthorReplayWindow::new(AUTHOR_ID_WINDOW),
        }
    }

    pub fn allow_direct(
        &mut self,
        author: &str,
        msg: &DirectMessage,
        now_ms: u64,
    ) -> Result<(), IngestReject> {
        if !check_envelope_freshness(msg.envelope.created_at_ms, now_ms) {
            return Err(IngestReject::Stale);
        }
        if self.replay.is_duplicate(author, &msg.envelope.id.0) {
            return Err(IngestReject::Replay);
        }
        if !self.rate_direct.allow(author, now_ms) {
            return Err(IngestReject::RateLimit);
        }
        Ok(())
    }

    /// Size + per-peer rate before constructing gossip envelopes (SD-032).
    pub fn check_gossip_payload(
        &mut self,
        from_peer: &str,
        payload_len: usize,
        now_ms: u64,
    ) -> Result<(), IngestReject> {
        if payload_len > MAX_GOSSIP_PAYLOAD_BYTES {
            return Err(IngestReject::Oversize);
        }
        if !self.rate_gossip.allow(from_peer, now_ms) {
            return Err(IngestReject::RateLimit);
        }
        Ok(())
    }

    /// Freshness + per-author id window for scoped gossip (SD-060).
    pub fn check_gossip_message(
        &mut self,
        from_peer: &str,
        message_id: &str,
        created_at_ms: u64,
        now_ms: u64,
    ) -> Result<(), IngestReject> {
        if !check_envelope_freshness(created_at_ms, now_ms) {
            return Err(IngestReject::Stale);
        }
        if self.replay.is_duplicate(from_peer, message_id) {
            return Err(IngestReject::Replay);
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn allow_gossip(
        &mut self,
        from_peer: &str,
        payload_len: usize,
        message_id: &str,
        created_at_ms: u64,
        now_ms: u64,
    ) -> Result<(), IngestReject> {
        self.check_gossip_payload(from_peer, payload_len, now_ms)?;
        self.check_gossip_message(from_peer, message_id, created_at_ms, now_ms)
    }
}

impl Default for IngestGuard {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mycelium_core::data::Envelope;

    fn mk_msg(from: &str, created_at_ms: u64, salt: u8) -> DirectMessage {
        let mut envelope = Envelope::new(from.to_string(), None, vec![salt]);
        envelope.created_at_ms = created_at_ms;
        DirectMessage {
            envelope,
            body: "hello".into(),
        }
    }

    #[test]
    fn freshness_rejects_stale_and_far_future() {
        let now = 10_000_000u64;
        assert!(!check_envelope_freshness(
            now.saturating_sub(REPLAY_MAX_AGE_MS + 1),
            now
        ));
        assert!(!check_envelope_freshness(
            now.saturating_add(REPLAY_MAX_FUTURE_SKEW_MS + 1),
            now
        ));
        assert!(check_envelope_freshness(now.saturating_sub(1000), now));
    }

    #[test]
    fn direct_rate_limit_trips() {
        let mut guard = IngestGuard::new();
        let now = 5_000_000u64;
        for i in 0..DIRECT_RATE_PER_MINUTE {
            let msg = mk_msg("author", now, i as u8);
            assert!(guard.allow_direct("author", &msg, now).is_ok(), "i={i}");
        }
        let msg = mk_msg("author", now, 255);
        assert_eq!(
            guard.allow_direct("author", &msg, now),
            Err(IngestReject::RateLimit)
        );
    }

    #[test]
    fn gossip_oversize_rejected() {
        let mut guard = IngestGuard::new();
        let now = 5_000_000u64;
        assert_eq!(
            guard.allow_gossip("peer", MAX_GOSSIP_PAYLOAD_BYTES + 1, "id1", now, now),
            Err(IngestReject::Oversize)
        );
    }

    #[test]
    fn author_replay_window_catches_duplicate_id() {
        let mut guard = IngestGuard::new();
        let now = 5_000_000u64;
        let msg = mk_msg("author", now, 0);
        assert!(guard.allow_direct("author", &msg, now).is_ok());
        assert_eq!(
            guard.allow_direct("author", &msg, now),
            Err(IngestReject::Replay)
        );
    }
}
