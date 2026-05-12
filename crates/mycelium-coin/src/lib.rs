//! Offline-first MeshCoin ledger and mesh propagation hooks.

mod address;
mod hot_wallet;
mod ledger;
mod node;
mod payload;
mod payment;
mod refill;

pub use address::{address_from_keypair, validate_address};
pub use hot_wallet::{HotWallet, HotWalletConfig, RefillResult};
pub use ledger::{
    AccountState, Address, ApplyResult, LocalLedger, Transaction, TxId, MUON_PER_MXC,
};
pub use node::{CoinNode, CoinTransport};
pub use payload::CoinPayload;
pub use payment::PaymentRequest;
pub use refill::{RefillRequest, REFILL_REQUEST_TTL_MS};

#[cfg(test)]
mod tests {
    use super::hot_wallet::{HotWallet, HotWalletConfig};
    use super::ledger::{ApplyResult, LocalLedger, Transaction, MUON_PER_MXC};
    use super::payload::CoinPayload;
    use super::payment::PaymentRequest;
    use super::refill::RefillRequest;
    use super::{address_from_keypair, validate_address, CoinNode, CoinTransport};
    use async_trait::async_trait;
    use libp2p::identity::Keypair;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    fn write_identity_store(path: &Path, kp: &Keypair) {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        let db = sled::open(path).unwrap();
        let tree = db.open_tree("identity").unwrap();
        let ed = kp.clone().try_into_ed25519().unwrap();
        tree.insert(b"ed25519_secret_key", ed.secret().as_ref())
            .unwrap();
        tree.flush().unwrap();
        db.flush().unwrap();
    }

    #[derive(Clone, Default)]
    struct NoopTransport;

