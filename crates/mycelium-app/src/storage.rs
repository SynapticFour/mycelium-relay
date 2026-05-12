use crate::envelope::{BulletinPost, ChatMessage, MailMessage};
use mycelium_core::data::now_ms;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AppStorage {
    db: sled::Db,
}

impl AppStorage {
    pub fn open(path: &str) -> anyhow::Result<Self> {
        let db = sled::Config::new()
            .path(path)
            .flush_every_ms(Some(5000))
            .open()?;
        Ok(Self { db })
    }

    pub fn save_chat(&self, peer_id: &str, msg: &ChatMessage) -> anyhow::Result<()> {
        let tree = self.db.open_tree(format!("chat:{peer_id}"))?;
        tree.insert(ts_key(msg.timestamp_ms), bincode::serialize(msg)?)?;
        Ok(())
    }

    pub fn chat_history(&self, peer_id: &str, limit: usize) -> anyhow::Result<Vec<ChatMessage>> {
        let tree = self.db.open_tree(format!("chat:{peer_id}"))?;
        let mut out = Vec::new();
        for item in tree.iter().rev().take(limit) {
            let (_k, v) = item?;
            out.push(bincode::deserialize(&v)?);
        }
        Ok(out)
    }

    pub fn save_bulletin(&self, post: &BulletinPost) -> anyhow::Result<()> {
        let tree = self.db.open_tree(format!("bulletin:{}", post.scope))?;
        let key = format!("{:020}:{}", post.expires_at_ms, post.id);
        tree.insert(key.as_bytes(), bincode::serialize(post)?)?;
        Ok(())
    }

    pub fn bulletins_for_scope(&self, scope: &str) -> anyhow::Result<Vec<BulletinPost>> {
        let tree = self.db.open_tree(format!("bulletin:{scope}"))?;
        let mut out = Vec::new();
        let now = now_ms();
        for item in tree.iter() {
            let (_k, v) = item?;
            let post: BulletinPost = bincode::deserialize(&v)?;
            if post.expires_at_ms >= now {
                out.push(post);
            }
        }
        out.sort_by_key(|p| std::cmp::Reverse(p.timestamp_ms));
        Ok(out)
    }

    pub fn prune_expired_bulletins(&self) -> anyhow::Result<usize> {
        let now = now_ms();
        let mut removed = 0usize;
        for name in self.db.tree_names() {
            let Some(name) = std::str::from_utf8(&name).ok() else {
                continue;
            };
            if !name.starts_with("bulletin:") {
                continue;
            }
            let tree = self.db.open_tree(name)?;
            let keys: Vec<Vec<u8>> = tree
                .iter()
                .filter_map(|res| {
                    let (k, v) = res.ok()?;
                    let post: BulletinPost = bincode::deserialize(&v).ok()?;
                    if post.expires_at_ms < now {
                        Some(k.to_vec())
                    } else {
                        None
                    }
                })
                .collect();
            for k in keys {
                tree.remove(k)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn save_mail_inbox(&self, mail: &MailMessage) -> anyhow::Result<()> {
        let tree = self.db.open_tree("mail:inbox")?;
        tree.insert(
            ts_uuid_key(mail.timestamp_ms, mail.id),
            bincode::serialize(mail)?,
        )?;
        Ok(())
    }

    pub fn save_mail_sent(&self, mail: &MailMessage) -> anyhow::Result<()> {
        let tree = self.db.open_tree("mail:sent")?;
        tree.insert(
            ts_uuid_key(mail.timestamp_ms, mail.id),
            bincode::serialize(mail)?,
        )?;
        Ok(())
    }

    pub fn inbox(&self, limit: usize) -> anyhow::Result<Vec<MailMessage>> {
        let tree = self.db.open_tree("mail:inbox")?;
        newest_mails(&tree, limit)
    }

    pub fn sent(&self, limit: usize) -> anyhow::Result<Vec<MailMessage>> {
        let tree = self.db.open_tree("mail:sent")?;
        newest_mails(&tree, limit)
    }

    pub fn mark_read(&self, mail_id: &Uuid) -> anyhow::Result<()> {
        let tree = self.db.open_tree("mail:read")?;
        tree.insert(mail_id.to_string().as_bytes(), &[1])?;
        Ok(())
    }

    pub fn is_read(&self, mail_id: &Uuid) -> anyhow::Result<bool> {
        let tree = self.db.open_tree("mail:read")?;
        Ok(tree.get(mail_id.to_string().as_bytes())?.is_some())
    }
}

fn newest_mails(tree: &sled::Tree, limit: usize) -> anyhow::Result<Vec<MailMessage>> {
    let mut out = Vec::new();
    for item in tree.iter().rev().take(limit) {
        let (_k, v) = item?;
        out.push(bincode::deserialize(&v)?);
    }
    Ok(out)
}

fn ts_key(ts: u64) -> [u8; 8] {
    ts.to_be_bytes()
}

fn ts_uuid_key(ts: u64, id: Uuid) -> Vec<u8> {
    format!("{:020}:{id}", ts).into_bytes()
}
