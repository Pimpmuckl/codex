use crate::custom_terminal::Terminal;
use ratatui::backend::Backend;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use std::io;
use std::io::Write;

/// Return visible history to the space released by a contracting bottom-aligned viewport.
pub(in crate::tui) fn restore_history_on_contraction<B>(
    terminal: &mut Terminal<B>,
    previous: Rect,
    next: Rect,
) -> io::Result<Position>
where
    B: Backend<Error = io::Error> + Write,
{
    if next.y > previous.y && next.height < previous.height {
        terminal.clear_after_position(Position::new(/*x*/ 0, previous.y))?;
        terminal
            .backend_mut()
            .scroll_region_down(0..next.y, next.y - previous.y)?;
        return Ok(Position::new(/*x*/ 0, next.y));
    }
    Ok(Position::new(/*x*/ 0, previous.y.min(next.y)))
}

#[cfg(test)]
#[path = "viewport_tests.rs"]
mod tests;
