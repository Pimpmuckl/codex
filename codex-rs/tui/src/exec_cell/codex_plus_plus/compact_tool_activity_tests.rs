use super::*;
use crate::exec_cell::CommandOutput;
use crate::exec_cell::new_active_exec_command;
use crate::history_cell::HistoryCell;
use crate::history_cell::HistoryRenderMode;
use codex_app_server_protocol::CommandExecutionSource;
use codex_protocol::parse_command::ParsedCommand;
use std::time::Duration;

fn rendered(cell: &ExecCell) -> String {
    cell.display_lines_for_mode(80, HistoryRenderMode::CompactToolActivity)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn compact_tool_activity_preserves_failures_shell_and_transcript() {
    let mut routine = new_active_exec_command(
        "routine".to_string(),
        vec!["cargo".into(), "check".into()],
        Vec::new(),
        CommandExecutionSource::Agent,
        /*interaction_input*/ None,
        /*animations_enabled*/ false,
    );
    routine.append_output("routine", "streamed output");
    insta::assert_snapshot!(rendered(&routine), @"• Running cargo check");

    routine.complete_call(
        "routine",
        CommandOutput::new(/*exit_code*/ 0, "final output".to_string()),
        Duration::from_millis(1),
    );
    assert!(rendered(&routine).is_empty());
    assert!(
        routine
            .transcript_lines(80)
            .iter()
            .any(|line| line.to_string().contains("final output"))
    );

    let mut failed = new_active_exec_command(
        "failed".to_string(),
        vec!["cat".into(), "missing".into()],
        vec![ParsedCommand::Read {
            name: "missing".to_string(),
            cmd: "cat missing".to_string(),
            path: "missing".into(),
        }],
        CommandExecutionSource::Agent,
        /*interaction_input*/ None,
        /*animations_enabled*/ false,
    );
    failed.complete_call(
        "failed",
        CommandOutput::new(/*exit_code*/ 1, "file not found".to_string()),
        Duration::from_millis(1),
    );
    insta::assert_snapshot!(rendered(&failed), @r###"
    • Exploration failed
      └ Read missing
      └ file not found
    "###);

    failed.calls[0].source = CommandExecutionSource::UserShell;
    failed.calls[0].output = Some(CommandOutput::new(/*exit_code*/ 0, "kept".to_string()));
    assert!(rendered(&failed).contains("kept"));
    failed.calls[0].source = CommandExecutionSource::UnifiedExecInteraction;
    assert!(!rendered(&failed).is_empty());
}
