use crate::address::{address_from_public_key, validate_address};
use crate::settlement_policy::SettlementPolicy;
use crate::velocity::VelocityTracker;
use libp2p::identity::PublicKey;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::HashSet;

pub type Address = String;
pub type TxId = String;

/// Coin unit: 1 MXC = 1_000_000 Muon (like Satoshi).
pub const MUON_PER_MXC: u64 = 1_000_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transaction {
    pub id: TxId,
    pub from: Address,
    pub to: Address,
    pub amount_muon: u64,
    pub fee_muon: u64,
    pub timestamp_ms: u64,
    pub nonce: u64,
    pub memo: Option<String>,
    /// Ed25519 public key (protobuf-encoded) matching `from`.
    pub from_public_key: Vec<u8>,
    pub signature: Vec<u8>,
    pub witnesses: HashSet<String>,
}

impl Transaction {
    pub fn new(
        to: Address,
        amount_muon: u64,
        fee_muon: u64,
        nonce: u64,
        memo: Option<String>,
        keypair: &libp2p::identity::Keypair,
    ) -> anyhow::Result<Self> {
        let from = address_from_keypair(keypair);
        let from_public_key = keypair.public().encode_protobuf();
        let timestamp_ms = mycelium_core::data::now_ms();
        let mut tx = Self {
            id: String::new(),
            from,
            to,
            amount_muon,
            fee_muon,
            timestamp_ms,
            nonce,
            memo,
            from_public_key,
            signature: Vec::new(),
            witnesses: HashSet::new(),
        };
        let bytes = tx.bytes_for_signing();
        tx.signature = keypair
            .sign(&bytes)
            .map_err(|e| anyhow::anyhow!("signing failed: {e}"))?;
        let mut id_input = bytes;
        id_input.extend_from_slice(&tx.signature);
        tx.id = blake3::hash(&id_input).to_hex().to_string();
        Ok(tx)
    }

    pub fn verify(&self) -> bool {
        if !validate_address(&self.from) || !validate_address(&self.to) {
            return false;
        }
        let Ok(pk) = PublicKey::try_decode_protobuf(&self.from_public_key) else {
            return false;
        };
        if address_from_public_key(&pk) != self.from {
            return false;
        }
        let bytes = self.bytes_for_signing();
        pk.verify(&bytes, &self.signature)
    }

    pub fn add_witness(&mut self, peer_id: String) {
        self.witnesses.insert(peer_id);
    }

    pub fn is_confirmed(&self) -> bool {
        self.witnesses.len() >= 3
    }

    fn bytes_for_signing(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.from.as_bytes());
        bytes.extend_from_slice(self.to.as_bytes());
        bytes.extend_from_slice(&self.amount_muon.to_le_bytes());
        bytes.extend_from_slice(&self.fee_muon.to_le_bytes());
        bytes.extend_from_slice(&self.timestamp_ms.to_le_bytes());
        bytes.extend_from_slice(&self.nonce.to_le_bytes());
        bytes.extend_from_slice(&self.from_public_key);
        if let Some(memo) = &self.memo {
            bytes.extend_from_slice(memo.as_bytes());
        }
        bytes
    }
}

fn address_from_keypair(keypair: &libp2p::identity::Keypair) -> String {
    crate::address::address_from_keypair(keypair)
}

/// UTXO-like model simplified: account balance tracking.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AccountState {
    pub address: Address,
    pub balance_muon: u64,
    pub next_nonce: u64,
    pub pending_received: u64,
}

/// Local ledger — no global state, only what this node has seen.
#[derive(Debug, Clone)]
pub struct LocalLedger {
    db: sled::Db,
    policy: SettlementPolicy,
    velocity: VelocityTracker,
}

impl LocalLedger {
    pub fn open(path: &str) -> anyhow::Result<Self> {
        let db = sled::Config::new()
            .path(path)
            .flush_every_ms(Some(5000))
            .open()?;
        let velocity = VelocityTracker::open(&db)?;
        Ok(Self {
            db,
            policy: SettlementPolicy::beta_defaults(),
            velocity,
        })
    }

    pub fn policy(&self) -> &SettlementPolicy {
        &self.policy
    }

    pub fn set_policy(&mut self, policy: SettlementPolicy) {
        self.policy = policy;
    }

