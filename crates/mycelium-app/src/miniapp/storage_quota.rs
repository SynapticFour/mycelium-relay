// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! Mini-app storage quotas (Cell C2 / H09).

pub const MAX_KEY_LEN: usize = 128;
pub const MAX_VALUE_LEN: usize = 32 * 1024;
pub const MAX_KEYS_PER_APP: usize = 256;
pub const MAX_TOTAL_BYTES_PER_APP: usize = 256 * 1024;

pub fn validate_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("storage key must not be empty".into());
    }
    if key.len() > MAX_KEY_LEN {
        return Err(format!("storage key exceeds {MAX_KEY_LEN} bytes"));
    }
    if key.contains('\0') {
        return Err("storage key contains invalid characters".into());
    }
    Ok(())
}

pub fn validate_value(value: &str) -> Result<(), String> {
    if value.len() > MAX_VALUE_LEN {
        return Err(format!("storage value exceeds {MAX_VALUE_LEN} bytes"));
    }
    Ok(())
}
