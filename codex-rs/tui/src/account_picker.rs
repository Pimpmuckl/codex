use std::time::Duration;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPaneView;
use crate::bottom_pane::ColumnWidthMode;
use crate::bottom_pane::ListSelectionView;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionRowDisplay;
use crate::bottom_pane::SelectionViewParams;
use crate::keymap::RuntimeKeymap;
use crate::render::renderable::Renderable;
use crate::tui::Tui;
use crate::tui::TuiEvent;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::style::Stylize as _;
use ratatui::text::Line;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_stream::StreamExt;

const STARTUP_AUTO_PICK_AFTER: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AccountPickerCandidate {
    pub(crate) id: String,
    pub(crate) email: String,
    pub(crate) weekly_reset: Option<String>,
    pub(crate) usage_left_percent: Option<u8>,
    pub(crate) is_default: bool,
}

pub(crate) async fn run_startup_account_picker(
    tui: &mut Tui,
    candidates: Vec<AccountPickerCandidate>,
) -> Result<Option<String>> {
    if candidates.len() <= 1 {
        return Ok(default_candidate(&candidates).map(|candidate| candidate.id.clone()));
    }

    let default_idx = default_candidate_index(&candidates);
    let deadline = Instant::now() + STARTUP_AUTO_PICK_AFTER;
    let mut view = new_view(&candidates, default_idx, STARTUP_AUTO_PICK_AFTER.as_secs());
    draw_view(tui, &view)?;

    let mut events = tui.event_stream();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => {
                return Ok(Some(candidates[default_idx].id.clone()));
            }
            _ = tick.tick() => {
                let remaining = deadline.saturating_duration_since(Instant::now()).as_secs();
                let selected_idx = view.selected_index().unwrap_or(default_idx);
                view = new_view(&candidates, selected_idx, remaining);
                draw_view(tui, &view)?;
            }
            event = events.next() => {
                let Some(event) = event else {
                    return Ok(Some(candidates[default_idx].id.clone()));
                };
                match event {
                    TuiEvent::Key(key) => {
                        if key.kind == KeyEventKind::Release {
                            continue;
                        }
                        if key.modifiers.contains(KeyModifiers::CONTROL)
                            && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('d'))
                        {
                            return Ok(None);
                        }
                        view.handle_key_event(key);
                        if view.is_complete() {
                            return Ok(view
                                .take_last_selected_index()
                                .and_then(|idx| candidates.get(idx))
                                .map(|candidate| candidate.id.clone()));
                        }
                        draw_view(tui, &view)?;
                    }
                    TuiEvent::Paste(_) => {}
                    TuiEvent::Draw | TuiEvent::Resize => draw_view(tui, &view)?,
                }
            }
        }
    }
}

pub(crate) fn default_candidate(
    candidates: &[AccountPickerCandidate],
) -> Option<&AccountPickerCandidate> {
    candidates
        .get(default_candidate_index(candidates))
        .or_else(|| candidates.first())
}

fn default_candidate_index(candidates: &[AccountPickerCandidate]) -> usize {
    candidates
        .iter()
        .position(|candidate| candidate.is_default)
        .unwrap_or(0)
}

fn new_view(
    candidates: &[AccountPickerCandidate],
    selected_idx: usize,
    seconds_remaining: u64,
) -> ListSelectionView {
    let (tx, _rx) = mpsc::unbounded_channel::<AppEvent>();
    ListSelectionView::new(
        selection_params(candidates, selected_idx, seconds_remaining),
        AppEventSender::new(tx),
        RuntimeKeymap::defaults().list,
    )
}

fn selection_params(
    candidates: &[AccountPickerCandidate],
    selected_idx: usize,
    seconds_remaining: u64,
) -> SelectionViewParams {
    SelectionViewParams {
        title: Some("Choose account".to_string()),
        footer_note: Some(Line::from(vec![
            "Auto-selects in ".dim(),
            seconds_remaining.to_string().bold(),
            "s".dim(),
        ])),
        items: candidates.iter().map(selection_item).collect(),
        initial_selected_idx: Some(selected_idx.min(candidates.len().saturating_sub(1))),
        is_searchable: false,
        col_width_mode: ColumnWidthMode::AutoAllRows,
        row_display: SelectionRowDisplay::SingleLine,
        ..Default::default()
    }
}

fn selection_item(candidate: &AccountPickerCandidate) -> SelectionItem {
    SelectionItem {
        name: candidate.email.clone(),
        description: Some(format!(
            "Weekly Reset: {}    Usage left: {}",
            candidate.weekly_reset.as_deref().unwrap_or("unknown"),
            usage_left(candidate.usage_left_percent)
        )),
        is_default: candidate.is_default,
        dismiss_on_select: true,
        search_value: Some(candidate.email.clone()),
        ..Default::default()
    }
}

fn usage_left(percent: Option<u8>) -> String {
    percent.map_or_else(|| "unknown".to_string(), |percent| format!("{percent}%"))
}

fn draw_view(tui: &mut Tui, view: &ListSelectionView) -> Result<()> {
    tui.draw(u16::MAX, |frame| {
        view.render(frame.area(), frame.buffer_mut());
    })?;
    Ok(())
}

#[cfg(test)]
#[path = "account_picker_tests.rs"]
mod tests;
