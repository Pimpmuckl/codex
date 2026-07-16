//! Durable user-message recognition, thread-local storage, and TUI rendering.

use std::collections::VecDeque;

use codex_config::ConfigLayerStack;
use codex_config::UserMessageInbox;
use codex_protocol::codex_plus_plus::USER_MESSAGE_ENVELOPE_PREFIX;
use codex_protocol::codex_plus_plus::USER_MESSAGE_ITEM_ID_PREFIX;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::models::MessagePhase;
use ratatui::style::Stylize;
use ratatui::text::Line;
use textwrap::Options;

use crate::history_cell::HistoryCell;

const MAX_MESSAGES: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UserMessage {
    id: String,
    body: String,
}

#[derive(Default)]
pub(crate) struct UserMessageInboxState {
    messages: VecDeque<UserMessage>,
    overflowed: bool,
}

#[derive(Debug)]
pub(crate) struct UserMessageHistoryCell {
    body: String,
}

#[derive(Debug)]
pub(crate) struct InboxHistoryCell {
    enabled: bool,
    messages: Vec<UserMessage>,
    overflowed: bool,
}

pub(crate) fn enabled(config_layer_stack: &ConfigLayerStack) -> bool {
    config_layer_stack
        .effective_config()
        .get("user_message_inbox")
        .cloned()
        .and_then(|value| value.try_into::<UserMessageInbox>().ok())
        .unwrap_or_default()
        == UserMessageInbox::Enabled
}

pub(crate) fn recognize(item: &AgentMessageItem) -> Option<UserMessage> {
    if item.phase != Some(MessagePhase::Commentary)
        || !item.id.starts_with(USER_MESSAGE_ITEM_ID_PREFIX)
    {
        return None;
    }

    let message = item
        .content
        .iter()
        .map(|content| match content {
            AgentMessageContent::Text { text } => text.as_str(),
        })
        .collect::<String>();
    let body = message.strip_prefix(USER_MESSAGE_ENVELOPE_PREFIX)?.trim();
    (!body.is_empty()).then(|| UserMessage {
        id: item.id.clone(),
        body: body.to_string(),
    })
}

impl UserMessageInboxState {
    pub(crate) fn record(&mut self, message: UserMessage) -> Option<UserMessageHistoryCell> {
        if self.messages.iter().any(|stored| stored.id == message.id) {
            return None;
        }

        let cell = UserMessageHistoryCell {
            body: message.body.clone(),
        };
        self.messages.push_front(message);
        if self.messages.len() > MAX_MESSAGES {
            self.messages.pop_back();
            self.overflowed = true;
        }
        Some(cell)
    }

    pub(crate) fn history_cell(&self, enabled: bool) -> InboxHistoryCell {
        InboxHistoryCell {
            enabled,
            messages: self.messages.iter().cloned().collect(),
            overflowed: self.overflowed,
        }
    }
}

fn push_wrapped(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    width: usize,
    initial_indent: &str,
    subsequent_indent: &str,
) {
    for (index, source_line) in text.lines().enumerate() {
        let initial_indent = if index == 0 {
            initial_indent
        } else {
            subsequent_indent
        };
        if source_line.is_empty() {
            lines.push(Line::default());
            continue;
        }
        lines.extend(
            textwrap::wrap(
                source_line,
                Options::new(width.max(1))
                    .initial_indent(initial_indent)
                    .subsequent_indent(subsequent_indent),
            )
            .into_iter()
            .map(|line| Line::from(line.into_owned())),
        );
    }
}

impl HistoryCell for UserMessageHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = vec!["Message for you".bold().into()];
        push_wrapped(&mut lines, &self.body, width.into(), "  ", "  ");
        lines
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        let mut lines = vec!["Message for you".into()];
        lines.extend(self.body.lines().map(|line| Line::from(line.to_string())));
        lines
    }
}

impl HistoryCell for InboxHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        if self.messages.is_empty() {
            return vec![if self.enabled {
                "No messages for you in this thread.".into()
            } else {
                "Enable Agent inbox messages in /codexplusplus, then restart Codex.".into()
            }];
        }

        let mut lines = vec!["Messages for you".bold().into()];
        for (index, message) in self.messages.iter().enumerate() {
            let initial_indent = format!("{}. ", index + 1);
            let subsequent_indent = " ".repeat(initial_indent.len());
            push_wrapped(
                &mut lines,
                &message.body,
                width.into(),
                &initial_indent,
                &subsequent_indent,
            );
        }
        if self.overflowed {
            lines.push(
                "Showing the 50 newest messages. Older messages remain in the transcript.".into(),
            );
        }
        if !self.enabled {
            lines.push("Enable Agent inbox messages in /codexplusplus, then restart Codex.".into());
        }
        lines
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.display_lines(u16::MAX)
    }
}

#[cfg(test)]
#[path = "user_message_inbox_tests.rs"]
mod tests;
