//! Session-local unread state for durable user messages.

use std::collections::HashMap;
use std::collections::VecDeque;

use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItem;
use codex_protocol::ThreadId;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;

use super::App;

const MAX_TRACKED_MESSAGES_PER_THREAD: usize = 50;

struct TrackedUserMessage {
    id: String,
    unread: bool,
}

#[derive(Default)]
pub(super) struct UserMessageUnreadState {
    messages: HashMap<ThreadId, VecDeque<TrackedUserMessage>>,
}

impl UserMessageUnreadState {
    fn record(&mut self, thread_id: ThreadId, item_id: &str, is_current: bool) {
        let messages = self.messages.entry(thread_id).or_default();
        if messages.iter().any(|message| message.id == item_id) {
            return;
        }
        messages.push_front(TrackedUserMessage {
            id: item_id.to_string(),
            unread: !is_current,
        });
        messages.truncate(MAX_TRACKED_MESSAGES_PER_THREAD);
    }

    fn mark_read(&mut self, thread_id: ThreadId) {
        if let Some(messages) = self.messages.get_mut(&thread_id) {
            for message in messages {
                message.unread = false;
            }
        }
    }

    fn has_unread(&self, thread_id: ThreadId) -> bool {
        self.messages
            .get(&thread_id)
            .is_some_and(|messages| messages.iter().any(|message| message.unread))
    }

    fn remove(&mut self, thread_id: ThreadId) {
        self.messages.remove(&thread_id);
    }

    fn clear(&mut self) {
        self.messages.clear();
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

    pub(super) fn remove_user_message_thread_state(&mut self, thread_id: ThreadId) {
        self.user_message_unread.remove(thread_id);
    }

    pub(super) fn clear_user_message_unread_state(&mut self) {
        self.user_message_unread.clear();
    }
}
