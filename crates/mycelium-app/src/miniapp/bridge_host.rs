// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! Central bridge dispatcher for mini-app `window.mycelium.*` calls (shared by hosts).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use mycelium_coin::{CoinNode, PaymentRequest};
use mycelium_core::data::now_ms;
use serde_json::{json, Value};

use crate::miniapp::bridge_limits::check_bridge_rate;
use crate::miniapp::bridge_session::validate_session;
use crate::miniapp::capability_token::{permission_for_method, validate_capability};
use crate::miniapp::manifest::Permission;
use crate::miniapp::safe_mode::{method_allowed_in_safe_mode, safe_mode_denial_message};
use crate::miniapp::storage_quota::{
    validate_key, validate_value, MAX_KEYS_PER_APP, MAX_TOTAL_BYTES_PER_APP,
};
use crate::miniapp::AppStore;
use crate::node::AppNode;
use crate::proximity::PresenceProfile;
use crate::storage::AppStorage;

/// Per-app broadcast counter: (window_start_ms, count).
static BROADCAST_LIMITS: LazyLock<Mutex<HashMap<String, (u64, u32)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

const BROADCAST_MAX_PER_HOUR: u32 = 10;
const BROADCAST_WINDOW_MS: u64 = 3_600_000;

fn check_broadcast_rate(app_id: &str) -> Result<(), String> {
    let now = now_ms();
    let mut guard = BROADCAST_LIMITS
        .lock()
        .map_err(|_| "broadcast limiter unavailable".to_string())?;
    let entry = guard.entry(app_id.to_string()).or_insert((now, 0));
    if now.saturating_sub(entry.0) > BROADCAST_WINDOW_MS {
        *entry = (now, 0);
    }
    if entry.1 >= BROADCAST_MAX_PER_HOUR {
        return Err("broadcast rate limit exceeded (10/hour)".into());
    }
    entry.1 += 1;
    Ok(())
}

pub struct BridgeHost {
    app_id: String,
    permissions: Vec<Permission>,
    app_node: Arc<AppNode>,
    app_storage: Arc<AppStorage>,
    app_store: Arc<AppStore>,
    coin: Option<Arc<CoinNode>>,
    local_peer_id: String,
    enc_pubkey_hex: String,
    safe_mode: bool,
    cap_mac_key: [u8; 32],
}

