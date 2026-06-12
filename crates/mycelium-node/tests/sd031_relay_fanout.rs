// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! SD-031: unknown-destination relay fanout must be bounded.

use mycelium_node::security::select_relay_candidates;
use std::collections::HashMap;

#[test]
fn fanout_is_bounded() {
    const MAX_RELAY_FANOUT: usize = 3;
    let peers: Vec<String> = (0..10).map(|i| format!("12D3KooWPeer{i:02}")).collect();
    let target = "12D3KooWUnknown".to_string();
    let selected = select_relay_candidates(&peers, &target, &HashMap::new(), MAX_RELAY_FANOUT);
    assert_eq!(selected.len(), MAX_RELAY_FANOUT);
    assert!(!selected.contains(&target));
}

#[test]
fn lower_strike_peers_preferred() {
    let peers = vec![
        "peer_a".into(),
        "peer_b".into(),
        "peer_c".into(),
        "peer_d".into(),
    ];
    let mut strikes = HashMap::new();
    strikes.insert("peer_a".into(), 5);
    strikes.insert("peer_b".into(), 0);
    strikes.insert("peer_c".into(), 1);
    strikes.insert("peer_d".into(), 0);
    let selected = select_relay_candidates(&peers, "missing", &strikes, 2);
    assert_eq!(selected.len(), 2);
    assert!(selected.contains(&"peer_b".to_string()) || selected.contains(&"peer_d".to_string()));
}
