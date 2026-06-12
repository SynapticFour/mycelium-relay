// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use crate::contacts::ContactStatus;
use crate::storage::AppStorage;
use tempfile::TempDir;

#[test]
fn contact_crud_and_filter_by_status() {
    let dir = TempDir::new().expect("tempdir");
    let storage = AppStorage::open(dir.path().to_str().unwrap()).expect("open storage");

    let c = storage
        .upsert_contact("peer-a", "Alice", ContactStatus::Pending)
        .expect("upsert");
    assert_eq!(c.peer_id, "peer-a");
    assert_eq!(c.status, ContactStatus::Pending);

    let accepted = storage
        .upsert_contact("peer-a", "Alice", ContactStatus::Accepted)
        .expect("accept");
    assert_eq!(accepted.status, ContactStatus::Accepted);

    let pending = storage
        .contacts_with_status(ContactStatus::Pending)
        .expect("pending");
    assert!(pending.is_empty());

    let all = storage.all_contacts().expect("all");
    assert_eq!(all.len(), 1);

    storage.delete_contact("peer-a").expect("delete");
    assert!(storage.contact_by_id("peer-a").expect("lookup").is_none());
}
