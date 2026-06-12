// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
pub enum NodeState {
    Active,
    Intermittent,
    Passive,
}

impl NodeState {
    pub fn sync_interval_secs(self) -> u64 {
        match self {
            Self::Active => 2,
            Self::Intermittent => 10,
            Self::Passive => 45,
        }
    }
}
