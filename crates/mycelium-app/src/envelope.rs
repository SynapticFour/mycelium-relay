use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppMessage {
    pub app_id: AppId,
    pub payload: AppPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AppId {
    Chat,
    Bulletin,
    Mail,
    Coin,
    AppStore,
    MiniAppRevocation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AppPayload {
    Chat(ChatMessage),
    Bulletin(BulletinPost),
    Mail(MailMessage),
    /// `bincode` of [`mycelium_coin::CoinPayload`].
    Coin(Vec<u8>),
    /// Mini-app store listing propagated on scope `mycelium/appstore/v1`.
    AppStoreListing(Box<crate::miniapp::store::AppStoreListing>),
    /// Signed revocation on scope `mycelium/appstore/revocations/v1`.
    MiniAppRevocation(Box<crate::miniapp::revocation::RevocationGossip>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: Uuid,
    pub from_display_name: String,
    pub body: String,
    pub timestamp_ms: u64,
    pub reply_to: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulletinPost {
    pub id: Uuid,
    pub from_display_name: String,
    pub title: String,
    pub body: String,
    pub scope: String,
    pub timestamp_ms: u64,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailMessage {
    pub id: Uuid,
    pub from_peer: String,
    pub from_display_name: String,
    pub to_peer: String,
    pub subject: String,
    pub body: String,
    pub timestamp_ms: u64,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub filename: String,
    pub mime_type: String,
    pub data: Vec<u8>,
    pub size_bytes: usize,
}

impl AppMessage {
    pub fn encode(&self) -> anyhow::Result<Vec<u8>> {
        Ok(bincode::serialize(self)?)
    }

    pub fn decode(bytes: &[u8]) -> anyhow::Result<Self> {
        Ok(bincode::deserialize(bytes)?)
    }

    pub fn encode_coin_payload(coin_inner: &[u8]) -> anyhow::Result<Vec<u8>> {
        Self {
            app_id: AppId::Coin,
            payload: AppPayload::Coin(coin_inner.to_vec()),
        }
        .encode()
    }
}
