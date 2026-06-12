// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! Network-layer security helpers (SD-030, SD-031).

use libp2p::PeerId;
use mycelium_core::transport::DirectMessage;
use std::collections::HashMap;

/// Unsigned `WireMessage::Data` are rejected when `now_ms >=` this value.
/// Set to `0` — no grace period (public beta gate SD-030).
pub const UNSIGNED_GRACE_PERIOD_UNTIL_MS: u64 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sd030DropReason {
    NoSignature,
    InvalidSignature,
    UnparseableAuthor,
}

/// Gossip relay bodies are unsigned by design: inner payload carries its own auth
/// (E2E group, signed store listing, enc1 bulletin). Only these exact tags bypass SD-030.
pub fn is_gossip_relay_body(body: &str) -> bool {
    body == "[mycelium:group]" || body == "[appstore]"
}

/// Returns `Ok(())` when the message may be processed; `Err` when it must be dropped.
pub fn validate_data_message_signature(
    message: &DirectMessage,
    now_ms: u64,
) -> Result<(), Sd030DropReason> {
    if is_gossip_relay_body(&message.body) {
        return Ok(());
    }
    match &message.envelope.signature {
        None => {
            if now_ms >= UNSIGNED_GRACE_PERIOD_UNTIL_MS {
                Err(Sd030DropReason::NoSignature)
            } else {
                Ok(())
            }
        }
        Some(_) => {
            let author = message.envelope.signature_author_peer();
            let Ok(author_id) = author.parse::<PeerId>() else {
                return Err(Sd030DropReason::UnparseableAuthor);
            };
            if message.envelope.verify(&author_id) {
                Ok(())
            } else {
                Err(Sd030DropReason::InvalidSignature)
            }
        }
    }
}

/// SD-031: pick up to `max_fanout` relay peers, preferring lower strike counts.
pub fn select_relay_candidates(
    peers: &[String],
    to_peer: &str,
    peer_strikes: &HashMap<String, u8>,
    max_fanout: usize,
) -> Vec<String> {
    let mut scored: Vec<(String, u8)> = peers
        .iter()
        .filter(|p| *p != to_peer)
        .map(|p| {
            let strikes = peer_strikes.get(p).copied().unwrap_or(0);
            (p.clone(), strikes)
        })
        .collect();
    scored.sort_by_key(|(_, strikes)| *strikes);
    scored
        .into_iter()
        .take(max_fanout)
        .map(|(peer, _)| peer)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity;
    use mycelium_core::data::Envelope;

    fn sample_message(signed: bool, body: &str) -> DirectMessage {
        let kp = identity::Keypair::generate_ed25519();
        let author = kp.public().to_peer_id().to_string();
        let mut envelope = Envelope::new(author, Some("dest".into()), b"payload".to_vec());
        if signed {
            envelope.sign(&kp).unwrap();
        } else {
            envelope.signature = None;
        }
        DirectMessage {
            envelope,
            body: body.into(),
        }
    }

    #[test]
    fn unsigned_dropped_after_grace_period() {
        let msg = sample_message(false, "hello");
        let after_grace = UNSIGNED_GRACE_PERIOD_UNTIL_MS + 1;
        assert_eq!(
            validate_data_message_signature(&msg, after_grace),
            Err(Sd030DropReason::NoSignature)
        );
    }

    #[test]
    fn unsigned_rejected_immediately() {
        let msg = sample_message(false, "hello");
        assert_eq!(
            validate_data_message_signature(&msg, 0),
            Err(Sd030DropReason::NoSignature)
        );
    }

    #[test]
    fn gossip_relay_body_skips_sig_check() {
        let msg = sample_message(false, "[mycelium:group]");
        let after_grace = UNSIGNED_GRACE_PERIOD_UNTIL_MS + 1;
        assert!(validate_data_message_signature(&msg, after_grace).is_ok());
    }

    #[test]
    fn invalid_sig_is_dropped() {
        let kp_a = identity::Keypair::generate_ed25519();
        let kp_b = identity::Keypair::generate_ed25519();
        let author_b = kp_b.public().to_peer_id().to_string();
        let mut envelope = Envelope::new(author_b, Some("dest".into()), b"payload".to_vec());
        envelope.sign(&kp_a).unwrap();
        let msg = DirectMessage {
            envelope,
            body: "hello".into(),
        };
        assert_eq!(
            validate_data_message_signature(&msg, 0),
            Err(Sd030DropReason::InvalidSignature)
        );
    }

    #[test]
    fn fanout_is_bounded() {
        let peers: Vec<String> = (0..10).map(|i| format!("peer{i}")).collect();
        let relay = select_relay_candidates(&peers, "unknown", &HashMap::new(), 3);
        assert_eq!(relay.len(), 3);
        assert!(!relay.iter().any(|p| p == "unknown"));
    }
}
