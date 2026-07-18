use super::*;
use assert_matches::assert_matches;
use codex_utils_path_uri::PathUri;

fn live_status_snapshot(
    email: &str,
    percent: f64,
) -> crate::codex_plus_plus::LiveStatusAccountSnapshot {
    let plan_type = PlanType::Plus;
    crate::codex_plus_plus::LiveStatusAccountSnapshot {
        account_display: StatusAccountDisplay::ChatGpt {
            email: Some(email.to_string()),
            plan: Some(crate::status::plan_type_display_name(plan_type)),
        },
        plan_type,
        rate_limits: vec![snapshot(percent)],
    }
}

#[tokio::test]
async fn status_command_renders_immediately_and_refreshes_rate_limits_for_chatgpt_auth() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    set_chatgpt_auth(&mut chat);

    chat.dispatch_command(SlashCommand::Status);

    let rendered = match rx.try_recv() {
        Ok(AppEvent::InsertHistoryCell(cell)) => {
            lines_to_single_string(&cell.display_lines(/*width*/ 80))
        }
        other => panic!("expected status output before refresh request, got {other:?}"),
    };
    assert!(
        !rendered.contains("refreshing limits"),
        "expected /status to avoid transient refresh text in terminal history, got: {rendered}"
    );
    let request_id = match rx.try_recv() {
        Ok(AppEvent::RefreshStatusAccountSnapshot { request_id }) => request_id,
        other => panic!("expected rate-limit refresh request, got {other:?}"),
    };
    pretty_assertions::assert_eq!(request_id, 0);
}

#[tokio::test]
async fn status_command_refresh_updates_cached_limits_for_future_status_outputs() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    set_chatgpt_auth(&mut chat);

    chat.dispatch_command(SlashCommand::Status);

    let status_cell = match rx.try_recv() {
        Ok(AppEvent::InsertHistoryCell(cell)) => cell,
        other => panic!("expected status output before refresh request, got {other:?}"),
    };
    let first_request_id = match rx.try_recv() {
        Ok(AppEvent::RefreshStatusAccountSnapshot { request_id }) => request_id,
        other => panic!("expected rate-limit refresh request, got {other:?}"),
    };

    chat.finish_status_account_snapshot_refresh(
        first_request_id,
        Some(live_status_snapshot(
            "replacement@example.com",
            /*percent*/ 92.0,
        )),
    );
    let rendered = lines_to_single_string(&status_cell.display_lines(/*width*/ 80));
    let live_account_rows = rendered
        .lines()
        .filter(|line| line.contains("Account:") || line.contains("limit:"))
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(live_account_rows, @r###"
│  Account:              replacement@example.com (Plus)              │
│  Usage limit:          [██░░░░░░░░░░░░░░░░░░] 8% left              │
"###);
    drain_insert_history(&mut rx);

    chat.dispatch_command(SlashCommand::Status);
    let refreshed = match rx.try_recv() {
        Ok(AppEvent::InsertHistoryCell(cell)) => {
            lines_to_single_string(&cell.display_lines(/*width*/ 80))
        }
        other => panic!("expected refreshed status output, got {other:?}"),
    };
    assert!(
        refreshed.contains("8% left"),
        "expected a future /status output to use refreshed cached limits, got: {refreshed}"
    );
}

#[tokio::test]
async fn status_command_renders_immediately_without_rate_limit_refresh() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;

    chat.dispatch_command(SlashCommand::Status);

    assert_matches!(rx.try_recv(), Ok(AppEvent::InsertHistoryCell(_)));
    assert!(
        !std::iter::from_fn(|| rx.try_recv().ok())
            .any(|event| matches!(event, AppEvent::RefreshStatusAccountSnapshot { .. })),
        "non-ChatGPT sessions should not request an account snapshot refresh for /status"
    );
}

#[tokio::test]
async fn status_command_uses_catalog_default_reasoning_when_config_empty() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(Some("gpt-5.4")).await;
    chat.config.model_reasoning_effort = None;

    chat.dispatch_command(SlashCommand::Status);

    let rendered = match rx.try_recv() {
        Ok(AppEvent::InsertHistoryCell(cell)) => {
            lines_to_single_string(&cell.display_lines(/*width*/ 80))
        }
        other => panic!("expected status output, got {other:?}"),
    };
    assert!(
        rendered.contains("gpt-5.4 (reasoning medium, summaries auto)"),
        "expected /status to render the catalog default reasoning effort, got: {rendered}"
    );
}

