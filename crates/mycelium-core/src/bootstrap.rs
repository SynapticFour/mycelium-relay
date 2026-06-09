//! Öffentliche Bootstrap-Peers für das Mycelium-Netzwerk.
//!
//! Nach einem Relay-Redeployment:
//! 1. `curl -s https://mycelium-relay.fly.dev/ | grep peer_id`
//! 2. Peer-ID in [`BOOTSTRAP_PEERS`] eintragen
//! 3. Commit + Push → alle Clients bekommen beim nächsten Build die richtige Adresse

/// Eingebettete Bootstrap-Peer-Adressen.
/// Format: libp2p Multiaddr — /dns4/<host>/tcp/<port>/p2p/<peer-id>
pub const RELAY_PEER_ID: &str = "12D3KooWGv6goWd2fHwcigDjqQm5Dfw28UijwEjnMcYwTpPRPZy6";

pub const BOOTSTRAP_PEERS: &[&str] = &[
    "/dns4/mycelium-relay.fly.dev/tcp/4001/p2p/12D3KooWGv6goWd2fHwcigDjqQm5Dfw28UijwEjnMcYwTpPRPZy6",
    "/dns4/mycelium-relay.fly.dev/udp/4001/quic-v1/p2p/12D3KooWGv6goWd2fHwcigDjqQm5Dfw28UijwEjnMcYwTpPRPZy6",
];

/// libp2p circuit address to reach `remote_peer_id` via the public relay (NAT traversal).
pub fn relay_circuit_multiaddr(remote_peer_id: &str) -> Option<String> {
    let remote = remote_peer_id.trim();
    if remote.is_empty() {
        return None;
    }
    let base = BOOTSTRAP_PEERS
        .iter()
        .find(|addr| addr.contains("/tcp/"))?;
    Some(format!("{base}/p2p-circuit/p2p/{remote}"))
}

/// Shareable dial info: compact invite first (QR), then relay circuit, then listen addrs.
pub fn shareable_dial_multiaddrs(local_peer_id: &str, listen_addrs: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    out.push(crate::invite::encode_invite(local_peer_id));
    if let Some(circuit) = relay_circuit_multiaddr(local_peer_id) {
        out.push(circuit);
    }
    for addr in listen_addrs {
        let trimmed = addr.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.contains("/p2p/") {
            out.push(trimmed.to_string());
        } else {
            out.push(format!("{trimmed}/p2p/{local_peer_id}"));
        }
    }
    out
}

/// Lädt Bootstrap-Peers mit folgender Priorität:
/// 1. Umgebungsvariable `MYCELIUM_BOOTSTRAP_PEERS` (kommasepariert)
/// 2. Datei `<db_path>/bootstrap.txt` (eine Adresse pro Zeile, # = Kommentar)
/// 3. Eingebettete Konstanten aus [`BOOTSTRAP_PEERS`]
pub fn load_bootstrap_peers(db_path: &str) -> Vec<String> {
    if let Ok(env_peers) = std::env::var("MYCELIUM_BOOTSTRAP_PEERS") {
        let peers: Vec<String> = env_peers
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        if !peers.is_empty() {
            tracing::info!(
                "bootstrap: using {} peer(s) from MYCELIUM_BOOTSTRAP_PEERS",
                peers.len()
            );
            return peers;
        }
    }

    let path = std::path::Path::new(db_path).join("bootstrap.txt");
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            let peers: Vec<String> = content
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(String::from)
                .collect();
            if !peers.is_empty() {
                tracing::info!(
                    "bootstrap: using {} peer(s) from {}",
                    peers.len(),
                    path.display()
                );
                return peers;
            }
        }
    }

    tracing::info!(
        "bootstrap: using {} built-in peer(s)",
        BOOTSTRAP_PEERS.len()
    );
    BOOTSTRAP_PEERS.iter().map(|s| (*s).to_string()).collect()
}

/// Reads custom bootstrap peers from `<db_path>/bootstrap.txt` only (no env / built-in fallback).
pub fn load_custom_bootstrap_peers(db_path: &str) -> Vec<String> {
    let path = std::path::Path::new(db_path).join("bootstrap.txt");
    if !path.exists() {
        return Vec::new();
    }
    std::fs::read_to_string(&path)
        .map(|content| {
            content
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Persists custom bootstrap peers to `<db_path>/bootstrap.txt`.
pub fn save_custom_bootstrap_peers(db_path: &str, peers: &[String]) -> std::io::Result<()> {
    std::fs::create_dir_all(db_path)?;
    let path = std::path::Path::new(db_path).join("bootstrap.txt");
    if peers.is_empty() {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        return Ok(());
    }
    std::fs::write(path, peers.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_circuit_addr_uses_embedded_relay() {
        let addr = relay_circuit_multiaddr("12D3KooWExamplePeerIdForTestOnly")
            .expect("circuit addr");
        assert!(addr.contains("mycelium-relay.fly.dev"));
        assert!(addr.contains("/p2p-circuit/p2p/12D3KooWExamplePeerIdForTestOnly"));
        assert!(addr.contains(RELAY_PEER_ID));
    }
}