impl BridgeHost {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        app_id: String,
        permissions: Vec<Permission>,
        app_node: Arc<AppNode>,
        app_storage: Arc<AppStorage>,
        app_store: Arc<AppStore>,
        coin: Option<Arc<CoinNode>>,
        local_peer_id: String,
        enc_pubkey_hex: String,
        safe_mode: bool,
        cap_mac_key: [u8; 32],
    ) -> Self {
        Self {
            app_id,
            permissions,
            app_node,
            app_storage,
            app_store,
            coin,
            local_peer_id,
            enc_pubkey_hex,
            safe_mode,
            cap_mac_key,
        }
    }

    fn require_capability(&self, method: &str, args: &Value) -> Result<(), String> {
        let Some(perm) = permission_for_method(method) else {
            return Ok(());
        };
        self.require_permission(perm.clone())?;
        let session = args
            .get("_session")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing bridge session token".to_string())?;
        let cap = args
            .get("_cap")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("missing capability token for {method}"))?;
        validate_capability(&self.cap_mac_key, &self.app_id, &perm, session, cap)
    }

    fn require_not_blocked_by_safe_mode(&self, method: &str) -> Result<(), String> {
        if self.safe_mode && !method_allowed_in_safe_mode(method) {
            return Err(safe_mode_denial_message(method));
        }
        Ok(())
    }

    fn scoped_key(&self, key: &str) -> String {
        format!("app:{}:{key}", self.app_id)
    }

    fn scoped_prefix(&self, prefix: &str) -> String {
        format!("app:{}:{prefix}", self.app_id)
    }

    fn require_permission(&self, perm: Permission) -> Result<(), String> {
        if self.permissions.contains(&perm) {
            Ok(())
        } else {
            Err(format!("permission denied: {perm:?}"))
        }
    }

    fn require_bulletin_scope(&self, scope: &str) -> Result<(), String> {
        let manifest = self
            .app_store
            .get_manifest(&self.app_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "app not installed".to_string())?;
        if manifest.allows_bulletin_scope(scope) {
            Ok(())
        } else {
            Err(format!("bulletin scope not allowed: {scope}"))
        }
    }

    fn check_storage_quota(&self, scoped_key: &str, new_value_len: usize) -> Result<(), String> {
        let prefix = format!("app:{}:", self.app_id);
        let keys = self
            .app_storage
            .miniapp_list(&prefix)
            .map_err(|e| e.to_string())?;
        if !keys.iter().any(|k| k == scoped_key) && keys.len() >= MAX_KEYS_PER_APP {
            return Err(format!("storage key limit exceeded ({MAX_KEYS_PER_APP})"));
        }
        let mut total = new_value_len;
        for k in &keys {
            if k == scoped_key {
                continue;
            }
            if let Ok(Some(v)) = self.app_storage.miniapp_get(k) {
                total += v.len();
            }
        }
        if total > MAX_TOTAL_BYTES_PER_APP {
            return Err(format!(
                "storage quota exceeded ({MAX_TOTAL_BYTES_PER_APP} bytes per app)"
            ));
        }
        Ok(())
    }

    pub async fn handle(&self, method: &str, args: &Value) -> Result<Value, String> {
        if let Some(tok) = args.get("_session").and_then(|v| v.as_str()) {
            validate_session(&self.app_id, tok)?;
        } else {
            return Err("missing bridge session token".into());
        }
        if let Err(e) = check_bridge_rate(&self.app_id) {
            let _ = self.app_store.record_bridge_anomaly(&self.app_id);
            return Err(e);
        }
        self.require_not_blocked_by_safe_mode(method)?;
        self.require_capability(method, args)?;

        match method {
            "identity.get" => {
                self.require_permission(Permission::Identity)?;
                Ok(json!({
                    "peer_id": self.local_peer_id,
                    "display_name": self.app_node.display_name().await,
                    "enc_pubkey": self.enc_pubkey_hex,
                }))
            }

            "messaging.send" => {
                self.require_permission(Permission::Messaging)?;
                let to = args
                    .get("to_peer")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| "missing to_peer".to_string())?;
                let payload = args
                    .get("payload")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| "missing payload".to_string())?;
                let body = format!("[app/{}] {}", self.app_id, payload);
                self.app_node
                    .send_chat(Some(to.to_string()), body)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(json!("ok"))
            }

            "messaging.subscribe" => {
                self.require_permission(Permission::Messaging)?;
                Ok(json!("subscribed"))
            }

            "messaging.broadcast" => {
                self.require_permission(Permission::MessagingBroadcast)?;
                check_broadcast_rate(&self.app_id)?;
                let payload = args
                    .get("payload")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| "missing payload".to_string())?;
                let _scope = args
                    .get("scope")
                    .and_then(|s| s.as_str())
                    .unwrap_or("mycelium/chat");
                let body = format!("[app/{}] {}", self.app_id, payload);
                self.app_node
                    .send_chat(None, body)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(json!("ok"))
            }

            "payment.request" => {
                self.require_permission(Permission::Payments)?;
                let amount = json_u64(args, "amount_muon")?;
                let memo = args.get("memo").and_then(|m| m.as_str()).map(String::from);
                Ok(json!({
                    "_bridge_action": "payment_confirmation_required",
                    "amount_muon": amount,
                    "memo": memo,
                }))
            }

            "payment.create_qr" => {
                self.require_permission(Permission::Payments)?;
                let coin = self
                    .coin
                    .as_ref()
                    .ok_or_else(|| "coin not available".to_string())?;
                let amount = json_u64(args, "amount_muon")?;
                let memo = args.get("memo").and_then(|m| m.as_str()).map(String::from);
                let address = coin.local_address().to_string();
                let req = PaymentRequest::new(address, amount, memo);
                if req.is_expired() {
                    return Err("payment request expired".into());
                }
                let uri = req.to_uri();
                Ok(json!({
                    "uri": uri,
                    "_bridge_action": "render_qr",
                    "_qr_content": uri,
                }))
            }

            "payment.get_balance" => {
                self.require_permission(Permission::Payments)?;
                let coin = self
                    .coin
                    .as_ref()
                    .ok_or_else(|| "coin not available".to_string())?;
                let (confirmed, pending) = coin.balance().map_err(|e| e.to_string())?;
                Ok(json!({
                    "confirmed_muon": confirmed,
                    "pending_muon": pending,
                }))
            }

            "storage.get" => {
                self.require_permission(Permission::Storage)?;
                let key = args
                    .get("key")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| "missing key".to_string())?;
                let sk = self.scoped_key(key);
                let v = self
                    .app_storage
                    .miniapp_get(&sk)
                    .map_err(|e| e.to_string())?;
                Ok(v.map(|s| json!(s)).unwrap_or(Value::Null))
            }

            "storage.set" => {
                self.require_permission(Permission::Storage)?;
                let key = args
                    .get("key")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| "missing key".to_string())?;
                let value = args
                    .get("value")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| "missing value".to_string())?;
                validate_key(key)?;
                validate_value(value)?;
                let sk = self.scoped_key(key);
                self.check_storage_quota(&sk, value.len())?;
                self.app_storage
                    .miniapp_set(&sk, value)
                    .map_err(|e| e.to_string())?;
                Ok(json!("ok"))
            }

            "storage.delete" => {
                self.require_permission(Permission::Storage)?;
                let key = args
                    .get("key")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| "missing key".to_string())?;
                let sk = self.scoped_key(key);
                self.app_storage
                    .miniapp_delete(&sk)
                    .map_err(|e| e.to_string())?;
                Ok(json!("ok"))
            }

            "storage.list" => {
                self.require_permission(Permission::Storage)?;
                let prefix = args.get("prefix").and_then(|p| p.as_str()).unwrap_or("");
                let pfx = self.scoped_prefix(prefix);
                let keys = self
                    .app_storage
                    .miniapp_list(&pfx)
                    .map_err(|e| e.to_string())?;
                let strip = format!("app:{}:", self.app_id);
                let out: Vec<String> = keys
                    .into_iter()
                    .filter_map(|k| k.strip_prefix(&strip).map(str::to_string))
                    .collect();
                Ok(json!(out))
            }

            "bulletin.get" => {
                self.require_permission(Permission::BulletinRead)?;
                let scope = args
                    .get("scope")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| "missing scope".to_string())?;
                self.require_bulletin_scope(scope)?;
                let _ = self
                    .app_node
                    .node_handle()
                    .subscribe_bulletin_scope(scope.to_string())
                    .await;
                let posts = self
                    .app_node
                    .bulletins_for_scope(scope)
                    .map_err(|e| e.to_string())?;
                let list: Vec<Value> = posts
                    .iter()
                    .map(|p| {
                        json!({
                            "id": p.id.to_string(),
                            "title": p.title,
                            "body": p.body,
                            "from_display_name": p.from_display_name,
                            "scope": p.scope,
                            "timestamp_ms": p.timestamp_ms,
                            "expires_at_ms": p.expires_at_ms,
                        })
                    })
                    .collect();
                Ok(Value::Array(list))
            }

            "bulletin.post" => {
                self.require_permission(Permission::BulletinWrite)?;
                let scope = args
                    .get("scope")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| "missing scope".to_string())?;
                self.require_bulletin_scope(scope)?;
                let title = args
                    .get("title")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| "missing title".to_string())?;
                let body = args
                    .get("body")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| "missing body".to_string())?;
                let ttl = args.get("ttl_secs").and_then(json_as_u64).unwrap_or(86_400);
                self.app_node
                    .post_bulletin(scope.into(), title.into(), body.into(), ttl)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(json!("ok"))
            }

            "peers.nearby" => {
                self.require_permission(Permission::PeerDiscovery)?;
                let peers = self.app_node.node_handle().known_peers().await;
                Ok(json!(peers))
            }

            "proximity.start" => {
                self.require_permission(Permission::PeerDiscovery)?;
                self.require_permission(Permission::Messaging)?;
                let profile = parse_presence_profile(args)?;
                let ttl = args.get("ttl_secs").and_then(json_as_u64).unwrap_or(300) as u32;
                self.app_node
                    .start_proximity(profile, ttl)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(json!("ok"))
            }

            "proximity.stop" => {
                self.require_permission(Permission::PeerDiscovery)?;
                self.app_node.stop_proximity().await;
                Ok(json!("ok"))
            }

            "proximity.nearby" => {
                self.require_permission(Permission::PeerDiscovery)?;
                let profiles = self.app_node.nearby_profiles().await;
                let list: Vec<Value> = profiles
                    .into_iter()
                    .map(|entry| {
                        let s = entry.signal;
                        json!({
                            "ephemeral_id": s.ephemeral_id.to_string(),
                            "enc_pubkey_hex": s.enc_pubkey_hex,
                            "display_name": s.profile.display_name,
                            "bio": s.profile.bio,
                            "age": s.profile.age,
                            "gender": s.profile.gender,
                            "looking_for": s.profile.looking_for,
                            "interests": s.profile.interests,
                            "photo_base64": s.profile.photo_base64,
                            "seen_at_ms": s.created_at_ms,
                            "has_expired": s.is_expired(),
                            "interest_sent": entry.interest_sent,
                            "interest_received": entry.interest_received,
                            "is_mutual": entry.is_mutual,
                        })
                    })
                    .collect();
                Ok(Value::Array(list))
            }

            "proximity.connect" => {
                self.require_permission(Permission::PeerDiscovery)?;
                let enc_pubkey_hex = args
                    .get("enc_pubkey_hex")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| "missing enc_pubkey_hex".to_string())?;
                let is_mutual = self
                    .app_node
                    .express_proximity_interest(enc_pubkey_hex.to_string())
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(json!({ "is_mutual": is_mutual }))
            }

            "proximity.messages" => {
                self.require_permission(Permission::Messaging)?;
                let since_ms = args.get("since_ms").and_then(json_as_u64).unwrap_or(0);
                let messages = self.app_node.proximity_messages(since_ms).await;
                let list: Vec<Value> = messages
                    .into_iter()
                    .map(|m| {
                        json!({
                            "from_enc_pubkey_hex": m.from_enc_pubkey_hex,
                            "body": m.body,
                            "received_at_ms": m.received_at_ms,
                        })
                    })
                    .collect();
                Ok(Value::Array(list))
            }

            "proximity.send_message" => {
                self.require_permission(Permission::Messaging)?;
                let enc_pubkey_hex = args
                    .get("enc_pubkey_hex")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| "missing enc_pubkey_hex".to_string())?;
                let message = args
                    .get("message")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| "missing message".to_string())?;
                self.app_node
                    .send_proximity_message(enc_pubkey_hex.to_string(), message.to_string())
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(json!("ok"))
            }

            "util.now" => Ok(json!(mycelium_core::data::now_ms())),

            "util.scan_qr" => {
                self.require_permission(Permission::Camera)?;
                Ok(json!({ "_bridge_action": "open_qr_scanner" }))
            }

            "util.alert" => {
                let message = args
                    .get("message")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| "missing message".to_string())?;
                Ok(json!({
                    "_bridge_action": "show_alert",
                    "message": message,
                }))
            }

            "util.confirm" => {
                let message = args
                    .get("message")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| "missing message".to_string())?;
                Ok(json!({
                    "_bridge_action": "show_confirm",
                    "message": message,
                }))
            }

            "app.get_id" => Ok(json!(self.app_id)),

            "app.get_version" => {
                let v = self
                    .app_store
                    .get_manifest(&self.app_id)
                    .map_err(|e| e.to_string())?
                    .map(|m| m.version)
                    .unwrap_or_default();
                Ok(json!(v))
            }

            _ => Err(format!("unknown bridge method: {method}")),
        }
    }
}

fn json_u64(args: &Value, key: &str) -> Result<u64, String> {
    args.get(key)
        .and_then(json_as_u64)
        .ok_or_else(|| format!("missing or invalid {key}"))
}

fn json_as_u64(v: &Value) -> Option<u64> {
    if let Some(n) = v.as_u64() {
        return Some(n);
    }
    if let Some(n) = v.as_i64() {
        return u64::try_from(n).ok();
    }
    None
}

fn parse_presence_profile(args: &Value) -> Result<PresenceProfile, String> {
    fn opt_str(v: &Value, key: &str) -> Option<String> {
        v.get(key)
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from)
    }
    let interests = args
        .get("interests")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let age = args.get("age").and_then(|v| {
        v.as_u64()
            .and_then(|n| u8::try_from(n).ok())
            .or_else(|| v.as_i64().and_then(|n| u8::try_from(n).ok()))
    });
    Ok(PresenceProfile {
        display_name: opt_str(args, "display_name"),
        bio: opt_str(args, "bio"),
        age,
        gender: opt_str(args, "gender"),
        looking_for: opt_str(args, "looking_for"),
        interests,
        photo_base64: opt_str(args, "photo_base64"),
    })
}
