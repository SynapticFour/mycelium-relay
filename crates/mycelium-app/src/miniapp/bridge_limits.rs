//! Bridge call rate limits (Cell C2 / H08).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use mycelium_core::data::now_ms;

const WINDOW_MS: u64 = 60_000;
const MAX_CALLS_PER_MINUTE: u32 = 120;

static BRIDGE_LIMITS: LazyLock<Mutex<HashMap<String, (u64, u32)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn check_bridge_rate(app_id: &str) -> Result<(), String> {
    let now = now_ms();
    let mut guard = BRIDGE_LIMITS
        .lock()
        .map_err(|_| "bridge rate limiter unavailable".to_string())?;
    let entry = guard.entry(app_id.to_string()).or_insert((now, 0));
    if now.saturating_sub(entry.0) > WINDOW_MS {
        *entry = (now, 0);
    }
    if entry.1 >= MAX_CALLS_PER_MINUTE {
        return Err("bridge rate limit exceeded (120/min)".into());
    }
    entry.1 += 1;
    Ok(())
}
