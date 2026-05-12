use crate::envelope::{
    AppId, AppMessage, AppPayload, Attachment, BulletinPost, ChatMessage, MailMessage,
};
use crate::notify::NotificationSink;
use crate::storage::AppStorage;
use mycelium_coin::CoinNode;
use mycelium_core::transport::DirectMessage;
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
    storage: Arc<AppStorage>,
    notifier: Arc<dyn NotificationSink>,
    coin: Option<Arc<CoinNode>>,
}

impl AppNode {
    pub fn new(
        handle: NodeHandle,
        local_peer_id: String,
        display_name: String,
        storage: Arc<AppStorage>,
        notifier: Arc<dyn NotificationSink>,
        coin: Option<Arc<CoinNode>>,
    ) -> (Self, AppInbox) {
        let (chat_tx, chat_rx) = broadcast::channel(256);
        let (bulletin_tx, bulletin_rx) = broadcast::channel(256);
        let (mail_tx, mail_rx) = broadcast::channel(256);
        let node = Self {
            handle,
            local_peer_id,
            display_name: Arc::new(RwLock::new(display_name)),
            chat_tx,
            bulletin_tx,
            mail_tx,
            storage,
            notifier,
            coin,
        };
        let inbox = AppInbox {
            chat_rx,
            bulletin_rx,
            mail_rx,
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
        match AppMessage::decode(&msg.envelope.payload) {
            Ok(app_msg) => match app_msg.payload {
                AppPayload::Chat(m) => {
                    let peer = msg.envelope.from_peer.clone();
                    if let Err(err) = self.storage.save_chat(&peer, &m) {
                        warn!("failed to save chat: {err}");
                    }
                    self.notifier
                        .on_chat_received(&peer, &m.body.chars().take(40).collect::<String>());
                    let _ = self.chat_tx.send(m);
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
                    self.notifier.on_mail_received(&m.from_display_name, &m.subject);
                    let _ = self.mail_tx.send(m);
                }
                AppPayload::Coin(inner) => {
                    if let Some(coin) = &self.coin {
                        let from_peer = msg.envelope.from_peer.clone();
                        match bincode::deserialize::<mycelium_coin::CoinPayload>(&inner) {
                            Ok(payload) => {
                                if let Err(err) =
                                    coin.handle_incoming(payload, &from_peer).await
                                {
                                    warn!("coin dispatch failed: {err}");
                                }
                            }
                            Err(err) => warn!("failed to decode CoinPayload: {err}"),
                        }
                    }
                }
            },
            Err(err) => {
                warn!("failed to decode AppMessage: {err}");
            }
        }
    }

    pub async fn send_chat(&self, to_peer: Option<String>, body: String) -> anyhow::Result<()> {
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
            payload: AppPayload::Chat(chat.clone()),
        }
        .encode()?;
        if let Some(to) = to_peer {
            self.storage.save_chat(&to, &chat)?;
            self.handle
                .send(NodeCommand::SendDirectPayload {
                    to_peer: to,
                    body,
                    payload,
                })
                .await?;
        } else {
            self.handle
                .send(NodeCommand::BroadcastPayload {
                    scope: "mycelium/chat".to_string(),
                    body,
                    payload,
                })
                .await?;
        }
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

    pub fn bulletins_for_scope(&self, scope: &str) -> anyhow::Result<Vec<BulletinPost>> {
        self.storage.bulletins_for_scope(scope)
    }

    pub fn mail_inbox(&self, limit: usize) -> anyhow::Result<Vec<MailMessage>> {
        self.storage.inbox(limit)
    }

    pub fn mail_sent(&self, limit: usize) -> anyhow::Result<Vec<MailMessage>> {
        self.storage.sent(limit)
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
}
