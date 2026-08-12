use super::ChatWidget;
use crate::exec_cell::ExecCell;
use crate::history_cell::HistoryCell;
use crate::history_cell::HistoryRenderMode;
use crate::history_cell::McpToolCallCell;
use crate::history_cell::WebSearchCell;

impl ChatWidget {
    pub(super) fn ensure_compact_activity_status(&mut self) {
        if self.history_render_mode() == HistoryRenderMode::CompactToolActivity {
            self.bottom_pane.ensure_status_indicator();
        }
    }

    pub(super) fn compact_transient_status(&self) -> Option<String> {
        if self.history_render_mode() != HistoryRenderMode::CompactToolActivity {
            return None;
        }
        if !self.bottom_pane.status_indicator_visible() {
            return None;
        }

        if let Some(hook) = self.active_hook_cell.as_ref()
            && hook.should_render()
        {
            return hook.compact_transient_status();
        }
        self.transcript.active_cell.as_deref().and_then(|cell| {
            (cell
                .as_any()
                .downcast_ref::<ExecCell>()
                .is_some_and(|exec| {
                    exec.is_active()
                        && !exec.iter_calls().any(|call| {
                            call.is_user_shell_command() || call.is_unified_exec_interaction()
                        })
                })
                || cell
                    .as_any()
                    .downcast_ref::<McpToolCallCell>()
                    .is_some_and(McpToolCallCell::is_active)
                || cell
                    .as_any()
                    .downcast_ref::<WebSearchCell>()
                    .is_some_and(WebSearchCell::is_active))
            .then(|| transient_status(cell))
            .flatten()
        })
    }
}

fn transient_status(cell: &dyn HistoryCell) -> Option<String> {
    let lines = cell.display_lines_for_mode(u16::MAX, HistoryRenderMode::CompactToolActivity);
    let line = lines.iter().rev().find(|line| !line.spans.is_empty())?;
    let skip = usize::from(line.spans.get(1).is_some_and(|span| span.content == " ")) * 2;
    let text = line
        .spans
        .iter()
        .skip(skip)
        .map(|span| span.content.as_ref())
        .collect::<String>();
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}