    #[async_trait]
    impl CoinTransport for NoopTransport {
        async fn broadcast_coin_inner(&self, _inner: Vec<u8>) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send_direct_coin_inner(
            &self,
            _to_peer: String,
            _inner: Vec<u8>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn basic_transfer() {
        let dir = tempdir().unwrap();
        let ledger = LocalLedger::open(dir.path().to_str().unwrap()).unwrap();
        let keypair = Keypair::generate_ed25519();
        let alice = address_from_keypair(&keypair);
        let bob = address_from_keypair(&Keypair::generate_ed25519());
        assert!(validate_address(&bob));
        ledger.genesis_credit(&alice, 10 * MUON_PER_MXC).unwrap();
        let tx = Transaction::new(bob, 5 * MUON_PER_MXC, 1000, 0, None, &keypair).unwrap();
        assert_eq!(
            ledger.apply_transaction(&tx).unwrap(),
            ApplyResult::Accepted
        );
        let acc = ledger.account_state(&alice).unwrap();
        assert_eq!(acc.balance_muon, 5 * MUON_PER_MXC - 1000);
    }

    #[test]
    fn double_spend_rejected() {
        let dir = tempdir().unwrap();
        let ledger = LocalLedger::open(dir.path().to_str().unwrap()).unwrap();
        let keypair = Keypair::generate_ed25519();
        let alice = address_from_keypair(&keypair);
        let bob = address_from_keypair(&Keypair::generate_ed25519());
        let carol = address_from_keypair(&Keypair::generate_ed25519());
        ledger.genesis_credit(&alice, 10 * MUON_PER_MXC).unwrap();
        let tx1 = Transaction::new(bob, 8 * MUON_PER_MXC, 1000, 0, None, &keypair).unwrap();
        let tx2 = Transaction::new(carol, 8 * MUON_PER_MXC, 1000, 0, None, &keypair).unwrap();
        assert_eq!(
            ledger.apply_transaction(&tx1).unwrap(),
            ApplyResult::Accepted
        );
        assert!(matches!(
            ledger.apply_transaction(&tx2).unwrap(),
            ApplyResult::InvalidNonce { .. }
        ));
    }

    #[test]
    fn witness_confirmation() {
        let dir = tempdir().unwrap();
        let ledger = LocalLedger::open(dir.path().to_str().unwrap()).unwrap();
        let keypair = Keypair::generate_ed25519();
        let alice = address_from_keypair(&keypair);
        let bob_addr = address_from_keypair(&Keypair::generate_ed25519());
        ledger.genesis_credit(&alice, 10 * MUON_PER_MXC).unwrap();
        let tx =
            Transaction::new(bob_addr.clone(), 5 * MUON_PER_MXC, 1000, 0, None, &keypair).unwrap();
        assert_eq!(
            ledger.apply_transaction(&tx).unwrap(),
            ApplyResult::Accepted
        );
        let bob = ledger.account_state(&bob_addr).unwrap();
        assert_eq!(bob.balance_muon, 0);
        assert_eq!(bob.pending_received, 5 * MUON_PER_MXC);
        ledger.add_witness(&tx.id, "peer1".into()).unwrap();
        ledger.add_witness(&tx.id, "peer2".into()).unwrap();
        ledger.add_witness(&tx.id, "peer3".into()).unwrap();
        let bob = ledger.account_state(&bob_addr).unwrap();
        assert_eq!(bob.balance_muon, 5 * MUON_PER_MXC);
        assert_eq!(bob.pending_received, 0);
    }

    #[test]
    fn address_generation_and_validation() {
        let keypair = Keypair::generate_ed25519();
        let addr = address_from_keypair(&keypair);
        assert!(addr.starts_with("mxc1"));
        assert!(validate_address(&addr));
        assert!(!validate_address("mxc1invalid"));
    }

    #[test]
    fn payment_request_roundtrip() {
        let req = PaymentRequest::new("mxc1abc123".into(), 5_000_000, Some("Kaffee".into()));
        let uri = req.to_uri();
        assert!(uri.starts_with("mxcpay:mxc1abc123?amount=5000000"));
        let parsed = PaymentRequest::from_uri(&uri).unwrap();
        assert_eq!(parsed.to_address, "mxc1abc123");
        assert_eq!(parsed.amount_muon, 5_000_000);
        assert_eq!(parsed.memo.as_deref(), Some("Kaffee"));
    }

    #[test]
    fn payment_request_expires() {
        let mut req = PaymentRequest::new("mxc1abc".into(), 1000, None);
        req.expires_at_ms = Some(1);
        assert!(req.is_expired());
    }

    #[test]
    fn refill_request_sign_and_verify() {
        let hot = Keypair::generate_ed25519();
        let cold_addr = address_from_keypair(&Keypair::generate_ed25519());
        let req = RefillRequest::new_signed(&hot, cold_addr, 5_000_000, 1000).unwrap();
        assert!(req.verify());
    }

    #[test]
    fn refill_request_rejects_tampering() {
        let hot = Keypair::generate_ed25519();
        let cold_addr = address_from_keypair(&Keypair::generate_ed25519());
        let mut req = RefillRequest::new_signed(&hot, cold_addr, 5_000_000, 1000).unwrap();
        req.amount_muon += 1;
        assert!(!req.verify());
    }

    #[tokio::test]
    async fn cold_accepts_signed_refill_once() {
        let dir = tempdir().unwrap();
        let ledger_path = dir.path().join("ledger");
        let cold_id = dir.path().join("cold_identity");
        let cold_ledger = Arc::new(LocalLedger::open(ledger_path.to_str().unwrap()).unwrap());
        let cold_kp = Keypair::generate_ed25519();
        let cold_addr = address_from_keypair(&cold_kp);
        let hot_kp = Keypair::generate_ed25519();
        let hot_addr = address_from_keypair(&hot_kp);
        write_identity_store(&cold_id, &cold_kp);
        cold_ledger
            .genesis_credit(&cold_addr, 100 * MUON_PER_MXC)
            .unwrap();

        #[derive(Clone)]
        struct Capture(Arc<Mutex<Vec<Vec<u8>>>>);

        #[async_trait]
        impl CoinTransport for Capture {
            async fn broadcast_coin_inner(&self, inner: Vec<u8>) -> anyhow::Result<()> {
                self.0.lock().unwrap().push(inner);
                Ok(())
            }

            async fn send_direct_coin_inner(
                &self,
                _to_peer: String,
                _inner: Vec<u8>,
            ) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let outs = Arc::new(Mutex::new(Vec::new()));
        let cold_node = CoinNode::new(
            cold_ledger.clone(),
            Arc::new(Capture(outs.clone())),
            cold_addr.clone(),
            "cold-peer".into(),
            cold_id.to_str().unwrap().to_string(),
        );
        let req =
            RefillRequest::new_signed(&hot_kp, cold_addr.clone(), 10 * MUON_PER_MXC, 1000).unwrap();
        let payload = CoinPayload::RefillRequest(req.clone());
        cold_node
            .handle_incoming(payload.clone(), "p99")
            .await
            .unwrap();
        cold_node.handle_incoming(payload, "p99").await.unwrap();

        assert_eq!(outs.lock().unwrap().len(), 1);
        let hot_acc = cold_ledger.account_state(&hot_addr).unwrap();
        assert_eq!(hot_acc.pending_received, 10 * MUON_PER_MXC);
        let spent = 10 * MUON_PER_MXC + 1000;
        let cold_acc = cold_ledger.account_state(&cold_addr).unwrap();
        assert_eq!(cold_acc.balance_muon, 100 * MUON_PER_MXC - spent);
    }

    #[tokio::test]
    async fn hot_wallet_enforce_cap_returns_excess() {
        let base =
            std::env::temp_dir().join(format!("mycelium-hot-wallet-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        let ledger_path = base.join("ledger");
        let id_path = base.join("identity");
        let ledger = Arc::new(LocalLedger::open(ledger_path.to_str().unwrap()).unwrap());
        let hot_kp = Keypair::generate_ed25519();
        let hot_addr = address_from_keypair(&hot_kp);
        let cold_addr = address_from_keypair(&Keypair::generate_ed25519());
        ledger
            .genesis_credit(&hot_addr, 100 * MUON_PER_MXC)
            .unwrap();

        let id_store = id_path.to_str().unwrap();
        write_identity_store(Path::new(id_store), &hot_kp);

        let coin = Arc::new(CoinNode::new(
            ledger.clone(),
            Arc::new(NoopTransport),
            hot_addr.clone(),
            "local-peer".into(),
            id_store.to_string(),
        ));

        let max = 50 * MUON_PER_MXC;
        let cfg = HotWalletConfig {
            max_cache_muon: max,
            refill_threshold_muon: max * 80 / 100,
            refill_amount_muon: max,
            cold_wallet_address: Some(cold_addr.clone()),
        };
        let hw = HotWallet::new(cfg, coin.clone(), id_store.to_string());
        hw.enforce_cap().await.unwrap();

        let hot_bal = ledger.account_state(&hot_addr).unwrap().balance_muon;
        let cold_acc = ledger.account_state(&cold_addr).unwrap();
        let cold_credited = cold_acc.balance_muon + cold_acc.pending_received;
        assert!(hot_bal <= max);
        assert!(cold_credited >= 49 * MUON_PER_MXC);
    }
}
