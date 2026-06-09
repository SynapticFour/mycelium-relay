//! HTTP status probe for the public mycelium-relay (used by desktop UI).

use serde::Serialize;

const DEFAULT_RELAY_HOST: &str = "mycelium-relay.fly.dev";

#[derive(Debug, Clone, Serialize)]
pub struct RelayStatus {
    pub online: bool,
    pub status: String,
    pub connections: u64,
    pub peer_id: Option<String>,
}

/// Fetch relay status JSON from `https://<host>/` (blocking).
pub fn fetch_relay_status(host: Option<&str>) -> RelayStatus {
    let host = host.unwrap_or(DEFAULT_RELAY_HOST);
    let url = format!("https://{host}/");
    let resp = match ureq::get(&url)
        .timeout(std::time::Duration::from_secs(8))
        .call()
    {
        Ok(r) => r,
        Err(_) => {
            return RelayStatus {
                online: false,
                status: "offline".into(),
                connections: 0,
                peer_id: None,
            };
        }
    };
    let body: serde_json::Value = match resp.into_json() {
        Ok(v) => v,
        Err(_) => {
            return RelayStatus {
                online: false,
                status: "offline".into(),
                connections: 0,
                peer_id: None,
            };
        }
    };
    let status = body
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let online = status == "ok";
    let connections = body
        .get("connections")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let peer_id = body
        .get("peer_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    RelayStatus {
        online,
        status,
        connections,
        peer_id,
    }
}
