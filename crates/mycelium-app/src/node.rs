use crate::contacts::{Contact, ContactStatus};
use crate::envelope::{
    AppId, AppMessage, AppPayload, Attachment, BulletinPost, ChatMessage, MailMessage,
};
use crate::groups::Group;
use crate::miniapp::store::AppStoreListing;
use crate::notify::NotificationSink;
use crate::storage::AppStorage;
use mycelium_coin::CoinNode;
use mycelium_core::transport::{DirectMessage, WireMessage};
use mycelium_node::{NodeCommand, NodeHandle};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::warn;

pub struct AppNode {
    handle: NodeHandle,
    local_peer_id: String,
    display_name: Arc<RwLock<String>>,
    chat_tx: broadcast::Sender<ChatMessage>,
    bulletin_tx: broadcast::Sender<BulletinPost>,
    mail_tx: broadcast::Sender<MailMessage>,
    appstore_tx: broadcast::Sender<AppStoreListing>,
    storage: Arc<AppStorage>,
    notifier: Arc<dyn NotificationSink>,
    coin: Option<Arc<CoinNode>>,
    app_store: Option<Arc<crate::miniapp::AppStore>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionResult {
    Encrypted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatSendResult {
    SentEncrypted,
    SentPlaintext,
}

fn enc_key_not_yet_exchanged_err() -> anyhow::Error {
    anyhow::anyhow!(
        "enc_key_not_yet_exchanged: message not sent. \
         Connect to the peer first and wait for key exchange."
    )
}

impl AppNode {
    pub fn new(
        handle: NodeHandle,
        local_peer_id: String,
        display_name: String,
        storage: Arc<AppStorage>,
        notifier: Arc<dyn NotificationSink>,
        coin: Option<Arc<CoinNode>>,
        app_store: Option<Arc<crate::miniapp::AppStore>>,
    ) -> (Self, AppInbox) {
        let (chat_tx, chat_rx) = broadcast::channel(256);
        let (bulletin_tx, bulletin_rx) = broadcast::channel(256);
        let (mail_tx, mail_rx) = broadcast::channel(256);
        let (appstore_tx, appstore_rx) = broadcast::channel(64);
        let node = Self {
            handle,
            local_peer_id,
            display_name: Arc::new(RwLock::new(display_name)),
            chat_tx,
            bulletin_tx,
            mail_tx,
            appstore_tx,
            storage,
            notifier,
            coin,
            app_store,
        };
        let inbox = AppInbox {
            chat_rx,
            bulletin_rx,
            mail_rx,
            appstore_rx,
        };
        (node, inbox)
    }

    pub fn start_incoming_task(self: Arc<Self>) {
        let mut rx = self.handle.subscribe_incoming();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(msg) => self.dispatch_incoming(&msg).await,
                    Err(err) => {
                        warn!("incoming subscription closed: {err}");
                        break;
                    }
                }
            }
        });
    }

    pub async fn dispatch_incoming(&self, msg: &DirectMessage) {
        if msg.body == "[mycelium:group]" {
            if let Ok(WireMessage::EncryptedGroup {
                group_id,
                encrypted_payload,
            }) = bincode::deserialize(&msg.envelope.payload)
            {
                let from = msg.envelope.from_peer.clone();
                match self.storage.group_by_id(&group_id) {
                    Ok(Some(group)) => {
                        match mycelium_core::crypto::decrypt_group(&encrypted_payload, &group.key) {
                            Ok(plain) => {
                                if let Ok(app_msg) = AppMessage::decode(&plain) {
                                    if let AppPayload::Chat(m) = app_msg.payload {
                                        if let Err(err) =
                                            self.storage.save_group_chat(&group_id, &m)
                                        {
                                            warn!("failed to save group chat: {err}");
                                        }
                                        if let Err(err) =
                                            self.storage.group_record_peer_seen(&group_id, &from)
                                        {
                                            warn!("group member record failed: {err}");
                                        }
                                        self.notifier.on_chat_received(
                                            &from,
                                            &m.body.chars().take(40).collect::<String>(),
                                        );
                                        let _ = self.chat_tx.send(m);
                                    }
                                }
                            }
                            Err(e) => warn!("group decrypt failed: {e}"),
                        }
                    }
                    Ok(None) => {}
                    Err(e) => warn!("group lookup failed: {e}"),
                }
            }
            return;
        }
        match AppMessage::decode(&msg.envelope.payload) {
            Ok(app_msg) => match app_msg.payload {
                AppPayload::Chat(m) => {
                    let peer = msg.envelope.from_peer.clone();
                    if peer == self.local_peer_id {
                        return;
                    }
                    let accepted = match self.storage.contact_by_id(&peer) {
                        Ok(Some(c)) if c.status == ContactStatus::Accepted => true,
                        Ok(Some(Contact {
                            status: ContactStatus::Pending,
                            display_name,
                            ..
                        })) => {
                            self.notifier
                                .on_contact_request(&peer, display_name.as_str());
                            false
                        }
                        Ok(None) => {
                            let name = if m.from_display_name.is_empty() {
                                peer.chars().take(12).collect::<String>()
                            } else {
                                m.from_display_name.clone()
                            };
                            if let Err(e) =
                                self.storage
                                    .upsert_contact(&peer, &name, ContactStatus::Pending)
                            {
                                warn!("failed to save pending contact: {e}");
                            }
                            self.notifier.on_contact_request(&peer, &name);
                            false
                        }
                        Err(e) => {
                            warn!("contact lookup failed: {e}");
                            false
                        }
                        Ok(Some(_)) => false,
                    };
                    if let Err(err) = self.storage.save_chat(&peer, &m) {
                        warn!("failed to save chat: {err}");
                    }
                    if accepted {
                        self.notifier
                            .on_chat_received(&peer, &m.body.chars().take(40).collect::<String>());
                        let _ = self.chat_tx.send(m);
                    }
                }
                AppPayload::Bulletin(m) => {
                    if let Err(err) = self.storage.save_bulletin(&m) {
                        warn!("failed to save bulletin: {err}");
                    }
                    self.notifier.on_bulletin_posted(&m.scope, &m.title);
                    let _ = self.bulletin_tx.send(m);
                }
                AppPayload::Mail(m) => {
                    if let Err(err) = self.storage.save_mail_inbox(&m) {
                        warn!("failed to save inbox mail: {err}");
                    }
                    self.notifier
                        .on_mail_received(&m.from_display_name, &m.subject);
                    let _ = self.mail_tx.send(m);
                }
                AppPayload::Coin(inner) => {
                    if let Some(coin) = &self.coin {
                        let from_peer = msg.envelope.from_peer.clone();
                        match bincode::deserialize::<mycelium_coin::CoinPayload>(&inner) {
                            Ok(payload) => {
                                if let Err(err) = coin.handle_incoming(payload, &from_peer).await {
                                    warn!("coin dispatch failed: {err}");
                                }
                            }
                            Err(err) => warn!("failed to decode CoinPayload: {err}"),
                        }
                    }
                }
                AppPayload::MiniAppRevocation(entry) => {
                    let entry = entry.as_ref();
                    if let Some(ref store) = self.app_store {
                        match store.ingest_revocation_gossip(entry) {
                            Ok(true) => tracing::info!("ingested revocation for {}", entry.app_id),
                            Ok(false) => {
                                warn!("rejected revocation gossip for {} (bad sig)", entry.app_id)
                            }
                            Err(e) => warn!("failed to ingest revocation {}: {e}", entry.app_id),
                        }
                    }
                }
                AppPayload::AppStoreListing(listing) => {
                    let listing = listing.as_ref();
                    match listing.verify_signature() {
                        Ok(true) => {
                            if let Some(ref store) = self.app_store {
                                match store.cache_listing(listing) {
                                    Err(e) => warn!(
                                        "failed to cache app listing {}: {e}",
                                        listing.manifest.id
                                    ),
                                    Ok(()) => {
                                        tracing::info!(
                                            "cached app listing: {} v{}",
                                            listing.manifest.id,
                                            listing.manifest.version
                                        );
                                        let _ = self.appstore_tx.send(listing.clone());
                                    }
                                }
                            }
                        }
                        Ok(false) => {
                            warn!(
                                "rejected listing with invalid signature: {}",
                                listing.manifest.id
                            );
                        }
                        Err(e) => warn!("signature verification error: {e}"),
                    }
                }
            },
            Err(err) => {
                if msg.body == "[encrypted]" {
                    warn!("failed to decode E2E direct chat payload: {err}");
                } else {
                    warn!("failed to decode AppMessage: {err}");
                }
            }
        }
    }

    pub async fn send_chat(
        &self,
        to_peer: Option<String>,
        body: String,
    ) -> anyhow::Result<ChatSendResult> {
        match to_peer {
            None => {
                let display_name = self.display_name.read().await.clone();
                let chat = ChatMessage {
                    id: uuid::Uuid::new_v4(),
                    from_display_name: display_name,
                    body: body.clone(),
                    timestamp_ms: mycelium_core::data::now_ms(),
                    reply_to: None,
                };
                let payload = AppMessage {
                    app_id: AppId::Chat,
                    payload: AppPayload::Chat(chat),
                }
                .encode()?;
                self.handle
                    .send(NodeCommand::BroadcastPayload {
                        scope: "mycelium/chat".to_string(),
                        body,
                        payload,
                    })
                    .await?;
                Ok(ChatSendResult::SentPlaintext)
            }
            Some(to) => {
                self.ensure_contact_accepted_for_send(&to, "")?;
                if self.handle.has_enc_key_for(&to).await {
                    self.send_chat_encrypted(to, body).await?;
                    Ok(ChatSendResult::SentEncrypted)
                } else {
                    warn!(
                        "SD-081: no enc key for {to} — refusing plaintext. \
                         Key will arrive via PeerInfo on next connection."
                    );
                    Err(enc_key_not_yet_exchanged_err())
                }
            }
        }
    }

    /// Broadcast a chat payload on an arbitrary mesh scope (e.g. `mycelium/chat`).
    pub async fn broadcast_chat(&self, scope: String, body: String) -> anyhow::Result<()> {
        let display_name = self.display_name.read().await.clone();
        let chat = ChatMessage {
            id: uuid::Uuid::new_v4(),
            from_display_name: display_name,
            body: body.clone(),
            timestamp_ms: mycelium_core::data::now_ms(),
            reply_to: None,
        };
        let payload = AppMessage {
            app_id: AppId::Chat,
            payload: AppPayload::Chat(chat),
        }
        .encode()?;
        self.handle
            .send(NodeCommand::BroadcastPayload {
                scope,
                body,
                payload,
            })
            .await?;
        Ok(())
    }

    pub async fn send_chat_encrypted(
        &self,
        to_peer: String,
        body: String,
    ) -> anyhow::Result<EncryptionResult> {
        self.ensure_contact_accepted_for_send(&to_peer, "")?;
        let display_name = self.display_name.read().await.clone();
        let chat = ChatMessage {
            id: uuid::Uuid::new_v4(),
            from_display_name: display_name,
            body: body.clone(),
            timestamp_ms: mycelium_core::data::now_ms(),
            reply_to: None,
        };
        let plaintext = AppMessage {
            app_id: AppId::Chat,
            payload: AppPayload::Chat(chat.clone()),
        }
        .encode()?;
        let wrapped = mycelium_core::e2e_direct_wrap::wrap_inner(&self.local_peer_id, &plaintext);
        if let Some(pk) = self.handle.peer_x25519_public(&to_peer).await {
            let encrypted = mycelium_core::crypto::encrypt_for(&wrapped, &pk)?;
            let wire = WireMessage::EncryptedDirect {
                to_peer: to_peer.clone(),
                sender_enc_pubkey: self.handle.local_enc_pubkey_hex(),
                encrypted_payload: encrypted,
                mesh_signature: None,
                hop_count: 0,
                max_hops: 8,
            };
            self.handle
                .send(NodeCommand::SendWire {
                    to_peer: to_peer.clone(),
                    message: wire,
                })
                .await?;
            self.storage.save_chat(&to_peer, &chat)?;
            return Ok(EncryptionResult::Encrypted);
        }
        Err(enc_key_not_yet_exchanged_err())
    }

    pub async fn send_group_message(&self, group: &Group, body: String) -> anyhow::Result<()> {
        let _ = self
            .storage
            .group_record_peer_seen(&group.id, &self.local_peer_id);
        let display_name = self.display_name.read().await.clone();
        let chat = ChatMessage {
            id: uuid::Uuid::new_v4(),
            from_display_name: display_name,
            body: body.clone(),
            timestamp_ms: mycelium_core::data::now_ms(),
            reply_to: None,
        };
        self.storage.save_group_chat(&group.id, &chat)?;
        let plaintext = AppMessage {
            app_id: AppId::Chat,
            payload: AppPayload::Chat(chat),
        }
        .encode()?;
        let encrypted = mycelium_core::crypto::encrypt_group(&plaintext, &group.key)?;
        let wire = WireMessage::EncryptedGroup {
            group_id: group.id.clone(),
            encrypted_payload: encrypted,
        };
        let payload = bincode::serialize(&wire)?;
        let scope = format!("mycelium/group/{}", group.id);
        let _ = self
            .handle
            .send(NodeCommand::SubscribeScope("mycelium/group/*".into()))
            .await;
        self.handle
            .send(NodeCommand::BroadcastPayload {
                scope,
                body: "[group message]".into(),
                payload,
            })
            .await?;
        Ok(())
    }

    pub async fn post_bulletin(
        &self,
        scope: String,
        title: String,
        body: String,
        ttl_secs: u64,
    ) -> anyhow::Result<()> {
        let display_name = self.display_name.read().await.clone();
        let now = mycelium_core::data::now_ms();
        let post = BulletinPost {
            id: uuid::Uuid::new_v4(),
            from_display_name: display_name,
            title,
            body: body.clone(),
            scope: scope.clone(),
            timestamp_ms: now,
            expires_at_ms: now + ttl_secs * 1000,
        };
        self.storage.save_bulletin(&post)?;
        let payload = AppMessage {
            app_id: AppId::Bulletin,
            payload: AppPayload::Bulletin(post),
        }
        .encode()?;
        self.handle
            .send(NodeCommand::BroadcastPayload {
                scope,
                body,
                payload,
            })
            .await?;
        Ok(())
    }

    pub async fn send_mail(
        &self,
        to_peer: String,
        subject: String,
        body: String,
        attachments: Vec<Attachment>,
    ) -> anyhow::Result<()> {
        let display_name = self.display_name.read().await.clone();
        let mail = MailMessage {
            id: uuid::Uuid::new_v4(),
            from_peer: self.local_peer_id.clone(),
            from_display_name: display_name,
            to_peer: to_peer.clone(),
            subject: subject.clone(),
            body: body.clone(),
            timestamp_ms: mycelium_core::data::now_ms(),
            attachments,
        };
        self.storage.save_mail_sent(&mail)?;
        let payload = AppMessage {
            app_id: AppId::Mail,
            payload: AppPayload::Mail(mail),
        }
        .encode()?;
        self.handle
            .send(NodeCommand::SendDirectPayload {
                to_peer,
                body: format!("mail:{subject}"),
                payload,
            })
            .await?;
        Ok(())
    }

    pub async fn set_display_name(&self, name: String) {
        *self.display_name.write().await = name;
    }

    pub fn local_peer_id(&self) -> &str {
        &self.local_peer_id
    }

    pub fn node_handle(&self) -> NodeHandle {
        self.handle.clone()
    }

    pub async fn display_name(&self) -> String {
        self.display_name.read().await.clone()
    }

    pub fn storage(&self) -> Arc<AppStorage> {
        self.storage.clone()
    }

    pub fn app_storage(&self) -> Arc<AppStorage> {
        self.storage.clone()
    }

    pub async fn chat_history(
        &self,
        peer_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<ChatMessage>> {
        self.storage.chat_history(peer_id, limit)
    }

    pub fn ensure_contact_accepted_for_send(
        &self,
        peer_id: &str,
        display_name: &str,
    ) -> anyhow::Result<()> {
        let name = if display_name.is_empty() {
            self.storage
                .contact_by_id(peer_id)?
                .map(|c| c.display_name)
                .unwrap_or_else(|| peer_id.chars().take(12).collect())
        } else {
            display_name.to_string()
        };
        self.storage
            .upsert_contact(peer_id, &name, ContactStatus::Accepted)?;
        Ok(())
    }

    pub fn list_contacts(&self) -> anyhow::Result<Vec<Contact>> {
        self.storage.all_contacts()
    }

    pub fn list_accepted_contacts(&self) -> anyhow::Result<Vec<Contact>> {
        self.storage.contacts_with_status(ContactStatus::Accepted)
    }

    pub fn list_pending_contacts(&self) -> anyhow::Result<Vec<Contact>> {
        self.storage.contacts_with_status(ContactStatus::Pending)
    }

    pub fn add_contact(
        &self,
        peer_id: &str,
        display_name: &str,
        accepted: bool,
    ) -> anyhow::Result<Contact> {
        let status = if accepted {
            ContactStatus::Accepted
        } else {
            ContactStatus::Pending
        };
        let name = if display_name.is_empty() {
            peer_id.to_string()
        } else {
            display_name.to_string()
        };
        self.storage.upsert_contact(peer_id, &name, status)
    }

    pub fn accept_contact(&self, peer_id: &str) -> anyhow::Result<Contact> {
        let existing = self.storage.contact_by_id(peer_id)?;
        let name = existing
            .map(|c| c.display_name)
            .unwrap_or_else(|| peer_id.to_string());
        self.storage
            .upsert_contact(peer_id, &name, ContactStatus::Accepted)
    }

    pub fn reject_contact(&self, peer_id: &str) -> anyhow::Result<()> {
        self.storage.delete_contact(peer_id)
    }

    pub fn remove_contact(&self, peer_id: &str) -> anyhow::Result<()> {
        self.storage.delete_contact(peer_id)
    }

    pub fn bulletins_for_scope(&self, scope: &str) -> anyhow::Result<Vec<BulletinPost>> {
        self.storage.bulletins_for_scope(scope)
    }

    pub fn mail_inbox(&self, limit: usize) -> anyhow::Result<Vec<MailMessage>> {
        self.storage.inbox(limit)
    }

    pub fn mail_sent(&self, limit: usize) -> anyhow::Result<Vec<MailMessage>> {
        self.storage.sent(limit)
    }

    /// Publishes a signed app listing on gossip scope `mycelium/appstore/v1`.
    pub async fn publish_app_listing(&self, listing: &AppStoreListing) -> anyhow::Result<()> {
        if !listing.verify_signature()? {
            anyhow::bail!("listing has invalid signature – sign with developer key first");
        }
        let payload = AppMessage {
            app_id: AppId::AppStore,
            payload: AppPayload::AppStoreListing(Box::new(listing.clone())),
        }
        .encode()?;
        self.handle
            .send(NodeCommand::BroadcastPayload {
                scope: "mycelium/appstore/v1".into(),
                body: format!("[app:{}]", listing.manifest.id),
                payload,
            })
            .await?;
        tracing::info!(
            "published listing: {} v{}",
            listing.manifest.id,
            listing.manifest.version
        );
        Ok(())
    }

    /// Publishes a signed revocation on gossip scope `mycelium/appstore/revocations/v1`.
    pub async fn publish_app_revocation(
        &self,
        entry: &crate::miniapp::revocation::RevocationGossip,
    ) -> anyhow::Result<()> {
        if !entry.verify_signature()? {
            anyhow::bail!("revocation has invalid signature");
        }
        let payload = AppMessage {
            app_id: AppId::MiniAppRevocation,
            payload: AppPayload::MiniAppRevocation(Box::new(entry.clone())),
        }
        .encode()?;
        self.handle
            .send(NodeCommand::BroadcastPayload {
                scope: "mycelium/appstore/revocations/v1".into(),
                body: format!("[revoke:{}]", entry.app_id),
                payload,
            })
            .await?;
        if let Some(ref store) = self.app_store {
            let _ = store.ingest_revocation_gossip(entry);
        }
        tracing::info!("published revocation for {}", entry.app_id);
        Ok(())
    }

    pub fn mark_mail_read(&self, mail_id: &uuid::Uuid) -> anyhow::Result<()> {
        self.storage.mark_read(mail_id)
    }

    pub fn is_mail_read(&self, mail_id: &uuid::Uuid) -> anyhow::Result<bool> {
        self.storage.is_read(mail_id)
    }
}

