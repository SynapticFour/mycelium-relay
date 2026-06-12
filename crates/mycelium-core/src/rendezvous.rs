// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! Fetch peer lists from the public relay rendezvous HTTP API.

use serde::Deserialize;

const DEFAULT_RELAY_HOST: &str = "mycelium-relay.fly.dev";

#[derive(Debug, Deserialize)]
struct RendezvousResponse {
    peers: Vec<String>,
}

/// Returns peer IDs currently connected to the public relay (excluding relay itself).
pub fn fetch_relay_rendezvous(host: Option<&str>) -> Vec<String> {
    let env_host = std::env::var("MYCELIUM_RELAY_HOST").ok();
    let host = host.or(env_host.as_deref()).unwrap_or(DEFAULT_RELAY_HOST);
    let url = format!("https://{host}/rendezvous");
    let resp = match ureq::get(&url)
        .timeout(std::time::Duration::from_secs(8))
        .call()
    {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!("rendezvous fetch failed: {e}");
            return Vec::new();
        }
    };
    let body: RendezvousResponse = match resp.into_json() {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("rendezvous json failed: {e}");
            return Vec::new();
        }
    };
    body.peers
}
