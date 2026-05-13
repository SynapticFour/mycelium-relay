//! Öffentliche Bootstrap-Peers für das Mycelium-Netzwerk.
//!
//! Nach einem Relay-Redeployment:
//! 1. `curl -s https://mycelium-relay.fly.dev/ | grep peer_id`
//! 2. Peer-ID in [`BOOTSTRAP_PEERS`] eintragen
//! 3. Commit + Push → alle Clients bekommen beim nächsten Build die richtige Adresse

/// Eingebettete Bootstrap-Peer-Adressen.
/// Format: libp2p Multiaddr — /dns4/<host>/tcp/<port>/p2p/<peer-id>
pub const BOOTSTRAP_PEERS: &[&str] = &[
    "/dns4/mycelium-relay.fly.dev/tcp/4001/p2p/12D3KooWGv6goWd2fHwcigDjqQm5Dfw28UijwEjnMcYwTpPRPZy6",
    "/dns4/mycelium-relay.fly.dev/udp/4001/quic-v1/p2p/12D3KooWGv6goWd2fHwcigDjqQm5Dfw28UijwEjnMcYwTpPRPZy6",
];

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
