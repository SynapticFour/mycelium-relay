// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! Bloom sync: peers exchange IDs missing from the remote bloom filter.

use mycelium_core::sync::BloomFilter;
fn ids_to_send_to_remote(own_ids: &[String], remote_bloom: &BloomFilter) -> Vec<String> {
    own_ids
        .iter()
        .filter(|id| !remote_bloom.contains(id))
        .cloned()
        .collect()
}

fn make_bloom(ids: &[&str]) -> BloomFilter {
    let mut bloom = BloomFilter::new();
    for id in ids {
        bloom.insert(id);
    }
    bloom
}

#[test]
fn bloom_diff_finds_ids_peer_lacks() {
    let remote_bloom = make_bloom(&["2", "3", "4"]);
    let own_ids: Vec<String> = vec!["1", "2", "3"].into_iter().map(String::from).collect();

    let send = ids_to_send_to_remote(&own_ids, &remote_bloom);
    assert_eq!(send, vec!["1".to_string()]);
}

#[test]
fn bloom_roundtrip_bytes_stable() {
    let bloom = make_bloom(&["a", "b", "c"]);
    let bytes = bloom.to_bytes();
    let restored = BloomFilter::from_bytes(&bytes).expect("valid bloom bytes");
    assert!(restored.contains("a"));
    assert!(!restored.contains("z"));
}

/// Simulates node A ([1,2,3]) vs node B ([2,3,4]) bloom exchange (K3).
#[test]
fn sync_bloom_exchanges_missing_messages() {
    let ids_a: Vec<String> = vec!["1", "2", "3"].into_iter().map(String::from).collect();
    let ids_b: Vec<String> = vec!["2", "3", "4"].into_iter().map(String::from).collect();

    let bloom_b = make_bloom(&["2", "3", "4"]);
    let a_to_b = ids_to_send_to_remote(&ids_a, &bloom_b);
    assert_eq!(a_to_b, vec!["1".to_string()]);

    let bloom_a = make_bloom(&["1", "2", "3"]);
    let b_to_a = ids_to_send_to_remote(&ids_b, &bloom_a);
    assert_eq!(b_to_a, vec!["4".to_string()]);
}
