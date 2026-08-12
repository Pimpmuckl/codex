use super::*;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use crate::exec_cell::new_active_exec_command;
use codex_app_server_protocol::CommandExecutionSource;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

#[derive(Debug)]
struct CountingHistoryCell {
    desired_height_calls: Arc<AtomicUsize>,
    display_lines_calls: Arc<AtomicUsize>,
    desired_height: u16,
    line_count: usize,
    stable_height: bool,
}

impl HistoryCell for CountingHistoryCell {
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        let frame = self.display_lines_calls.fetch_add(1, Ordering::Relaxed) + 1;
        (0..self.line_count)
            .map(|row| Line::from(format!("frame {frame} row {row}")))
            .collect()
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        Vec::new()
    }

    fn desired_height(&self, _width: u16) -> u16 {
        self.desired_height_calls.fetch_add(1, Ordering::Relaxed);
        self.desired_height
    }

    fn desired_height_for_mode(&self, width: u16, _mode: HistoryRenderMode) -> u16 {
        self.desired_height(width)
    }

    fn has_stable_transcript_height(&self) -> bool {
        self.stable_height
    }
}

async fn widget_with_counting_cell(
    desired_height: u16,
    line_count: usize,
    stable_height: bool,
) -> (ChatWidget, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let (mut widget, _sender, _events, _operations) = make_chatwidget_manual_with_sender().await;
    let desired_height_calls = Arc::new(AtomicUsize::new(0));
    let display_lines_calls = Arc::new(AtomicUsize::new(0));
    widget.transcript.active_cell = Some(Box::new(CountingHistoryCell {
        desired_height_calls: Arc::clone(&desired_height_calls),
        display_lines_calls: Arc::clone(&display_lines_calls),
        desired_height,
        line_count,
        stable_height,
    }));
    (widget, desired_height_calls, display_lines_calls)
}

fn render_frame(widget: &ChatWidget, width: u16) -> Buffer {
    let renderable = widget.as_renderable();
    let height = renderable.desired_height(width);
    let area = Rect::new(/*x*/ 0, /*y*/ 0, width, height);
    let mut buffer = Buffer::empty(area);
    renderable.render(area, &mut buffer);
    buffer
}

fn contains_text(buffer: &Buffer, text: &str) -> bool {
    buffer
        .content
        .chunks(usize::from(buffer.area.width))
        .any(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .contains(text)
        })
}

#[tokio::test]
async fn compact_live_tool_reuses_status_row_without_changing_height() {
    let (mut widget, _sender, _events, _operations) = make_chatwidget_manual_with_sender().await;
    widget.config.codex_plus_plus_tool_activity = codex_config::ToolActivityPresentation::Compact;
    widget.bottom_pane.set_task_running(/*running*/ true);
    widget.bottom_pane.ensure_status_indicator();
    let working = render_frame(&widget, /*width*/ 80);

    widget.transcript.active_cell = Some(Box::new(new_active_exec_command(
        "call-1".to_string(),
        vec!["git".to_string(), "status".to_string()],
        Vec::new(),
        CommandExecutionSource::Agent,
        /*interaction_input*/ None,
        /*animations_enabled*/ false,
    )));
    let active = render_frame(&widget, /*width*/ 80);

    assert_eq!(working.area.height, active.area.height);
    let status_row = active
        .content
        .chunks(usize::from(active.area.width))
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
        })
        .find(|row| row.contains("Running git status"))
        .expect("status row");
    insta::assert_snapshot!(status_row.trim_end(), @"• Running git status (0s • esc to interrupt)");

    widget.transcript.active_cell = Some(Box::new(new_active_exec_command(
        "call-2".to_string(),
        vec!["echo".to_string(), "hello".to_string()],
        Vec::new(),
        CommandExecutionSource::UserShell,
        /*interaction_input*/ None,
        /*animations_enabled*/ false,
    )));
    assert_eq!(widget.compact_transient_status(), None);
}

#[tokio::test]
async fn active_cell_layout_reuses_heights_without_freezing_animation() {
    let (widget, desired_height_calls, display_lines_calls) = widget_with_counting_cell(
        /*desired_height*/ 2, /*line_count*/ 2, /*stable_height*/ true,
    )
    .await;

    let first = render_frame(&widget, /*width*/ 80);
    let second = render_frame(&widget, /*width*/ 80);

    assert_eq!(desired_height_calls.load(Ordering::Relaxed), 1);
    assert_eq!(display_lines_calls.load(Ordering::Relaxed), 2);
    assert!(contains_text(&first, "frame 1 row 0"));
    assert!(contains_text(&second, "frame 2 row 0"));
    let cached = widget
        .transcript
        .active_cell_layout
        .get()
        .expect("active-cell layout should be cached");
    assert_eq!(cached.desired_height, Some(2));
    assert_eq!(cached.rendered_height, Some(2));
}

