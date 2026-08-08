use super::super::ExecCell;
use super::super::OutputLinesParams;
use super::super::TOOL_CALL_MAX_LINES;
use super::super::output_lines;
use crate::history_cell::HistoryCell;
use ratatui::style::Stylize as _;
use ratatui::text::Line;

pub(in crate::exec_cell) fn compact_display_lines(
    cell: &ExecCell,
    width: u16,
) -> Vec<Line<'static>> {
    if cell
        .calls
        .iter()
        .any(|call| call.is_user_shell_command() || call.is_unified_exec_interaction())
    {
        return cell.display_lines(width);
    }
    if cell.is_active() {
        return if cell.is_exploring_cell() {
            cell.exploring_display_lines(width)
        } else {
            cell.command_display_lines_with_output(width, /*include_output*/ false)
        };
    }
    if cell.calls.iter().all(|call| {
        call.output
            .as_ref()
            .is_some_and(|output| output.exit_code == 0)
    }) {
        return Vec::new();
    }
    if !cell.is_exploring_cell() {
        return cell.display_lines(width);
    }

    let mut lines = cell.exploring_display_lines(width);
    lines[0] = vec!["•".red().bold(), " ".into(), "Exploration failed".bold()].into();
    if let Some(output) = cell
        .calls
        .iter()
        .filter_map(|call| call.output.as_ref())
        .find(|output| output.exit_code != 0)
    {
        lines.extend(
            output_lines(
                Some(output),
                OutputLinesParams {
                    line_limit: TOOL_CALL_MAX_LINES,
                    only_err: true,
                    include_angle_pipe: true,
                    include_prefix: true,
                },
            )
            .lines,
        );
    }
    lines
}

#[cfg(test)]
#[path = "compact_tool_activity_tests.rs"]
mod tests;
