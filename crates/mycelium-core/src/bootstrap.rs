//! Default public relay multiaddrs for wide-area connectivity.
//!
//! The embedded peer id in each multiaddr must match the relay deployed at
//! `https://mycelium-relay.fly.dev/` (`GET /` returns `peer_id`). Update this
//! module if the relay identity is rotated.

pub const BOOTSTRAP_PEERS: &[&str] = &[
    "/dns4/mycelium-relay.fly.dev/tcp/4001/p2p/12D3KooWGv6goWd2fHwcigDjqQm5Dfw28UijwEjnMcYwTpPRPZy6",
    "/dns4/mycelium-relay.fly.dev/udp/4001/quic-v1/p2p/12D3KooWGv6goWd2fHwcigDjqQm5Dfw28UijwEjnMcYwTpPRPZy6",
];

/// Owned multiaddr strings for [`mycelium_node::NodeConfig::bootstrap_peers`] and FFI.
#[must_use]
pub fn default_peer_multiaddrs() -> Vec<String> {
    BOOTSTRAP_PEERS.iter().map(|s| (*s).to_string()).collect()
}

#[cfg(test)]
mod tests {
    use libp2p::Multiaddr;

    #[test]
    fn bootstrap_multiaddrs_parse() {
        for s in super::BOOTSTRAP_PEERS {
            s.parse::<Multiaddr>()
                .unwrap_or_else(|e| panic!("invalid bootstrap multiaddr {s:?}: {e}"));
        }
    }
}