#[tokio::test]
async fn status_command_renders_native_and_foreign_instruction_sources() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let (foreign_source, foreign_display) = if cfg!(windows) {
        (
            PathUri::parse("file:///remote/AGENTS.md").expect("POSIX instruction source"),
            "/remote/AGENTS.md",
        )
    } else {
        (
            PathUri::parse("file:///C:/remote/AGENTS.md").expect("Windows instruction source"),
            r"C:\remote\AGENTS.md",
        )
    };
    chat.instruction_source_paths = vec![
        PathUri::from_abs_path(&chat.config.cwd.join("AGENTS.md")),
        foreign_source,
    ];

    chat.dispatch_command(SlashCommand::Status);

    let rendered = match rx.try_recv() {
        Ok(AppEvent::InsertHistoryCell(cell)) => {
            lines_to_single_string(&cell.display_lines(/*width*/ 80))
        }
        other => panic!("expected status output, got {other:?}"),
    };
    assert!(
        rendered.contains(&format!("AGENTS.md, {foreign_display}")),
        "expected /status to show native-relative and environment-native foreign paths, got: {rendered}"
    );
    assert!(
        !rendered.contains("Agents.md  <none>"),
        "expected /status to avoid stale <none> when app-server provided instruction sources, got: {rendered}"
    );
}

#[tokio::test]
async fn status_command_overlapping_refreshes_update_matching_cells_only() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    set_chatgpt_auth(&mut chat);

    chat.dispatch_command(SlashCommand::Status);
    match rx.try_recv() {
        Ok(AppEvent::InsertHistoryCell(_)) => {}
        other => panic!("expected first status output, got {other:?}"),
    }
    let first_request_id = match rx.try_recv() {
        Ok(AppEvent::RefreshStatusAccountSnapshot { request_id }) => request_id,
        other => panic!("expected first refresh request, got {other:?}"),
    };

    chat.dispatch_command(SlashCommand::Status);
    let second_rendered = match rx.try_recv() {
        Ok(AppEvent::InsertHistoryCell(cell)) => {
            lines_to_single_string(&cell.display_lines(/*width*/ 80))
        }
        other => panic!("expected second status output, got {other:?}"),
    };
    let second_request_id = match rx.try_recv() {
        Ok(AppEvent::RefreshStatusAccountSnapshot { request_id }) => request_id,
        other => panic!("expected second refresh request, got {other:?}"),
    };

    assert_ne!(first_request_id, second_request_id);
    assert!(
        !second_rendered.contains("refreshing limits"),
        "expected /status to avoid transient refresh text in terminal history, got: {second_rendered}"
    );

    chat.finish_status_account_snapshot_refresh(first_request_id, None);
    pretty_assertions::assert_eq!(chat.refreshing_status_outputs.len(), 1);

    chat.finish_status_account_snapshot_refresh(
        second_request_id,
        Some(live_status_snapshot(
            "replacement@example.com",
            /*percent*/ 92.0,
        )),
    );
    assert!(chat.refreshing_status_outputs.is_empty());
}

#[tokio::test]
async fn failed_live_account_refresh_keeps_identity_and_hides_unverified_limits() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    set_chatgpt_auth(&mut chat);
    chat.status_account_display = Some(StatusAccountDisplay::ChatGpt {
        email: Some("known@example.com".to_string()),
        plan: Some("Plus".to_string()),
    });
    chat.on_rate_limit_snapshot(Some(snapshot(/*percent*/ 92.0)));
    drain_insert_history(&mut rx);

    chat.dispatch_command(SlashCommand::Status);
    let status_cell = match rx.try_recv() {
        Ok(AppEvent::InsertHistoryCell(cell)) => cell,
        other => panic!("expected status output before refresh request, got {other:?}"),
    };
    let request_id = match rx.try_recv() {
        Ok(AppEvent::RefreshStatusAccountSnapshot { request_id }) => request_id,
        other => panic!("expected status refresh request, got {other:?}"),
    };

    chat.finish_status_account_snapshot_refresh(request_id, None);
    let rendered = lines_to_single_string(&status_cell.display_lines(/*width*/ 80));

    assert!(rendered.contains("known@example.com (Plus)"));
    assert!(rendered.contains("data not available yet"));
    assert!(!rendered.contains("8% left"));
}

#[tokio::test]
async fn account_update_rejects_stale_status_rate_limit_snapshots() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    set_chatgpt_auth(&mut chat);
    chat.dispatch_command(SlashCommand::Status);
    assert_matches!(rx.try_recv(), Ok(AppEvent::InsertHistoryCell(_)));
    let request_id = match rx.try_recv() {
        Ok(AppEvent::RefreshStatusAccountSnapshot { request_id }) => request_id,
        other => panic!("expected status refresh request, got {other:?}"),
    };

    chat.update_account_state(
        /*status_account_display*/ None, /*plan_type*/ None,
        /*has_chatgpt_account*/ true, /*has_codex_backend_auth*/ true,
    );
    chat.finish_status_account_snapshot_refresh(
        request_id,
        Some(live_status_snapshot(
            "stale@example.com",
            /*percent*/ 92.0,
        )),
    );

    assert!(chat.rate_limit_snapshots_by_limit_id.is_empty());
}
