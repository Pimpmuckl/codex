use codex_protocol::codex_plus_plus::USER_MESSAGE_ITEM_ID_PREFIX;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::protocol::EventMsg;

use crate::protocol::v2::ThreadItem;

#[derive(Default)]
pub(crate) struct InboxLegacyDeduper {
    pending_message: Option<String>,
}

impl InboxLegacyDeduper {
    pub(crate) fn should_materialize(item: &AgentMessageItem) -> bool {
        item.id.starts_with(USER_MESSAGE_ITEM_ID_PREFIX)
    }

    pub(crate) fn prepare_for(&mut self, event: &EventMsg) {
        if !matches!(event, EventMsg::AgentMessage(_)) {
            self.pending_message = None;
        }
    }

    pub(crate) fn record_materialized(&mut self, item: &ThreadItem) {
        if let ThreadItem::AgentMessage { id, text, .. } = item
            && id.starts_with(USER_MESSAGE_ITEM_ID_PREFIX)
        {
            self.pending_message = Some(text.clone());
        }
    }

    pub(crate) fn consume_if_matches(&mut self, text: &str) -> bool {
        self.pending_message.take().as_deref() == Some(text)
    }
}
