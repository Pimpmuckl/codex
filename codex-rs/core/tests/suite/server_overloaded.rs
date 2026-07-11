use std::time::Duration;

use codex_protocol::error::CodexErr;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::sse_failed;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;

const CAPACITY_MESSAGE: &str = "The selected model is at capacity. Retrying in one minute (1/1).";

async fn submit(codex: &codex_core::CodexThread) {
    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .expect("submit turn");
}

#[tokio::test]
async fn capacity_retry_waits_one_minute_and_preserves_input() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = start_mock_server().await;
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse_failed("resp-overloaded", "server_is_overloaded", "at capacity"),
            sse(vec![
                ev_response_created("resp-ok"),
                ev_completed("resp-ok"),
            ]),
        ],
    )
    .await;
    let test = test_codex()
        .with_config(|config| {
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(1);
        })
        .build(&server)
        .await?;

    submit(&test.codex).await;
    let warning = wait_for_event(
        &test.codex,
        |event| matches!(event, EventMsg::Warning(warning) if warning.message == CAPACITY_MESSAGE),
    )
    .await;
    let EventMsg::Warning(warning) = warning else {
        unreachable!();
    };
    assert_eq!(warning.message, CAPACITY_MESSAGE);
    assert_eq!(responses.requests().len(), 1);
    tokio::time::pause();
    tokio::task::yield_now().await;

    tokio::time::advance(Duration::from_secs(59)).await;
    tokio::task::yield_now().await;
    assert_eq!(responses.requests().len(), 1);
    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::time::resume();
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].input(), requests[1].input());
    Ok(())
}

#[tokio::test]
async fn interrupt_cancels_capacity_retry_wait() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = start_mock_server().await;
    let responses = mount_sse_once(
        &server,
        sse_failed("resp-overloaded", "server_is_overloaded", "at capacity"),
    )
    .await;
    let test = test_codex()
        .with_config(|config| {
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(1);
        })
        .build(&server)
        .await?;

    submit(&test.codex).await;
    wait_for_event(
        &test.codex,
        |event| matches!(event, EventMsg::Warning(warning) if warning.message == CAPACITY_MESSAGE),
    )
    .await;
    tokio::time::pause();
    tokio::task::yield_now().await;
    test.codex.submit(Op::Interrupt).await?;
    tokio::time::resume();
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnAborted(_))
    })
    .await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(60)).await;
    tokio::task::yield_now().await;
    tokio::time::resume();
    assert_eq!(responses.requests().len(), 1);
    Ok(())
}

#[tokio::test]
async fn exhausted_capacity_retry_budget_surfaces_original_error() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = start_mock_server().await;
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse_failed("resp-overloaded-1", "server_is_overloaded", "at capacity"),
            sse_failed(
                "resp-overloaded-2",
                "server_is_overloaded",
                "still at capacity",
            ),
        ],
    )
    .await;
    let test = test_codex()
        .with_config(|config| {
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(1);
        })
        .build(&server)
        .await?;

    submit(&test.codex).await;
    wait_for_event(
        &test.codex,
        |event| matches!(event, EventMsg::Warning(warning) if warning.message == CAPACITY_MESSAGE),
    )
    .await;
    tokio::time::pause();
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(60)).await;
    tokio::time::resume();
    let error = wait_for_event(&test.codex, |event| matches!(event, EventMsg::Error(_))).await;
    let EventMsg::Error(error) = error else {
        unreachable!();
    };
    assert_eq!(error.message, CodexErr::ServerOverloaded.to_string());
    assert_eq!(
        error.codex_error_info,
        Some(CodexErrorInfo::ServerOverloaded)
    );
    assert_eq!(responses.requests().len(), 2);
    Ok(())
}