#[tokio::test]
async fn active_cell_layout_preserves_custom_height_and_bottom_scroll() {
    let (widget, desired_height_calls, _display_lines_calls) = widget_with_counting_cell(
        /*desired_height*/ 1, /*line_count*/ 3, /*stable_height*/ true,
    )
    .await;

    let first = render_frame(&widget, /*width*/ 80);
    let second = render_frame(&widget, /*width*/ 80);

    let mut transcript = second.clone();
    transcript.resize(Rect {
        height: 2,
        ..second.area
    });
    insta::assert_snapshot!(
        "cached_active_cell_bottom_scroll",
        format!("{transcript:?}")
    );
    assert!(contains_text(&first, "frame 1 row 2"));
    assert!(!contains_text(&first, "frame 1 row 0"));
    assert!(contains_text(&second, "frame 2 row 2"));
    assert!(!contains_text(&second, "frame 2 row 0"));
    assert_eq!(desired_height_calls.load(Ordering::Relaxed), 1);
    let cached = widget
        .transcript
        .active_cell_layout
        .get()
        .expect("active-cell layout should be cached");
    assert_eq!(cached.desired_height, Some(1));
    assert_eq!(cached.rendered_height, Some(3));
}

#[tokio::test]
async fn active_cell_layout_invalidates_width_revision_mode_theme_and_identity() {
    let (mut widget, desired_height_calls, _display_lines_calls) = widget_with_counting_cell(
        /*desired_height*/ 1, /*line_count*/ 1, /*stable_height*/ true,
    )
    .await;

    render_frame(&widget, /*width*/ 80);
    render_frame(&widget, /*width*/ 80);
    assert_eq!(desired_height_calls.load(Ordering::Relaxed), 1);

    render_frame(&widget, /*width*/ 81);
    assert_eq!(desired_height_calls.load(Ordering::Relaxed), 2);

    widget.transcript.bump_active_cell_revision();
    render_frame(&widget, /*width*/ 81);
    assert_eq!(desired_height_calls.load(Ordering::Relaxed), 3);

    widget.config.codex_plus_plus_tool_activity = codex_config::ToolActivityPresentation::Compact;
    render_frame(&widget, /*width*/ 81);
    assert_eq!(desired_height_calls.load(Ordering::Relaxed), 4);

    let mut cached = widget
        .transcript
        .active_cell_layout
        .get()
        .expect("active-cell layout should be cached");
    cached.key.syntax_theme_revision = cached.key.syntax_theme_revision.wrapping_sub(1);
    widget.transcript.active_cell_layout.set(Some(cached));
    render_frame(&widget, /*width*/ 81);
    assert_eq!(desired_height_calls.load(Ordering::Relaxed), 5);

    let replacement_height_calls = Arc::new(AtomicUsize::new(0));
    widget.transcript.active_cell = Some(Box::new(CountingHistoryCell {
        desired_height_calls: Arc::clone(&replacement_height_calls),
        display_lines_calls: Arc::new(AtomicUsize::new(0)),
        desired_height: 1,
        line_count: 1,
        stable_height: true,
    }));
    render_frame(&widget, /*width*/ 81);
    assert_eq!(replacement_height_calls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn externally_mutable_active_cells_bypass_persistent_layout_cache() {
    let (widget, desired_height_calls, display_lines_calls) = widget_with_counting_cell(
        /*desired_height*/ 2, /*line_count*/ 2, /*stable_height*/ false,
    )
    .await;

    render_frame(&widget, /*width*/ 80);
    render_frame(&widget, /*width*/ 80);

    assert_eq!(desired_height_calls.load(Ordering::Relaxed), 2);
    assert_eq!(display_lines_calls.load(Ordering::Relaxed), 2);
    assert_eq!(widget.transcript.active_cell_layout.get(), None);
}

#[tokio::test]
async fn removing_active_cell_invalidates_layout_before_reusing_its_identity() {
    let (mut widget, desired_height_calls, _display_lines_calls) = widget_with_counting_cell(
        /*desired_height*/ 2, /*line_count*/ 2, /*stable_height*/ true,
    )
    .await;

    render_frame(&widget, /*width*/ 80);
    let revision = widget.transcript.active_cell_revision;
    let active_cell = widget
        .transcript
        .take_active_cell()
        .expect("active cell should exist");

    assert_eq!(widget.transcript.active_cell_layout.get(), None);
    widget.transcript.active_cell = Some(active_cell);
    assert_eq!(widget.transcript.active_cell_revision, revision);

    render_frame(&widget, /*width*/ 80);
    assert_eq!(desired_height_calls.load(Ordering::Relaxed), 2);
}
