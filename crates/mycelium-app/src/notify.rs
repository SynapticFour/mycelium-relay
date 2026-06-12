// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
pub trait NotificationSink: Send + Sync {
    fn on_chat_received(&self, from: &str, preview: &str);
    fn on_mail_received(&self, from: &str, subject: &str);
    fn on_bulletin_posted(&self, scope: &str, title: &str);
    /// New message from a peer who is not yet an accepted contact.
    fn on_contact_request(&self, _peer_id: &str, _display_name: &str) {}
}

#[derive(Debug, Default)]
pub struct NoopNotifier;

impl NotificationSink for NoopNotifier {
    fn on_chat_received(&self, _from: &str, _preview: &str) {}
    fn on_mail_received(&self, _from: &str, _subject: &str) {}
    fn on_bulletin_posted(&self, _scope: &str, _title: &str) {}
}
