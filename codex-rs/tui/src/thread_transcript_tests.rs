use super::*;
use crate::history_cell::HistoryRenderMode;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

fn command(status: CommandExecutionStatus, cwd: &AbsolutePathBuf) -> ThreadItem {
    ThreadItem::CommandExecution {
        id: "command".to_string(),
        plugin_id: None,
        script_path: None,
        command: "cargo test".to_string(),
        cwd: cwd.clone().into(),
        process_id: None,
        source: CommandExecutionSource::Agent,
        status,
        command_actions: Vec::new(),
        aggregated_output: Some("output".to_string()),
        exit_code: Some(0),
        duration_ms: None,
    }
}

#[test]
fn resumed_tool_cells_keep_full_transcript_while_compact_hides_success() {
    let cwd = AbsolutePathBuf::try_from(std::env::current_dir().expect("current directory"))
        .expect("absolute path");
    let cells = thread_items_to_transcript_cells(
        /*thread_id*/ None,
        &cwd,
        [
            command(CommandExecutionStatus::Completed, &cwd),
            command(CommandExecutionStatus::Failed, &cwd),
            ThreadItem::WebSearch(codex_app_server_protocol::WebSearchItem {
                id: "active-search".to_string(),
                query: String::new(),
                action: None,
                results: None,
            }),
            ThreadItem::WebSearch(codex_app_server_protocol::WebSearchItem {
                id: "completed-search".to_string(),
                query: "rust".to_string(),
                action: None,
                results: Some(Vec::new()),
            }),
        ],
        RawReasoningVisibility::Hidden,
        /*codex_home*/ None,
    );

    let compact_hidden = cells
        .iter()
        .map(|cell| {
            cell.display_lines_for_mode(80, HistoryRenderMode::CompactToolActivity)
                .is_empty()
        })
        .collect::<Vec<_>>();
    assert_eq!(compact_hidden, vec![true, false, false, true]);
    assert!(!cells[0].transcript_lines(80).is_empty());
}
