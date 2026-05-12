use crate::ledger::MUON_PER_MXC;
use crate::node::CoinNode;
use crate::payload::CoinPayload;
use crate::refill::RefillRequest;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HotWalletConfig {
    /// Max on-device balance (Muon).
    pub max_cache_muon: u64,
    pub refill_threshold_muon: u64,
    pub refill_amount_muon: u64,
    pub cold_wallet_address: Option<String>,
}

impl Default for HotWalletConfig {
    fn default() -> Self {
        let max = 50 * MUON_PER_MXC;
        Self {
            max_cache_muon: max,
            refill_threshold_muon: max * 80 / 100,
            refill_amount_muon: max,
            cold_wallet_address: None,
        }
    }
}

pub struct HotWallet {
    config: Arc<RwLock<HotWalletConfig>>,
    coin_node: Arc<CoinNode>,
    identity_path: String,
}

impl HotWallet {
    pub fn new(config: HotWalletConfig, coin_node: Arc<CoinNode>, identity_path: String) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            coin_node,
            identity_path,
        }
    }

    pub async fn maybe_refill(&self) -> anyhow::Result<RefillResult> {
        let (confirmed, _pending) = self.coin_node.balance()?;
        let (cold_addr, to_request) = {
            let config = self.config.read().await;
            if config.cold_wallet_address.is_none() {
                return Ok(RefillResult::NoColdWallet);
            }
            if confirmed >= config.refill_threshold_muon {
                return Ok(RefillResult::NotNeeded {
                    current_muon: confirmed,
                });
            }
            let cold_addr = config.cold_wallet_address.clone().unwrap();
            let to_request = config.refill_amount_muon.saturating_sub(confirmed);
            (cold_addr, to_request)
        };

        let hot_kp = mycelium_node::load_or_create_keypair(&self.identity_path)?;
        let req = RefillRequest::new_signed(&hot_kp, cold_addr.clone(), to_request, 1000)?;
        self.coin_node
            .broadcast_coin_payload(CoinPayload::RefillRequest(req.clone()))
            .await?;

        info!(
            "hot wallet below threshold ({} MXC), broadcast refill {} MXC from cold {}",
            confirmed / MUON_PER_MXC,
            to_request / MUON_PER_MXC,
            cold_addr
        );

        Ok(RefillResult::RefillBroadcast {
            amount_muon: to_request,
            request_id: req.request_id,
        })
    }

    pub async fn enforce_cap(&self) -> anyhow::Result<()> {
        let (confirmed, _) = self.coin_node.balance()?;
        let maybe_return = {
            let config = self.config.read().await;
            if confirmed > config.max_cache_muon {
                let excess = confirmed - config.max_cache_muon;
                config.cold_wallet_address.clone().map(|c| (c, excess))
            } else {
                None
            }
        };
        if let Some((cold_addr, excess)) = maybe_return {
            info!(
                "hot wallet over cap, returning {} MXC to cold wallet",
                excess / MUON_PER_MXC
            );
            self.coin_node
                .submit_transfer_from_identity_path(
                    &self.identity_path,
                    cold_addr,
                    excess,
                    1000,
                    Some("auto-cap-return".into()),
                )
                .await?;
        }
        Ok(())
    }

    pub async fn update_config(&self, new_config: HotWalletConfig) {
        *self.config.write().await = new_config;
    }

    pub async fn config(&self) -> HotWalletConfig {
        self.config.read().await.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefillResult {
    NoColdWallet,
    NotNeeded { current_muon: u64 },
    RefillBroadcast {
        amount_muon: u64,
        request_id: String,
    },
}
