//! Safe mode bridge restrictions (H20).

/// Methods allowed while safe mode is active (read-only + clock).
pub fn method_allowed_in_safe_mode(method: &str) -> bool {
    matches!(
        method,
        "storage.get"
            | "storage.list"
            | "bulletin.get"
            | "util.now"
            | "app.get_id"
            | "app.get_version"
    )
}

pub fn safe_mode_denial_message(method: &str) -> String {
    format!("blocked in safe mode: {method}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_payments_in_safe_mode() {
        assert!(!method_allowed_in_safe_mode("payment.get_balance"));
        assert!(method_allowed_in_safe_mode("storage.get"));
    }
}
