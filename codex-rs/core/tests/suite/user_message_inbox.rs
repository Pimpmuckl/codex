use anyhow::Result;
use codex_config::CONFIG_TOML_FILE;
use codex_protocol::ThreadId;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::TurnItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use core_test_support::wait_for_event_match;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

const CALL_ID: &str = "call-message-1";
const ENVELOPE: &str = "[Message for you]\n\nCheck the deployment.";

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

    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "Continue the task.".to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;

    let item = wait_for_event_match(&test.codex, |event| match event {
        EventMsg::ItemCompleted(event) => match &event.item {
            TurnItem::AgentMessage(item) if item.id == format!("user-message:{CALL_ID}") => {
                Some(item.clone())
            }
            _ => None,
        },
        _ => None,
    })
    .await;
    assert_user_message_item(&item);
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
        items[0].get("type").and_then(Value::as_str) == Some("function_call")
            && items[0].get("call_id").and_then(Value::as_str) == Some(CALL_ID)
            && items[1].get("type").and_then(Value::as_str) == Some("function_call_output")
            && items[1].get("call_id").and_then(Value::as_str) == Some(CALL_ID)
    }));
    assert!(!serde_json::to_string(&input)?.contains("[Message for you]"));
    Ok(())
}

#[test]
fn user_message_history_mode_keeps_the_readable_note_in_both_formats() {
    let item = TurnItem::AgentMessage(user_message_item());
    let raw = vec![
        RolloutItem::EventMsg(EventMsg::ItemCompleted(ItemCompletedEvent {
            thread_id: ThreadId::new(),
            turn_id: "turn-1".to_string(),
            item: item.clone(),
            completed_at_ms: 1,
        })),
        RolloutItem::EventMsg(item.as_legacy_events(false).pop().unwrap()),
    ];

    let legacy = codex_rollout::persisted_rollout_items(&raw, ThreadHistoryMode::Legacy);
    let [RolloutItem::EventMsg(EventMsg::AgentMessage(event))] = legacy.as_slice() else {
        panic!("legacy history should retain AgentMessage");
    };
    assert_eq!(event.message, ENVELOPE);

    let paginated = codex_rollout::persisted_rollout_items(&raw, ThreadHistoryMode::Paginated);
    let [RolloutItem::EventMsg(EventMsg::ItemCompleted(event))] = paginated.as_slice() else {
        panic!("paginated history should retain ItemCompleted");
    };
    let TurnItem::AgentMessage(item) = &event.item else {
        panic!("paginated history should retain AgentMessage item");
    };
    assert_user_message_item(item);
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

fn assert_user_message_item(item: &AgentMessageItem) {
    assert_eq!(item.id, format!("user-message:{CALL_ID}"));
    let [AgentMessageContent::Text { text }] = item.content.as_slice() else {
        panic!("expected one text content item");
    };
    assert_eq!(text, ENVELOPE);
    assert_eq!(item.phase, Some(MessagePhase::Commentary));
}
