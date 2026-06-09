use crate::address::address_from_keypair;
use crate::ledger::{ApplyResult, LocalLedger, Transaction};
use crate::payload::CoinPayload;
use crate::settlement_policy::SettlementPolicy;
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{info, warn};

/// Broadcast / direct-send of coin frames (`AppMessage` with [`CoinPayload`] bytes) is implemented by the host (e.g. FFI).
#[async_trait]
pub trait CoinTransport: Send + Sync {
    async fn broadcast_coin_inner(&self, coin_inner: Vec<u8>) -> anyhow::Result<()>;
    async fn send_direct_coin_inner(
        &self,
        to_peer: String,
        coin_inner: Vec<u8>,
    ) -> anyhow::Result<()>;
}

#[derive(Clone)]
pub struct CoinNode {
    ledger: Arc<LocalLedger>,
    transport: Arc<dyn CoinTransport>,
    local_address: String,
    local_peer_id: String,
    /// Path to sled identity store for this node's coin keypair (cold signs outgoing refills).
    coin_identity_path: String,
}

impl std::fmt::Debug for CoinNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoinNode")
            .field("local_address", &self.local_address)
            .field("local_peer_id", &self.local_peer_id)
            .finish_non_exhaustive()
    }
}

impl CoinNode {
    pub fn new(
        ledger: Arc<LocalLedger>,
        transport: Arc<dyn CoinTransport>,
        local_address: String,
        local_peer_id: String,
        coin_identity_path: String,
    ) -> Self {
        Self {
            ledger,
            transport,
            local_address,
            local_peer_id,
            coin_identity_path,
        }
    }

    pub async fn broadcast_coin_payload(&self, payload: CoinPayload) -> anyhow::Result<()> {
        let inner = bincode::serialize(&payload)?;
        self.transport.broadcast_coin_inner(inner).await
    }

    pub fn local_address(&self) -> &str {
        &self.local_address
    }

    pub fn settlement_policy(&self) -> SettlementPolicy {
        self.ledger.policy().clone()
    }

    pub async fn submit_transfer_from_identity_path(
        &self,
        identity_path: &str,
        to: String,
        amount_muon: u64,
        fee_muon: u64,
        memo: Option<String>,
    ) -> anyhow::Result<()> {
        let kp = mycelium_node::load_or_create_keypair(identity_path)?;
        if address_from_keypair(&kp) != self.local_address {
            anyhow::bail!("identity does not match coin address");
        }
        let nonce = self.ledger.account_state(&self.local_address)?.next_nonce;
        let tx = Transaction::new(to, amount_muon, fee_muon, nonce, memo, &kp)?;
        self.send_transaction(tx).await
    }

    pub async fn send_transaction(&self, tx: Transaction) -> anyhow::Result<()> {
        match self.ledger.apply_transaction(&tx)? {
            ApplyResult::Accepted => {
                info!("TX {} accepted locally, broadcasting", tx.id);
                let inner = bincode::serialize(&CoinPayload::Transaction(tx))?;
                self.transport.broadcast_coin_inner(inner).await?;
                Ok(())
            }
            result => Err(anyhow::anyhow!("TX rejected: {result:?}")),
        }
    }

    pub async fn handle_incoming(
        &self,
        payload: CoinPayload,
        from_peer: &str,
    ) -> anyhow::Result<()> {
        match payload {
            CoinPayload::Transaction(tx) => match self.ledger.apply_transaction(&tx)? {
                ApplyResult::Accepted => {
                    info!("TX {} received and accepted", tx.id);
                    let _ = self
                        .ledger
                        .add_witness(&tx.id, self.local_peer_id.clone())?;
                    let witness = CoinPayload::Witness {
                        tx_id: tx.id.clone(),
                        peer_id: self.local_peer_id.clone(),
                    };
                    let inner = bincode::serialize(&witness)?;
                    self.transport.broadcast_coin_inner(inner).await?;
                }
                ApplyResult::Duplicate => {
                    if self
                        .ledger
                        .add_witness(&tx.id, self.local_peer_id.clone())?
                    {
                        let witness = CoinPayload::Witness {
                            tx_id: tx.id.clone(),
                            peer_id: self.local_peer_id.clone(),
                        };
                        let inner = bincode::serialize(&witness)?;
                        self.transport.broadcast_coin_inner(inner).await?;
                    }
                }
                result => {
                    warn!("TX from {from_peer} rejected: {result:?}");
                }
            },
            CoinPayload::Witness { tx_id, peer_id } => {
                let _ = self.ledger.add_witness(&tx_id, peer_id)?;
            }
            CoinPayload::BalanceQuery { address } => {
                let acc = self.ledger.account_state(&address)?;
                let response = CoinPayload::BalanceResponse {
                    address,
                    balance_muon: acc.balance_muon,
                    pending_muon: acc.pending_received,
                };
                let inner = bincode::serialize(&response)?;
                self.transport
                    .send_direct_coin_inner(from_peer.to_string(), inner)
                    .await?;
            }
            CoinPayload::BalanceResponse { .. } => {}
            CoinPayload::RefillRequest(req) => {
                if req.cold_address != self.local_address {
                    return Ok(());
                }
                if mycelium_core::data::now_ms() > req.expires_at_ms {
                    warn!("refill request expired from {from_peer}");
                    return Ok(());
                }
                if !req.verify() {
                    warn!("invalid refill request from {from_peer}");
                    return Ok(());
                }
                if self.ledger.refill_already_applied(&req.request_id)? {
                    return Ok(());
                }
                match self
                    .submit_transfer_from_identity_path(
                        &self.coin_identity_path,
                        req.hot_address.clone(),
                        req.amount_muon,
                        req.fee_muon,
                        Some("refill".into()),
                    )
                    .await
                {
                    Ok(()) => {
                        if let Err(e) = self.ledger.mark_refill_applied(&req.request_id) {
                            warn!(
                                "failed to record refill request_id {}: {e:#}",
                                req.request_id
                            );
                        }
                    }
                    Err(e) => {
                        warn!("refill transfer failed: {e:#}");
                    }
                }
            }
        }
        Ok(())
    }

    pub fn balance(&self) -> anyhow::Result<(u64, u64)> {
        let acc = self.ledger.account_state(&self.local_address)?;
        Ok((acc.balance_muon, acc.pending_received))
    }

    pub fn recent_transactions(&self, limit: usize) -> anyhow::Result<Vec<Transaction>> {
        self.ledger.recent_transactions(limit)
    }

    /// Testnet helper: credit this node once if it has no funds.
    pub fn request_faucet_coins(&self) -> anyhow::Result<()> {
        let acc = self.ledger.account_state(&self.local_address)?;
        if acc.balance_muon == 0 && acc.pending_received == 0 {
            self.ledger
                .genesis_credit(&self.local_address, 10 * crate::ledger::MUON_PER_MXC)?;
        }
        Ok(())
    }
}
