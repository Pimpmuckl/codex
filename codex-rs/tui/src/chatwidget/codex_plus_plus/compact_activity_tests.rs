use super::*;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use crate::diff_model::FileChange;
use crate::exec_cell::CommandOutput;
use crate::exec_cell::new_active_exec_command;
use crate::history_cell;
use codex_app_server_protocol::WebSearchAction;
use codex_protocol::mcp::CallToolResult;
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

fn text(lines: Vec<Line<'static>>) -> String {
    lines
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn compact_history_policy_keeps_complete_transcript_and_raw_output() {
    let (mut chat, _sender, _events, _operations) = make_chatwidget_manual_with_sender().await;
    chat.config.codex_plus_plus_tool_activity = codex_config::ToolActivityPresentation::Compact;
    chat.on_task_started();

    let mut exec = new_active_exec_command(
        "exec".to_string(),
        vec!["printf one".to_string()],
        Vec::new(),
        CommandExecutionSource::Agent,
        /*interaction_input*/ None,
        /*animations_enabled*/ false,
    );
    exec.complete_call(
        "exec",
        CommandOutput::new(/*exit_code*/ 0, "one\n".to_string()),
        Duration::from_millis(5),
    );
    chat.raw_output_mode = true;
    let exec = chat.compact_history_cell(Box::new(exec));
    chat.raw_output_mode = false;

    let mut mcp = history_cell::new_active_mcp_tool_call(
        "mcp".to_string(),
        history_cell::McpInvocation {
            server: "workspace".to_string(),
            tool: "inspect".to_string(),
            arguments: Some(serde_json::json!({"path": "README.md"})),
        },
        /*animations_enabled*/ false,
    );
    mcp.complete(
        Duration::from_millis(5),
        Ok(CallToolResult {
            content: vec![serde_json::json!({"type": "text", "text": "full result"})],
            structured_content: None,
            is_error: None,
            meta: None,
        }),
    );
    chat.transcript.active_cell = Some(Box::new(mcp));
    chat.retain_fast_compact_completion_status();
    assert_eq!(
        chat.status_state.current_status.header,
        "Called workspace.inspect({\"path\":\"README.md\"})"
    );
    let mcp = chat
        .transcript
        .take_active_cell()
        .expect("completed MCP cell");
    let mcp = chat.compact_history_cell(mcp);
    let web = chat.compact_history_cell(Box::new(history_cell::new_web_search_call(
        "web".to_string(),
        "compact policy".to_string(),
        WebSearchAction::Search {
            query: Some("compact policy".to_string()),
            queries: None,
        },
    )));
    let patch = chat.compact_history_cell(Box::new(history_cell::new_patch_event(
        HashMap::from([(
            PathBuf::from("src/lib.rs"),
            FileChange::Update {
                unified_diff: "@@ -1 +1 @@\n-old\n+new\n".to_string(),
                move_path: None,
            },
        )]),
        &chat.config.cwd,
    )));

    insta::assert_snapshot!(format!(
        "main:\n{}\n{}\n{}\n{}\ntranscript:\n{}\n{}\n{}\n{}",
        text(exec.display_lines(80)),
        text(mcp.display_lines(80)),
        text(web.display_lines(80)),
        text(patch.display_lines(80)),
        text(exec.transcript_lines(80)),
        text(mcp.transcript_lines(80)),
        text(web.transcript_lines(80)),
        text(patch.transcript_lines(80)),
    ), @r###"
    main:



    • Edited src/lib.rs (+1 -1)
    transcript:
    $ 'printf one'
    one
    ✓ • 5ms
    • Called workspace.inspect({"path":"README.md"})
      └ full result
    • Searched the web for compact policy
    • Edited src/lib.rs (+1 -1)
        1 -old
        1 +new
    "###);

    assert_eq!(text(exec.raw_lines()), text(exec.transcript_lines(80)));
}
