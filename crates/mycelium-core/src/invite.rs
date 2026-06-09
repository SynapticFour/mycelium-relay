//! Compact invite payloads for QR codes and deep links (all Mycelium clients).

const PREFIX_V1: &str = "mycelium://invite/v1#";

/// QR-friendly invite (peer id only; dial via default relay circuit).
pub fn encode_invite(peer_id: &str) -> String {
    format!("{PREFIX_V1}{}", peer_id.trim())
}

/// Parse invite QR, multiaddr, or legacy `peerId@host:port` form.
pub fn parse_invite(raw: &str) -> Option<InviteTarget> {
    let input = raw.trim();
    if input.is_empty() {
        return None;
    }
    if let Some(peer_id) = input.strip_prefix(PREFIX_V1) {
        let peer_id = peer_id.trim();
        if peer_id.is_empty() {
            return None;
        }
        return Some(InviteTarget::PeerId(peer_id.to_string()));
    }
    if input.starts_with('/') {
        return Some(InviteTarget::Multiaddr(input.to_string()));
    }
    if looks_like_peer_id(input) {
        return Some(InviteTarget::PeerId(input.to_string()));
    }
    legacy_host_port_to_multiaddr(input)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InviteTarget {
    PeerId(String),
    Multiaddr(String),
}

impl InviteTarget {
    pub fn to_dial_multiaddr(&self) -> Option<String> {
        match self {
            InviteTarget::Multiaddr(addr) => Some(addr.clone()),
            InviteTarget::PeerId(id) => crate::bootstrap::relay_circuit_multiaddr(id),
        }
    }
}

fn looks_like_peer_id(s: &str) -> bool {
    s.len() >= 40 && s.starts_with("12D3Koo")
}

fn legacy_host_port_to_multiaddr(input: &str) -> Option<InviteTarget> {
    let parts = input.split('@').collect::<Vec<_>>();
    if parts.len() != 2 {
        return None;
    }
    let peer_id = parts[0].trim();
    let host_and_port = parts[1].split(':').collect::<Vec<_>>();
    if peer_id.is_empty() || host_and_port.len() != 2 {
        return None;
    }
    let host = host_and_port[0];
    let port = host_and_port[1];
    if port.parse::<u16>().is_err() {
        return None;
    }
    Some(InviteTarget::Multiaddr(format!(
        "/ip4/{host}/tcp/{port}/p2p/{peer_id}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_invite_v1() {
        let id = "12D3KooWGv6goWd2fHwcigDjqQm5Dfw28UijwEjnMcYwTpPRPZy6";
        let encoded = encode_invite(id);
        match parse_invite(&encoded).unwrap() {
            InviteTarget::PeerId(p) => assert_eq!(p, id),
            _ => panic!("expected peer id"),
        }
    }

    #[test]
    fn parses_dns4_multiaddr() {
        let addr = "/dns4/mycelium-relay.fly.dev/tcp/4001/p2p/12D3KooW/p2p-circuit/p2p/12D3KooWOther";
        match parse_invite(addr).unwrap() {
            InviteTarget::Multiaddr(m) => assert_eq!(m, addr),
            _ => panic!("expected multiaddr"),
        }
    }
}
