// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! Rendezvous HTTP API against the public relay (opt-in discovery).

use serde::{Deserialize, Serialize};

const DEFAULT_RELAY_HOST: &str = "mycelium-relay.fly.dev";

#[derive(Debug, Deserialize)]
struct RendezvousResponse {
    peers: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RegisterBody<'a> {
    peer_id: &'a str,
}

fn relay_host(host: Option<&str>) -> String {
    let env_host = std::env::var("MYCELIUM_RELAY_HOST").ok();
    host.or(env_host.as_deref())
        .unwrap_or(DEFAULT_RELAY_HOST)
        .to_string()
}

/// Register or unregister this peer for relay rendezvous (opt-in visibility).
pub fn set_relay_rendezvous_registration(peer_id: &str, register: bool, host: Option<&str>) {
    let host = relay_host(host);
    let path = if register {
        "rendezvous/register"
    } else {
        "rendezvous/unregister"
    };
    let url = format!("https://{host}/{path}");
    let body = RegisterBody { peer_id };
    let result = ureq::post(&url)
        .timeout(std::time::Duration::from_secs(8))
        .send_json(body);
    if let Err(e) = result {
        tracing::debug!("rendezvous registration ({register}) failed: {e}");
    }
}

/// Returns peer IDs opted in and currently connected to the public relay.
pub fn fetch_relay_rendezvous(host: Option<&str>) -> Vec<String> {
    let host = relay_host(host);
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
