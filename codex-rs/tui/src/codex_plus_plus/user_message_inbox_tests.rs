use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::models::MessagePhase;
use pretty_assertions::assert_eq;

use super::*;

fn item(id: &str, text: &str, phase: Option<MessagePhase>) -> AgentMessageItem {
    AgentMessageItem {
        id: id.to_string(),
        content: vec![AgentMessageContent::Text {
            text: text.to_string(),
        }],
        phase,
        memory_citation: None,
    }
}
fn user_message(id: impl Into<String>, body: impl Into<String>) -> UserMessage {
    UserMessage {
        id: id.into(),
        body: body.into(),
    }
}
fn rendered(cell: &impl HistoryCell, width: u16) -> String {
    cell.display_lines(width)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn recognizes_only_exact_durable_user_messages() {
    assert!(!enabled(&ConfigLayerStack::default()));
    let message = recognize(&item(
        "user-message:call-1",
        "[Message for you]\n  Check deployment.  ",
        Some(MessagePhase::Commentary),
    ));
    assert_eq!(
        message.map(|message| (message.id, message.body)),
        Some((
            "user-message:call-1".to_string(),
            "Check deployment.".to_string(),
        ))
    );

    for invalid in [
        item(
            "message:call-1",
            "[Message for you]\nCheck deployment.",
            Some(MessagePhase::Commentary),
        ),
        item(
            "user-message:call-1",
            "Message for you\nCheck deployment.",
            Some(MessagePhase::Commentary),
        ),
        item(
            "user-message:call-1",
            "[Message for you]\nCheck deployment.",
            Some(MessagePhase::FinalAnswer),
        ),
        item(
            "user-message:call-1",
            "[Message for you]\n  ",
            Some(MessagePhase::Commentary),
        ),
    ] {
        assert_eq!(recognize(&invalid), None);
    }
}
#[test]
fn transcript_and_inbox_snapshots_cover_bounded_thread_state() {
    let mut populated = UserMessageInboxState::default();
    let transcript_cell = populated
        .record(user_message(
            "user-message:first",
            "First paragraph\n\nSecond paragraph",
        ))
        .expect("new message");
    populated
        .record(user_message("user-message:second", "Second note"))
        .expect("new message");
    assert!(
        populated
            .record(user_message("user-message:second", "duplicate"))
            .is_none()
    );

    let transcript_lines = transcript_cell.display_lines(/*width*/ 40);
    assert!(
        transcript_lines
            .iter()
            .flat_map(|line| &line.spans)
            .all(|span| span.style.fg.is_none())
    );
    insta::assert_snapshot!(
        "user_message_transcript_cell",
        rendered(&transcript_cell, /*width*/ 40)
    );
    insta::assert_snapshot!(
        "user_message_inbox_disabled",
        rendered(
            &UserMessageInboxState::default().history_cell(/*enabled*/ false),
            /*width*/ 80,
        )
    );
    insta::assert_snapshot!(
        "user_message_inbox_empty",
        rendered(
            &UserMessageInboxState::default().history_cell(/*enabled*/ true),
            /*width*/ 80,
        )
    );
    insta::assert_snapshot!(
        "user_message_inbox_populated",
        rendered(&populated.history_cell(/*enabled*/ true), /*width*/ 80)
    );
    let disabled_with_messages = rendered(&populated.history_cell(false), 80);
    assert!(disabled_with_messages.contains("Second note"));
    assert!(disabled_with_messages.contains("Enable Agent inbox messages"));
    let mut overflowed = UserMessageInboxState::default();
    for index in 0..=MAX_MESSAGES {
        overflowed
            .record(user_message(
                format!("user-message:{index}"),
                format!("Message {index}"),
            ))
            .expect("unique message");
    }
    assert_eq!(overflowed.messages.len(), MAX_MESSAGES);
    assert_eq!(overflowed.messages.front().unwrap().body, "Message 50");
    assert_eq!(overflowed.messages.back().unwrap().body, "Message 1");
    let overflow_notice = overflowed
        .history_cell(/*enabled*/ true)
        .display_lines(/*width*/ 80)
        .pop()
        .expect("overflow notice");
    insta::assert_snapshot!(
        "user_message_inbox_overflow",
        overflow_notice
            .spans
            .into_iter()
            .map(|span| span.content.into_owned())
            .collect::<String>()
    );
}