pub struct AppInbox {
    pub chat_rx: broadcast::Receiver<ChatMessage>,
    pub bulletin_rx: broadcast::Receiver<BulletinPost>,
    pub mail_rx: broadcast::Receiver<MailMessage>,
    pub appstore_rx: broadcast::Receiver<AppStoreListing>,
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;
    use crate::envelope::{AppId, AppMessage};
    use crate::miniapp::manifest::MiniAppManifest;
    use crate::miniapp::store::{AppSource, AppStore};
    use mycelium_core::data::Envelope;
    use mycelium_core::transport::DirectMessage;

    fn sample_manifest(peer_id: Option<String>) -> MiniAppManifest {
        MiniAppManifest {
            id: "com.test.app".into(),
            name: "Test".into(),
            description: "d".into(),
            version: "0.1.0".into(),
            developer: "T".into(),
            developer_peer_id: peer_id,
            entry: "index.html".into(),
            icon_base64: None,
            permissions: vec![],
            min_mycelium_version: "0.1.0".into(),
            accepts_payments: false,
            payment_address: None,
            categories: vec![],
            runtime: "webview".into(),
            bulletin_scopes: vec![],
            reproducible_build: None,
        }
    }

    #[tokio::test]
    async fn appstore_listing_cached_after_dispatch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path().to_str().expect("path");
        let store = Arc::new(AppStore::open(&format!("{base}/miniapp")).expect("store"));
        let storage = Arc::new(AppStorage::open(&format!("{base}/app")).expect("storage"));

