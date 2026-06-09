pub mod api;
pub mod contacts;
pub mod envelope;
pub mod groups;
pub mod miniapp;
mod miniapp_storage_enc;
pub mod node;
pub mod notify;
pub mod storage;

#[cfg(test)]
mod tests {
    use crate::envelope::{AppId, AppMessage, AppPayload, BulletinPost, ChatMessage, MailMessage};
    use crate::storage::AppStorage;

    #[tokio::test]
    async fn app_message_roundtrip() {
        let app = AppMessage {
            app_id: AppId::Chat,
            payload: AppPayload::Chat(ChatMessage {
                id: uuid::Uuid::new_v4(),
                from_display_name: "alice".to_string(),
                body: "hello".to_string(),
                timestamp_ms: 1,
                reply_to: None,
            }),
        };
        let encoded = app.encode().expect("encode");
        let decoded = AppMessage::decode(&encoded).expect("decode");
        match decoded.payload {
            AppPayload::Chat(c) => assert_eq!(c.body, "hello"),
            _ => panic!("expected chat payload"),
        }
    }

    #[tokio::test]
    async fn bulletin_expiry_pruning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = AppStorage::open(dir.path().to_str().expect("path")).expect("open");
        let now = mycelium_core::data::now_ms();
        let mk = |expires_at_ms: u64, title: &str| BulletinPost {
            id: uuid::Uuid::new_v4(),
            from_display_name: "alice".to_string(),
            title: title.to_string(),
            body: "body".to_string(),
            scope: "scope/a".to_string(),
            timestamp_ms: now,
            expires_at_ms,
        };
        storage.save_bulletin(&mk(now - 1000, "e1")).expect("save1");
        storage.save_bulletin(&mk(now - 500, "e2")).expect("save2");
        storage
            .save_bulletin(&mk(now + 10_000, "live"))
            .expect("save3");
        let removed = storage.prune_expired_bulletins().expect("prune");
        assert_eq!(removed, 2);
        let left = storage
            .bulletins_for_scope("scope/a")
            .expect("list bulletins");
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].title, "live");
    }

    #[tokio::test]
    async fn mail_inbox_sorted_by_timestamp() {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = AppStorage::open(dir.path().to_str().expect("path")).expect("open");
        let mk = |ts: u64, body: &str| MailMessage {
            id: uuid::Uuid::new_v4(),
            from_peer: "p1".to_string(),
            from_display_name: "alice".to_string(),
            to_peer: "p2".to_string(),
            subject: "sub".to_string(),
            body: body.to_string(),
            timestamp_ms: ts,
            attachments: vec![],
        };
        storage
            .save_mail_inbox(&mk(100, "older"))
            .expect("save older");
        storage
            .save_mail_inbox(&mk(300, "newest"))
            .expect("save newest");
        storage
            .save_mail_inbox(&mk(200, "middle"))
            .expect("save middle");

        let inbox = storage.inbox(10).expect("inbox");
        assert_eq!(inbox[0].body, "newest");
        assert_eq!(inbox[1].body, "middle");
        assert_eq!(inbox[2].body, "older");
    }

    #[tokio::test]
    async fn group_record_peer_seen_persists() {
        use crate::groups::Group;
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = AppStorage::open(dir.path().to_str().expect("path")).expect("open");
        let g = Group::new("Team".into());
        storage.save_group(&g).expect("save");
        storage
            .group_record_peer_seen(&g.id, "12D3KooWpeer")
            .expect("record");
        let loaded = storage.group_by_id(&g.id).expect("load").expect("exists");
        assert!(loaded.members.contains(&"12D3KooWpeer".to_string()));
    }
}