    /// Store a TX. Returns `Err` on I/O; business rules as [`ApplyResult`].
    pub fn apply_transaction(&self, tx: &Transaction) -> anyhow::Result<ApplyResult> {
        if self.transaction_tree()?.contains_key(tx.id.as_bytes())? {
            return Ok(ApplyResult::Duplicate);
        }
        if !tx.verify() {
            return Ok(ApplyResult::Invalid("invalid signature".into()));
        }
        if tx.from == tx.to {
            return Ok(ApplyResult::Invalid("from and to must differ".into()));
        }
        let account = self.account_state(&tx.from)?;
        if tx.nonce != account.next_nonce {
            return Ok(ApplyResult::InvalidNonce {
                expected: account.next_nonce,
                got: tx.nonce,
            });
        }
        let total = tx
            .amount_muon
            .checked_add(tx.fee_muon)
            .ok_or_else(|| anyhow::anyhow!("amount overflow"))?;
        if account.balance_muon < total {
            return Ok(ApplyResult::InsufficientFunds {
                have: account.balance_muon,
                need: total,
            });
        }
        if tx.memo.as_ref().map_or(0, |m| m.len()) > 64 {
            return Ok(ApplyResult::Invalid("memo too long".into()));
        }

        if let Err(msg) = self.policy.check_transfer_amount(tx.amount_muon) {
            return Ok(ApplyResult::Invalid(msg));
        }

        let encoded = bincode::serialize(tx)?;
        if let Err(msg) = self.policy.check_tx_size(tx, encoded.len()) {
            return Ok(ApplyResult::Invalid(msg));
        }

        let to_acc = self.account_state(&tx.to)?;
        if let Err(msg) = self.policy.check_balance_cap(
            to_acc.balance_muon,
            to_acc.pending_received,
            tx.amount_muon,
        ) {
            return Ok(ApplyResult::Invalid(msg));
        }

        let recent = self.velocity.recent_for(&tx.from)?;
        if let Err(msg) = self
            .policy
            .check_velocity(&recent, tx.timestamp_ms, tx.amount_muon)
        {
            return Ok(ApplyResult::Invalid(msg));
        }
        self.transaction_tree()?.insert(tx.id.as_bytes(), encoded)?;

        self.update_account(&tx.from, |acc| {
            acc.balance_muon -= total;
            acc.next_nonce += 1;
        })?;
        self.update_account(&tx.to, |acc| {
            if tx.is_confirmed() {
                acc.balance_muon += tx.amount_muon;
            } else {
                acc.pending_received += tx.amount_muon;
            }
        })?;

        self.velocity
            .record_outbound(&tx.from, tx.timestamp_ms, tx.amount_muon)?;

        Ok(ApplyResult::Accepted)
    }

    /// Peer has seen this TX — add witness. Returns `Ok(true)` if the TX row was updated.
    pub fn add_witness(&self, tx_id: &str, peer_id: String) -> anyhow::Result<bool> {
        let tree = self.transaction_tree()?;
        let Some(bytes) = tree.get(tx_id.as_bytes())? else {
            return Ok(false);
        };
        let mut tx: Transaction = bincode::deserialize(&bytes)?;
        let was_unconfirmed = !tx.is_confirmed();
        let inserted = tx.witnesses.insert(peer_id);
        if !inserted {
            return Ok(false);
        }

        if was_unconfirmed && tx.is_confirmed() {
            let to = tx.to.clone();
            let amt = tx.amount_muon;
            let to_acc = self.account_state(&to)?;
            if let Err(msg) = self.policy.check_balance_cap(
                to_acc.balance_muon,
                to_acc.pending_received.saturating_sub(amt),
                amt,
            ) {
                return Err(anyhow::anyhow!(msg));
            }
            self.update_account(&to, |acc| {
                acc.pending_received = acc.pending_received.saturating_sub(amt);
                acc.balance_muon += amt;
            })?;
        }

        let encoded = bincode::serialize(&tx)?;
        tree.insert(tx_id.as_bytes(), encoded)?;
        Ok(true)
    }

    pub fn account_state(&self, address: &str) -> anyhow::Result<AccountState> {
        let tree = self.db.open_tree("accounts")?;
        match tree.get(address.as_bytes())? {
            Some(bytes) => Ok(bincode::deserialize(&bytes)?),
            None => Ok(AccountState {
                address: address.to_string(),
                ..Default::default()
            }),
        }
    }

    pub fn transaction(&self, id: &str) -> anyhow::Result<Option<Transaction>> {
        match self.transaction_tree()?.get(id.as_bytes())? {
            Some(bytes) => Ok(Some(bincode::deserialize(&bytes)?)),
            None => Ok(None),
        }
    }

    pub fn recent_transactions(&self, limit: usize) -> anyhow::Result<Vec<Transaction>> {
        let tree = self.transaction_tree()?;
        let mut txs: Vec<Transaction> = Vec::new();
        for item in tree.iter() {
            let (_, bytes) = item?;
            txs.push(bincode::deserialize(&bytes)?);
        }
        txs.sort_by_key(|tx| Reverse(tx.timestamp_ms));
        txs.truncate(limit);
        Ok(txs)
    }

    /// Genesis: give a new account initial credits (preview / emergency onboarding only).
    pub fn genesis_credit(&self, address: &str, amount_muon: u64) -> anyhow::Result<()> {
        let acc = self.account_state(address)?;
        self.policy
            .check_balance_cap(acc.balance_muon, acc.pending_received, amount_muon)
            .map_err(|e| anyhow::anyhow!(e))?;
        self.update_account(address, |acc| {
            acc.balance_muon += amount_muon;
        })
    }

    fn transaction_tree(&self) -> anyhow::Result<sled::Tree> {
        Ok(self.db.open_tree("transactions")?)
    }

    fn refill_applied_tree(&self) -> anyhow::Result<sled::Tree> {
        Ok(self.db.open_tree("refill_applied")?)
    }

    pub fn refill_already_applied(&self, request_id: &str) -> anyhow::Result<bool> {
        Ok(self
            .refill_applied_tree()?
            .contains_key(request_id.as_bytes())?)
    }

    pub fn mark_refill_applied(&self, request_id: &str) -> anyhow::Result<()> {
        self.refill_applied_tree()?
            .insert(request_id.as_bytes(), &[1u8])?;
        Ok(())
    }

    fn update_account(
        &self,
        address: &str,
        f: impl FnOnce(&mut AccountState),
    ) -> anyhow::Result<()> {
        let tree = self.db.open_tree("accounts")?;
        let mut acc = self.account_state(address)?;
        f(&mut acc);
        tree.insert(address.as_bytes(), bincode::serialize(&acc)?)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyResult {
    Accepted,
    Duplicate,
    InvalidNonce { expected: u64, got: u64 },
    InsufficientFunds { have: u64, need: u64 },
    Invalid(String),
}
