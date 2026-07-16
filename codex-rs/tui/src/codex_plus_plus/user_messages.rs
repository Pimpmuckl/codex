//! Session-local unread state for durable user messages.

use std::collections::HashMap;
use std::collections::HashSet;

use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItem;
use codex_protocol::ThreadId;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;

use super::App;

#[derive(Default)]
pub(super) struct UserMessageUnreadState {
    seen: HashMap<ThreadId, HashSet<String>>,
    unread: HashMap<ThreadId, HashSet<String>>,
}

impl UserMessageUnreadState {
    fn record(&mut self, thread_id: ThreadId, item_id: &str, is_current: bool) {
        if !self
            .seen
            .entry(thread_id)
            .or_default()
            .insert(item_id.to_string())
            || is_current
        {
            return;
        }
        self.unread
            .entry(thread_id)
            .or_default()
            .insert(item_id.to_string());
    }

    fn mark_read(&mut self, thread_id: ThreadId) {
        self.unread.remove(&thread_id);
    }

    fn has_unread(&self, thread_id: ThreadId) -> bool {
        self.unread
            .get(&thread_id)
            .is_some_and(|messages| !messages.is_empty())
    }

    fn clear(&mut self) {
        self.seen.clear();
        self.unread.clear();
    }
}

impl App {
    pub(super) fn record_live_user_message(
        &mut self,
        thread_id: ThreadId,
        notification: &ServerNotification,
    ) {
        let ServerNotification::ItemCompleted(notification) = notification else {
            return;
        };
        let ThreadItem::AgentMessage {
            id, text, phase, ..
        } = &notification.item
        else {
            return;
        };
        let item = AgentMessageItem {
            id: id.clone(),
            content: vec![AgentMessageContent::Text { text: text.clone() }],
            phase: phase.clone(),
            memory_citation: None,
        };
        if crate::codex_plus_plus::recognize_user_message(&item).is_some() {
            self.user_message_unread.record(
                thread_id,
                id,
                self.current_displayed_thread_id() == Some(thread_id),
            );
        }
    }

    pub(super) fn mark_user_messages_read(&mut self, thread_id: ThreadId) {
        self.user_message_unread.mark_read(thread_id);
    }

    pub(super) fn has_unread_user_message(&self, thread_id: ThreadId) -> bool {
        self.user_message_unread.has_unread(thread_id)
    }

    pub(super) fn clear_user_message_unread_state(&mut self) {
        self.user_message_unread.clear();
    }
}
