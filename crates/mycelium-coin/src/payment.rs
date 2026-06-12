// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! QR-friendly payment URI (`mxcpay:`) for MeshCoin transfers.

use mycelium_core::data::now_ms;
use serde::{Deserialize, Serialize};

/// Payment request encoded as `mxcpay:{address}?amount={muon}&memo=…&expires=…`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaymentRequest {
    pub to_address: String,
    pub amount_muon: u64,
    pub memo: Option<String>,
    pub expires_at_ms: Option<u64>,
    pub request_id: String,
}

impl PaymentRequest {
    pub fn new(to_address: String, amount_muon: u64, memo: Option<String>) -> Self {
        Self {
            to_address,
            amount_muon,
            memo,
            expires_at_ms: Some(now_ms().saturating_add(30 * 60 * 1000)),
            request_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    pub fn to_uri(&self) -> String {
        let mut uri = format!("mxcpay:{}?amount={}", self.to_address, self.amount_muon);
        if let Some(memo) = &self.memo {
            uri.push_str(&format!("&memo={}", urlencoding::encode(memo)));
        }
        if let Some(exp) = self.expires_at_ms {
            uri.push_str(&format!("&expires={}", exp));
        }
        uri
    }

    pub fn from_uri(uri: &str) -> anyhow::Result<Self> {
        let rest = uri
            .strip_prefix("mxcpay:")
            .ok_or_else(|| anyhow::anyhow!("not a mxcpay URI"))?;
        let (address, params) = rest.split_once('?').unwrap_or((rest, ""));
        let mut amount_muon = 0u64;
        let mut memo = None;
        let mut expires_at_ms = None;
        for param in params.split('&').filter(|p| !p.is_empty()) {
            if let Some(v) = param.strip_prefix("amount=") {
                amount_muon = v.parse().unwrap_or(0);
            } else if let Some(v) = param.strip_prefix("memo=") {
                memo = Some(urlencoding::decode(v)?.into_owned());
            } else if let Some(v) = param.strip_prefix("expires=") {
                expires_at_ms = v.parse().ok();
            }
        }
        Ok(Self {
            to_address: address.to_string(),
            amount_muon,
            memo,
            expires_at_ms,
            request_id: uuid::Uuid::new_v4().to_string(),
        })
    }

    pub fn is_expired(&self) -> bool {
        self.expires_at_ms
            .map(|exp| exp < now_ms())
            .unwrap_or(false)
    }
}
