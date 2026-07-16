use anyhow::Result;
use codex_config::CONFIG_TOML_FILE;
use codex_protocol::ThreadId;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::TurnItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use core_test_support::wait_for_event_match;
use pretty_assertions::assert_eq;
use serde_json::json;

const CALL_ID: &str = "call-message-1";
const ENVELOPE: &str = "[Message for you]\nCheck the deployment.";
fn enable_inbox(config: &mut codex_core::config::Config) {
    config.config_layer_stack = config.config_layer_stack.with_user_config(
        &config.codex_home.join(CONFIG_TOML_FILE),
        toml::toml! { user_message_inbox = "enabled" }.into(),
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_message_tool_preserves_responses_order_without_synthetic_model_message() -> Result<()>
{
    let server = responses::start_mock_server().await;
    let requests = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("resp-1"),
                responses::ev_function_call(
                    CALL_ID,
                    "leave_user_message",
                    &serde_json::to_string(&json!({
                        "message": "  Check the deployment.  "
                    }))?,
                ),
                responses::ev_completed("resp-1"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("resp-2"),
                responses::ev_assistant_message("msg-2", "Continuing."),
                responses::ev_completed("resp-2"),
            ]),
        ],
    )
    .await;
    let test = test_codex()
        .with_config(enable_inbox)
        .build_with_auto_env(&server)
        .await?;

    let input = UserInput::Text {
        text: "Continue the task.".to_string(),
        text_elements: Vec::new(),
    };
    test.codex.submit(vec![input].into()).await?;

    let item = wait_for_event_match(&test.codex, |event| match event {
        EventMsg::ItemCompleted(ItemCompletedEvent {
            item: TurnItem::AgentMessage(item),
            ..
        }) if item.id == format!("user-message:{CALL_ID}") => Some(item.clone()),
        _ => None,
    })
    .await;
    assert_json_eq(&item, &user_message_item());
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let requests = requests.requests();
    let follow_up = &requests[1];
    assert_eq!(
        follow_up.function_call_output_text(CALL_ID).as_deref(),
        Some("Message left for the user.")
    );
    let input = follow_up.input();
    assert!(input.windows(2).any(|items| {
        items[0]["type"] == "function_call"
            && items[0]["call_id"] == CALL_ID
            && items[1]["type"] == "function_call_output"
            && items[1]["call_id"] == CALL_ID
    }));
    assert!(!serde_json::to_string(&input)?.contains("[Message for you]"));
    Ok(())
}

#[test]
fn user_message_history_mode_keeps_the_readable_note_in_both_formats() {
    let item = TurnItem::AgentMessage(user_message_item());
    let completed = RolloutItem::EventMsg(EventMsg::ItemCompleted(ItemCompletedEvent {
        thread_id: ThreadId::new(),
        turn_id: "turn-1".to_string(),
        item: item.clone(),
        completed_at_ms: 1,
    }));
    let legacy = RolloutItem::EventMsg(item.as_legacy_events(false).pop().unwrap());
    let raw = vec![completed.clone(), legacy.clone()];

    let legacy_items = codex_rollout::persisted_rollout_items(&raw, ThreadHistoryMode::Legacy);
    assert_json_eq(&legacy_items, &vec![legacy]);
    let paginated_items =
        codex_rollout::persisted_rollout_items(&raw, ThreadHistoryMode::Paginated);
    assert_json_eq(&paginated_items, &vec![completed]);
}

fn user_message_item() -> AgentMessageItem {
    AgentMessageItem {
        id: format!("user-message:{CALL_ID}"),
        content: vec![AgentMessageContent::Text {
            text: ENVELOPE.to_string(),
        }],
        phase: Some(MessagePhase::Commentary),
        memory_citation: None,
    }
}

fn assert_json_eq<T: serde::Serialize>(actual: &T, expected: &T) {
    let actual = serde_json::to_value(actual).expect("actual value should serialize");
    let expected = serde_json::to_value(expected).expect("expected value should serialize");
    assert_eq!(actual, expected);
}
