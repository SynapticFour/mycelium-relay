// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use crate::contacts::{Contact, ContactStatus};
use crate::envelope::{BulletinPost, ChatMessage, MailMessage};
use crate::groups::Group;
use crate::scope_key::ScopeKey;
use mycelium_core::data::now_ms;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AppStorage {
    db: sled::Db,
    /// When set, mini-app KV values are encrypted at rest (Cell C2).
    at_rest_key: Option<[u8; 32]>,
}

impl AppStorage {
    pub fn open(path: &str) -> anyhow::Result<Self> {
        Self::open_with_key(path, None)
    }

    pub fn open_with_key(path: &str, at_rest_key: Option<[u8; 32]>) -> anyhow::Result<Self> {
        let db = sled::Config::new()
            .path(path)
            .flush_every_ms(Some(5000))
            .open()?;
        Ok(Self { db, at_rest_key })
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

    pub fn save_contact(&self, contact: &Contact) -> anyhow::Result<()> {
        let tree = self.db.open_tree("contacts:data")?;
        tree.insert(contact.peer_id.as_bytes(), bincode::serialize(contact)?)?;
        Ok(())
    }

    pub fn contact_by_id(&self, peer_id: &str) -> anyhow::Result<Option<Contact>> {
        let tree = self.db.open_tree("contacts:data")?;
        Ok(match tree.get(peer_id.as_bytes())? {
            Some(v) => Some(bincode::deserialize(&v)?),
            None => None,
        })
    }

    pub fn all_contacts(&self) -> anyhow::Result<Vec<Contact>> {
        let tree = self.db.open_tree("contacts:data")?;
        let mut out: Vec<Contact> = Vec::new();
        for item in tree.iter() {
            let (_k, v) = item?;
            out.push(bincode::deserialize(&v)?);
        }
        out.sort_by_key(|c| std::cmp::Reverse(c.added_at_ms));
        Ok(out)
    }

    pub fn contacts_with_status(&self, status: ContactStatus) -> anyhow::Result<Vec<Contact>> {
        Ok(self
            .all_contacts()?
            .into_iter()
            .filter(|c| c.status == status)
            .collect())
    }

    pub fn delete_contact(&self, peer_id: &str) -> anyhow::Result<()> {
        let tree = self.db.open_tree("contacts:data")?;
        tree.remove(peer_id.as_bytes())?;
        Ok(())
    }

    pub fn upsert_contact(
        &self,
        peer_id: &str,
        display_name: &str,
        status: ContactStatus,
    ) -> anyhow::Result<Contact> {
        let contact = match self.contact_by_id(peer_id)? {
            Some(mut existing) => {
                if !display_name.is_empty() {
                    existing.display_name = display_name.to_string();
                }
                existing.status = status;
                existing
            }
            None => Contact::new(peer_id.to_string(), display_name.to_string(), status),
        };
        self.save_contact(&contact)?;
        Ok(contact)
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

    pub fn save_group(&self, group: &Group) -> anyhow::Result<()> {
        let tree = self.db.open_tree("groups:data")?;
        tree.insert(group.id.as_bytes(), bincode::serialize(group)?)?;
        Ok(())
    }

    pub fn delete_group(&self, id: &str) -> anyhow::Result<()> {
        let tree = self.db.open_tree("groups:data")?;
        tree.remove(id.as_bytes())?;
        Ok(())
    }

    pub fn group_by_id(&self, id: &str) -> anyhow::Result<Option<Group>> {
        let tree = self.db.open_tree("groups:data")?;
        Ok(match tree.get(id.as_bytes())? {
            Some(v) => Some(bincode::deserialize(&v)?),
            None => None,
        })
    }

    pub fn all_groups(&self) -> anyhow::Result<Vec<Group>> {
        let tree = self.db.open_tree("groups:data")?;
        let mut out = Vec::new();
        for item in tree.iter() {
            let (_k, v) = item?;
            out.push(bincode::deserialize(&v)?);
        }
        Ok(out)
    }

    /// Records a libp2p peer id as having been seen in this group (best-effort membership).
    pub fn group_record_peer_seen(&self, group_id: &str, peer_id: &str) -> anyhow::Result<()> {
        let Some(mut g) = self.group_by_id(group_id)? else {
            return Ok(());
        };
        g.record_peer_seen(peer_id.to_string());
        self.save_group(&g)
    }

    pub fn save_group_chat(&self, group_id: &str, msg: &ChatMessage) -> anyhow::Result<()> {
        let tree = self.db.open_tree(format!("groupchat:{group_id}"))?;
        tree.insert(
            ts_uuid_key(msg.timestamp_ms, msg.id),
            bincode::serialize(msg)?,
        )?;
        Ok(())
    }

    pub fn group_chat_history(
        &self,
        group_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<ChatMessage>> {
        let tree = self.db.open_tree(format!("groupchat:{group_id}"))?;
        let mut out = Vec::new();
        for item in tree.iter().rev().take(limit) {
            let (_k, v) = item?;
            out.push(bincode::deserialize(&v)?);
        }
        Ok(out)
    }

    /// Mini-app bridge KV (`app:<app_id>:<user_key>`) in the main app sled DB.
    pub fn miniapp_get(&self, scoped_key: &str) -> anyhow::Result<Option<String>> {
        let tree = self.db.open_tree("miniapp_storage")?;
        Ok(match tree.get(scoped_key.as_bytes())? {
            Some(v) => Some(self.decode_miniapp_value(scoped_key, &v)?),
            None => None,
        })
    }

    pub fn miniapp_set(&self, scoped_key: &str, value: &str) -> anyhow::Result<()> {
        let tree = self.db.open_tree("miniapp_storage")?;
        let bytes = self.encode_miniapp_value(scoped_key, value)?;
        tree.insert(scoped_key.as_bytes(), bytes)?;
        Ok(())
    }

    fn encode_miniapp_value(&self, scoped_key: &str, value: &str) -> anyhow::Result<Vec<u8>> {
        match &self.at_rest_key {
            Some(master) => Ok(crate::miniapp_storage_enc::encrypt_value(
                master, scoped_key, value,
            )),
            None => Ok(value.as_bytes().to_vec()),
        }
    }

    fn decode_miniapp_value(&self, scoped_key: &str, blob: &[u8]) -> anyhow::Result<String> {
        match &self.at_rest_key {
            Some(master) => crate::miniapp_storage_enc::decrypt_value(master, scoped_key, blob),
            None => Ok(String::from_utf8_lossy(blob).into_owned()),
        }
    }

    pub fn miniapp_delete(&self, scoped_key: &str) -> anyhow::Result<()> {
        let tree = self.db.open_tree("miniapp_storage")?;
        tree.remove(scoped_key.as_bytes())?;
        Ok(())
    }

    pub fn miniapp_list(&self, scoped_prefix: &str) -> anyhow::Result<Vec<String>> {
        let tree = self.db.open_tree("miniapp_storage")?;
        let mut out = Vec::new();
        for item in tree.scan_prefix(scoped_prefix.as_bytes()) {
            let (k, _) = item?;
            out.push(String::from_utf8_lossy(&k).into_owned());
        }
        out.sort();
        Ok(out)
    }

    pub fn miniapp_clear_all_for_app(&self, app_id: &str) -> anyhow::Result<()> {
        let prefix = format!("app:{app_id}:");
        let tree = self.db.open_tree("miniapp_storage")?;
        let keys: Vec<Vec<u8>> = tree
            .scan_prefix(prefix.as_bytes())
            .filter_map(|r| r.ok())
            .map(|(k, _)| k.to_vec())
            .collect();
        for k in keys {
            tree.remove(k)?;
        }
        Ok(())
    }

    pub fn save_scope_key(&self, sk: &ScopeKey) -> anyhow::Result<()> {
        let tree = self.db.open_tree("scope_keys")?;
        tree.insert(sk.scope.as_bytes(), serde_json::to_vec(sk)?.as_slice())?;
        Ok(())
    }

    pub fn get_scope_key(&self, scope: &str) -> anyhow::Result<Option<ScopeKey>> {
        let tree = self.db.open_tree("scope_keys")?;
        Ok(match tree.get(scope.as_bytes())? {
            Some(bytes) => Some(serde_json::from_slice(&bytes)?),
            None => None,
        })
    }

    pub fn all_scope_keys(&self) -> anyhow::Result<Vec<ScopeKey>> {
        let tree = self.db.open_tree("scope_keys")?;
        let mut keys = Vec::new();
        for item in tree.iter() {
            let (_, v) = item?;
            if let Ok(sk) = serde_json::from_slice::<ScopeKey>(&v) {
                keys.push(sk);
            }
        }
        keys.sort_by_key(|sk| sk.added_at_ms);
        Ok(keys)
    }

    pub fn delete_scope_key(&self, scope: &str) -> anyhow::Result<()> {
        let tree = self.db.open_tree("scope_keys")?;
        tree.remove(scope.as_bytes())?;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn miniapp_storage_encryption_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let key = blake3::derive_key("test-miniapp-enc", b"seed");
        let storage = AppStorage::open_with_key(dir.path().to_str().unwrap(), Some(key)).unwrap();
        storage
            .miniapp_set("app:com.enc.app:secret", "payload")
            .unwrap();
        let v = storage.miniapp_get("app:com.enc.app:secret").unwrap();
        assert_eq!(v.as_deref(), Some("payload"));
    }

    #[test]
    fn miniapp_storage_isolation() {
        let dir = tempfile::tempdir().unwrap();
        let storage = AppStorage::open(dir.path().to_str().unwrap()).unwrap();

        storage
            .miniapp_set("app:com.app.a:key1", "value_a")
            .unwrap();
        storage
            .miniapp_set("app:com.app.b:key1", "value_b")
            .unwrap();

        let a_keys = storage.miniapp_list("app:com.app.a:").unwrap();
        assert_eq!(a_keys.len(), 1);
        assert!(a_keys[0].contains("com.app.a"));

        storage.miniapp_clear_all_for_app("com.app.a").unwrap();
        let b_val = storage.miniapp_get("app:com.app.b:key1").unwrap();
        assert_eq!(b_val.as_deref(), Some("value_b"));
    }
}
