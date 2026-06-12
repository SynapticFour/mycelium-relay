// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! HMAC capability tokens for runtime permission grants (H25 / P4).

use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use super::manifest::Permission;

type HmacSha256 = Hmac<Sha256>;

const CAP_VERSION: u8 = 1;
/// Default runtime grant lifetime (15 minutes).
pub const DEFAULT_CAP_TTL_MS: u64 = 900_000;

/// Derive MAC key from host storage path (stable per device database).
pub fn mac_key_from_db_path(db_path: &str) -> [u8; 32] {
    blake3::derive_key("mycelium-miniapp-cap-v1", db_path.as_bytes())
}

pub fn permission_name(perm: &Permission) -> &'static str {
    match perm {
        Permission::Messaging => "Messaging",
        Permission::MessagingBroadcast => "MessagingBroadcast",
        Permission::Identity => "Identity",
        Permission::Payments => "Payments",
        Permission::Storage => "Storage",
        Permission::BulletinRead => "BulletinRead",
        Permission::BulletinWrite => "BulletinWrite",
        Permission::PeerDiscovery => "PeerDiscovery",
        Permission::Camera => "Camera",
    }
}

pub fn parse_permission_name(name: &str) -> Option<Permission> {
    match name {
        "Messaging" => Some(Permission::Messaging),
        "MessagingBroadcast" => Some(Permission::MessagingBroadcast),
        "Identity" => Some(Permission::Identity),
        "Payments" => Some(Permission::Payments),
        "Storage" => Some(Permission::Storage),
        "BulletinRead" => Some(Permission::BulletinRead),
        "BulletinWrite" => Some(Permission::BulletinWrite),
        "PeerDiscovery" => Some(Permission::PeerDiscovery),
        "Camera" => Some(Permission::Camera),
        _ => None,
    }
}

/// Bridge methods that require a runtime capability token (tier 2–3).
pub fn permission_for_method(method: &str) -> Option<Permission> {
    match method {
        "identity.get" => Some(Permission::Identity),
        "messaging.send" => Some(Permission::Messaging),
        "messaging.broadcast" => Some(Permission::MessagingBroadcast),
        "payment.request" | "payment.create_qr" | "payment.get_balance" => {
            Some(Permission::Payments)
        }
        "util.scan_qr" => Some(Permission::Camera),
        "bulletin.post" => Some(Permission::BulletinWrite),
        "peers.nearby" => Some(Permission::PeerDiscovery),
        "proximity.start" => Some(Permission::PeerDiscovery),
        "proximity.nearby" => Some(Permission::PeerDiscovery),
        "proximity.connect" => Some(Permission::PeerDiscovery),
        "proximity.messages" => Some(Permission::Messaging),
        "proximity.send_message" => Some(Permission::Messaging),
        _ => None,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Issue a capability token bound to the current bridge session.
pub fn issue_capability(
    mac_key: &[u8; 32],
    app_id: &str,
    permission: &Permission,
    session_token: &str,
    ttl_ms: u64,
) -> String {
    let expires_at_ms = now_ms().saturating_add(ttl_ms);
    let tag = permission_name(permission);
    let payload = format!("{CAP_VERSION}|{app_id}|{tag}|{session_token}|{expires_at_ms}");
    let mut mac = HmacSha256::new_from_slice(mac_key).expect("HMAC key length");
    mac.update(payload.as_bytes());
    let sig = mac.finalize().into_bytes();
    URL_SAFE_NO_PAD.encode(format!("{payload}|{}", hex::encode(sig)))
}

pub fn validate_capability(
    mac_key: &[u8; 32],
    app_id: &str,
    permission: &Permission,
    session_token: &str,
    token: &str,
) -> Result<(), String> {
    let decoded = URL_SAFE_NO_PAD
        .decode(token)
        .map_err(|_| "invalid capability token encoding".to_string())?;
    let decoded =
        String::from_utf8(decoded).map_err(|_| "invalid capability token utf8".to_string())?;
    let (payload, sig_hex) = decoded
        .rsplit_once('|')
        .ok_or_else(|| "malformed capability token".to_string())?;
    let mut mac = HmacSha256::new_from_slice(mac_key)
        .map_err(|_| "capability MAC unavailable".to_string())?;
    mac.update(payload.as_bytes());
    let sig = hex::decode(sig_hex).map_err(|_| "invalid capability signature".to_string())?;
    mac.verify_slice(&sig)
        .map_err(|_| "invalid capability token".to_string())?;

    let parts: Vec<&str> = payload.split('|').collect();
    if parts.len() != 5 {
        return Err("malformed capability payload".into());
    }
    if parts[0].parse::<u8>().ok() != Some(CAP_VERSION) {
        return Err("unsupported capability version".into());
    }
    if parts[1] != app_id {
        return Err("capability token app mismatch".into());
    }
    if parts[2] != permission_name(permission) {
        return Err("capability token permission mismatch".into());
    }
    if parts[3] != session_token {
        return Err("capability token session mismatch".into());
    }
    let expires_at_ms: u64 = parts[4]
        .parse()
        .map_err(|_| "invalid capability expiry".to_string())?;
    if now_ms() > expires_at_ms {
        return Err("capability token expired".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_roundtrip_and_session_bind() {
        let key = mac_key_from_db_path("/tmp/test-db");
        let session = "sess-abc";
        let cap = issue_capability(
            &key,
            "com.app",
            &Permission::Identity,
            session,
            DEFAULT_CAP_TTL_MS,
        );
        assert!(validate_capability(&key, "com.app", &Permission::Identity, session, &cap).is_ok());
        assert!(
            validate_capability(&key, "com.app", &Permission::Identity, "other", &cap).is_err()
        );
        assert!(
            validate_capability(&key, "com.other", &Permission::Identity, session, &cap).is_err()
        );
    }
}
