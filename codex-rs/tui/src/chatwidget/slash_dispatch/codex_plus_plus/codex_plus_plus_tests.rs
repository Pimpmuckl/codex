use assert_matches::assert_matches;
use crossterm::event::KeyEvent;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use tokio::sync::mpsc::unbounded_channel;

use super::*;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPaneView;
use crate::bottom_pane::ListSelectionView;
use crate::keymap::RuntimeKeymap;
use crate::test_backend::VT100Backend;

fn settings_view(
    current: AutomaticAccountSelection,
) -> (
    ListSelectionView,
    tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) {
    let (tx, rx) = unbounded_channel();
    let keymap = settings_list_keymap(RuntimeKeymap::defaults().list);
    (
        ListSelectionView::new(
            codex_plus_plus_settings_params(current, &keymap),
            AppEventSender::new(tx),
            keymap,
        ),
        rx,
    )
}

fn render_settings(current: AutomaticAccountSelection) -> String {
    let (view, _rx) = settings_view(current);
    let mut terminal =
        Terminal::new(VT100Backend::new(/*width*/ 84, /*height*/ 10)).expect("terminal");
    terminal
        .draw(|frame| view.render(frame.area(), frame.buffer_mut()))
        .expect("render Codex++ settings");
    terminal.backend().to_string()
}

#[test]
fn settings_enabled_snapshot() {
    insta::assert_snapshot!(
        "codex_plus_plus_settings_enabled",
        render_settings(AutomaticAccountSelection::Enabled)
    );
}

#[test]
fn settings_disabled_snapshot() {
    insta::assert_snapshot!(
        "codex_plus_plus_settings_disabled",
        render_settings(AutomaticAccountSelection::Disabled)
    );
}

#[test]
fn enter_saves_full_selection() {
    let (mut view, mut rx) = settings_view(AutomaticAccountSelection::Enabled);

    view.handle_key_event(KeyEvent::from(KeyCode::Char(' ')));
    view.handle_key_event(KeyEvent::from(KeyCode::Enter));

    assert_matches!(
        rx.try_recv(),
        Ok(AppEvent::PersistAutomaticAccountSelection {
            selection: AutomaticAccountSelection::Disabled,
        })
    );
}

#[test]
fn escape_cancels_without_writing() {
    let (mut view, mut rx) = settings_view(AutomaticAccountSelection::Enabled);

    view.handle_key_event(KeyEvent::from(KeyCode::Char(' ')));
    view.handle_key_event(KeyEvent::from(KeyCode::Esc));

    assert_matches!(rx.try_recv(), Err(_));
}

#[test]
fn settings_hint_uses_list_keymap() {
    let mut keymap = RuntimeKeymap::defaults().list;
    keymap.accept = vec![key_hint::plain(KeyCode::Char(' '))];
    keymap.cancel = vec![key_hint::plain(KeyCode::Char('q'))];
    let keymap = settings_list_keymap(keymap);

    let text = settings_hint_line(&keymap)
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert_eq!(text, "Press space to toggle; enter to save; q to cancel");
}

#[test]
fn persistence_messages_snapshot() {
    insta::assert_snapshot!(
        "codex_plus_plus_persistence_messages",
        [
            persistence_success_message(AutomaticAccountSelection::Disabled),
            persistence_error_message("permission denied".to_string()),
            persistence_overridden_message(AutomaticAccountSelection::Enabled),
            persistence_verification_failed_message("connection closed".to_string()),
        ]
        .join("\n")
    );
}