        let storage_key = blake3::derive_key("mycelium-test-vault", base.as_bytes());
        let mut config = mycelium_node::NodeConfig::with_defaults(base);
        config.storage_key = Some(storage_key);
        let (runner, handle) = mycelium_node::NodeRunner::new(config).expect("node");
        tokio::spawn(async move {
            let _ = runner.run().await;
        });

        let keypair = mycelium_node::secrets::load_or_create_keypair(
            &format!("{base}/identity"),
            Some(storage_key),
        )
        .expect("identity");
        let peer_id = keypair.public().to_peer_id().to_string();
        let manifest = sample_manifest(Some(peer_id.clone()));
        let listing = AppStoreListing::new_signed(
            manifest,
            b"bundle-bytes",
            vec![AppSource::Peer(peer_id.clone())],
            &keypair,
        )
        .expect("signed listing");

        let (app_node, _) = AppNode::new(
            handle,
            peer_id,
            "test".into(),
            storage,
            Arc::new(crate::notify::NoopNotifier),
            None,
            Some(store.clone()),
        );
        let app_node = Arc::new(app_node);

        let app_msg = AppMessage {
            app_id: AppId::AppStore,
            payload: AppPayload::AppStoreListing(Box::new(listing.clone())),
        };
        let payload = app_msg.encode().expect("encode");
        let dm = DirectMessage {
            envelope: Envelope::new("remote-peer".into(), None, payload),
            body: "[appstore]".into(),
        };
        app_node.dispatch_incoming(&dm).await;

        let rows = store.browse_listings().expect("browse");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].manifest.id, listing.manifest.id);
        assert_eq!(rows[0].manifest.version, listing.manifest.version);
    }

    #[test]
    fn sd081_enc_key_error_is_actionable() {
        let err = enc_key_not_yet_exchanged_err();
        assert!(err.to_string().contains("enc_key_not_yet_exchanged"));
    }
}
