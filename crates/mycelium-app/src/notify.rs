pub trait NotificationSink: Send + Sync {
    fn on_chat_received(&self, from: &str, preview: &str);
    fn on_mail_received(&self, from: &str, subject: &str);
    fn on_bulletin_posted(&self, scope: &str, title: &str);
}

#[derive(Debug, Default)]
pub struct NoopNotifier;

impl NotificationSink for NoopNotifier {
    fn on_chat_received(&self, _from: &str, _preview: &str) {}
    fn on_mail_received(&self, _from: &str, _subject: &str) {}
    fn on_bulletin_posted(&self, _scope: &str, _title: &str) {}
}
