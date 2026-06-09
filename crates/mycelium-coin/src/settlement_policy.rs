//! Bounded emergency settlement policy — hard caps and velocity limits.
//!
//! MeshCoin is **not** a cryptocurrency; these limits enforce low-value, utility-only use.

use crate::ledger::{Transaction, MUON_PER_MXC};
use serde::{Deserialize, Serialize};

/// Beta-safe defaults: ~€100 soft cap, €300 hard cap (1 MXC ≈ €1 utility unit for policy math).
pub const DEFAULT_MAX_BALANCE_MXC: u64 = 100;
pub const HARD_MAX_BALANCE_MXC: u64 = 300;
pub const DEFAULT_MAX_TX_MXC: u64 = 25;
pub const MAX_TX_SERIALIZED_BYTES: usize = 4096;

/// Outbound velocity (anti-burst / anti-laundering heuristics).
pub const DEFAULT_MAX_TX_PER_HOUR: u32 = 20;
pub const DEFAULT_MAX_OUTBOUND_MXC_PER_HOUR: u64 = 50;

const HOUR_MS: u64 = 3_600_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SettlementPolicy {
    /// Maximum confirmed + pending balance per address (muon).
    pub max_balance_muon: u64,
    /// Maximum single transfer amount excluding fee (muon).
    pub max_transfer_muon: u64,
    /// Max serialized transaction size on the wire.
    pub max_tx_bytes: usize,
    pub max_tx_per_hour: u32,
    pub max_outbound_muon_per_hour: u64,
}

impl Default for SettlementPolicy {
    fn default() -> Self {
        Self::beta_defaults()
    }
}

impl SettlementPolicy {
    pub fn beta_defaults() -> Self {
        Self {
            max_balance_muon: DEFAULT_MAX_BALANCE_MXC * MUON_PER_MXC,
            max_transfer_muon: DEFAULT_MAX_TX_MXC * MUON_PER_MXC,
            max_tx_bytes: MAX_TX_SERIALIZED_BYTES,
            max_tx_per_hour: DEFAULT_MAX_TX_PER_HOUR,
            max_outbound_muon_per_hour: DEFAULT_MAX_OUTBOUND_MXC_PER_HOUR * MUON_PER_MXC,
        }
    }

    pub fn hard_cap() -> Self {
        let mut p = Self::beta_defaults();
        p.max_balance_muon = HARD_MAX_BALANCE_MXC * MUON_PER_MXC;
        p
    }

    pub fn check_transfer_amount(&self, amount_muon: u64) -> Result<(), String> {
        if amount_muon == 0 {
            return Err("transfer amount must be positive".into());
        }
        if amount_muon > self.max_transfer_muon {
            return Err(format!(
                "transfer exceeds max single settlement ({} muon cap)",
                self.max_transfer_muon
            ));
        }
        Ok(())
    }

    pub fn check_tx_size(&self, tx: &Transaction, encoded_len: usize) -> Result<(), String> {
        if encoded_len > self.max_tx_bytes {
            return Err(format!(
                "settlement event exceeds max size ({} bytes)",
                self.max_tx_bytes
            ));
        }
        self.check_transfer_amount(tx.amount_muon)
    }

    pub fn check_balance_cap(
        &self,
        balance_muon: u64,
        pending_received_muon: u64,
        incoming_muon: u64,
    ) -> Result<(), String> {
        let total = balance_muon
            .saturating_add(pending_received_muon)
            .saturating_add(incoming_muon);
        if total > self.max_balance_muon {
            return Err(format!(
                "balance would exceed vault cap ({} muon max)",
                self.max_balance_muon
            ));
        }
        Ok(())
    }

    pub fn check_velocity(
        &self,
        recent: &[(u64, u64)],
        now_ms: u64,
        new_amount_muon: u64,
    ) -> Result<(), String> {
        let window_start = now_ms.saturating_sub(HOUR_MS);
        let mut count = 0u32;
        let mut outbound = 0u64;
        for (ts, amt) in recent {
            if *ts >= window_start {
                count += 1;
                outbound = outbound.saturating_add(*amt);
            }
        }
        if count >= self.max_tx_per_hour {
            return Err("velocity limit: too many settlements per hour".into());
        }
        let projected = outbound.saturating_add(new_amount_muon);
        if projected > self.max_outbound_muon_per_hour {
            return Err("velocity limit: hourly outbound settlement cap exceeded".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beta_defaults_match_prompt() {
        let p = SettlementPolicy::beta_defaults();
        assert_eq!(p.max_balance_muon, 100 * MUON_PER_MXC);
        assert_eq!(p.max_transfer_muon, 25 * MUON_PER_MXC);
    }
}
