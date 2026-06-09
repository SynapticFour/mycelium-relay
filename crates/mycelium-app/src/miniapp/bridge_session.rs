//! Per-app bridge session tokens (Cell C2).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use mycelium_core::data::now_ms;
use uuid::Uuid;

const SESSION_TTL_MS: u64 = 3_600_000;

#[derive(Debug, Clone)]
struct Session {
    token: String,
    issued_at_ms: u64,
}

static SESSIONS: LazyLock<Mutex<HashMap<String, Session>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Issue a new session token for an app instance (invalidates prior token for that app).
pub fn issue_session(app_id: &str) -> String {
    let token = Uuid::new_v4().to_string();
    let now = now_ms();
    let mut guard = SESSIONS.lock().expect("bridge session lock");
    guard.insert(
        app_id.to_string(),
        Session {
            token: token.clone(),
            issued_at_ms: now,
        },
    );
    token
}

pub fn revoke_session(app_id: &str) {
    if let Ok(mut guard) = SESSIONS.lock() {
        guard.remove(app_id);
    }
}

pub fn validate_session(app_id: &str, token: &str) -> Result<(), String> {
    let now = now_ms();
    let guard = SESSIONS
        .lock()
        .map_err(|_| "bridge session unavailable".to_string())?;
    let Some(sess) = guard.get(app_id) else {
        return Err("bridge session expired or not issued".into());
    };
    if sess.token != token {
        return Err("invalid bridge session token".into());
    }
    if now.saturating_sub(sess.issued_at_ms) > SESSION_TTL_MS {
        return Err("bridge session expired".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_issue_and_validate() {
        let tok = issue_session("com.test.app");
        assert!(validate_session("com.test.app", &tok).is_ok());
        assert!(validate_session("com.test.app", "wrong").is_err());
        revoke_session("com.test.app");
        assert!(validate_session("com.test.app", &tok).is_err());
    }
}
