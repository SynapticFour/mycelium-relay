// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContactStatus {
    Pending,
    Accepted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub peer_id: String,
    pub display_name: String,
    pub added_at_ms: u64,
    pub status: ContactStatus,
}

impl Contact {
    pub fn new(peer_id: String, display_name: String, status: ContactStatus) -> Self {
        Self {
            peer_id,
            display_name,
            added_at_ms: mycelium_core::data::now_ms(),
            status,
        }
    }
}
