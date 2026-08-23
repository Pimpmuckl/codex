use super::ChatWidget;
use super::transient_status;
use crate::exec_cell::ExecCell;
use crate::history_cell::HistoryCell;
use crate::history_cell::McpToolCallCell;
use crate::history_cell::PatchHistoryCell;
use crate::history_cell::WebSearchCell;
use crate::terminal_hyperlinks::HyperlinkLine;
use codex_app_server_protocol::CommandExecutionSource;
use ratatui::text::Line;

#[derive(Debug)]
enum CompactMainPresentation {
    Hidden,
    FirstLine,
}

#[derive(Debug)]
struct CompactActivityHistoryCell {
    inner: Box<dyn HistoryCell>,
    main: CompactMainPresentation,
}

impl HistoryCell for CompactActivityHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        match self.main {
            CompactMainPresentation::Hidden => Vec::new(),
            CompactMainPresentation::FirstLine => self
                .inner
                .display_lines(width)
                .into_iter()
                .take(1)
                .collect(),
        }
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.inner.raw_lines()
    }

    fn display_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        match self.main {
            CompactMainPresentation::Hidden => Vec::new(),
            CompactMainPresentation::FirstLine => self
                .inner
                .display_hyperlink_lines(width)
                .into_iter()
                .take(1)
                .collect(),
        }
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.inner.transcript_lines(width)
    }

    fn transcript_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        self.inner.transcript_hyperlink_lines(width)
    }
}

impl ChatWidget {
    pub(in crate::chatwidget) fn compact_history_cell(
        &self,
        cell: Box<dyn HistoryCell>,
    ) -> Box<dyn HistoryCell> {
        if !self.compact_tool_activity_enabled() {
            return cell;
        }
        let main = if cell.as_any().is::<PatchHistoryCell>() {
            Some(CompactMainPresentation::FirstLine)
        } else if compact_success_is_transcript_only(cell.as_ref()) {
            Some(CompactMainPresentation::Hidden)
        } else {
            None
        };
        if let Some(main) = main {
            Box::new(CompactActivityHistoryCell { inner: cell, main })
        } else {
            cell
        }
    }

    pub(in crate::chatwidget) fn retain_fast_compact_completion_status(&mut self) {
        let status = self
            .compact_tool_activity_enabled()
            .then(|| self.transcript.active_cell.as_deref())
            .flatten()
            .filter(|cell| compact_success_is_transcript_only(*cell))
            .and_then(transient_status);
        if let Some(status) = status {
            self.set_status_header(status);
        }
    }
}

fn compact_success_is_transcript_only(cell: &dyn HistoryCell) -> bool {
    cell.as_any()
        .downcast_ref::<ExecCell>()
        .is_some_and(|exec| {
            !exec.is_active()
                && exec.iter_calls().all(|call| {
                    matches!(
                        call.source,
                        CommandExecutionSource::Agent | CommandExecutionSource::UnifiedExecStartup
                    ) && call
                        .output
                        .as_ref()
                        .is_some_and(|output| output.exit_code == 0)
                })
        })
        || cell
            .as_any()
            .downcast_ref::<McpToolCallCell>()
            .is_some_and(|mcp| mcp.success() == Some(true))
        || cell
            .as_any()
            .downcast_ref::<WebSearchCell>()
            .is_some_and(WebSearchCell::is_completed)
}

#[cfg(test)]
#[path = "compact_activity_tests.rs"]
mod tests;
