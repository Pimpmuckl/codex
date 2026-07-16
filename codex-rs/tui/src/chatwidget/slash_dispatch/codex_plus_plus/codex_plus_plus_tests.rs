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
    automatic: AutomaticAccountSelection,
    weekly: WeeklyUsageWindowAutoStart,
    capacity: ModelCapacityRetryMode,
) -> (
    ListSelectionView,
    tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) {
    let (tx, rx) = unbounded_channel();
    let keymap = settings_list_keymap(RuntimeKeymap::defaults().list);
    let weekly_supported = automatic == AutomaticAccountSelection::Enabled;
    (
        ListSelectionView::new(
            codex_plus_plus_settings_params(
                automatic,
                weekly,
                capacity,
                /*current_user_message_inbox*/ false,
                weekly_supported,
                &keymap,
            ),
            AppEventSender::new(tx),
            keymap,
        ),
        rx,
    )
}

fn render_settings(
    automatic: AutomaticAccountSelection,
    weekly: WeeklyUsageWindowAutoStart,
    capacity: ModelCapacityRetryMode,
) -> String {
    let (view, _rx) = settings_view(automatic, weekly, capacity);
    let mut terminal =
        Terminal::new(VT100Backend::new(/*width*/ 84, /*height*/ 14)).expect("terminal");
    terminal
        .draw(|frame| view.render(frame.area(), frame.buffer_mut()))
        .expect("render Codex++ settings");
    terminal.backend().to_string()
}

#[test]
fn settings_enabled_snapshot() {
    insta::assert_snapshot!(
        "codex_plus_plus_settings_enabled",
        render_settings(
            AutomaticAccountSelection::Enabled,
            WeeklyUsageWindowAutoStart::Enabled,
            ModelCapacityRetryMode::Bounded,
        )
    );
}

#[test]
fn settings_unsupported_snapshot() {
    insta::assert_snapshot!(
        "codex_plus_plus_settings_unsupported",
        render_settings(
            AutomaticAccountSelection::Disabled,
            WeeklyUsageWindowAutoStart::Disabled,
            ModelCapacityRetryMode::Indefinite,
        )
    );
}

#[test]
fn unsupported_settings_save_only_the_visible_settings() {
    let (mut view, mut rx) = settings_view(
        AutomaticAccountSelection::Disabled,
        WeeklyUsageWindowAutoStart::Enabled,
        ModelCapacityRetryMode::Bounded,
    );

    view.handle_key_event(KeyEvent::from(KeyCode::Char(' ')));
    view.handle_key_event(KeyEvent::from(KeyCode::Enter));

    assert_matches!(
        rx.try_recv(),
        Ok(AppEvent::PersistCodexPlusPlusSettings {
            automatic_account_selection: AutomaticAccountSelection::Enabled,
            weekly_usage_window_auto_start: None,
            model_capacity_retry_mode: ModelCapacityRetryMode::Bounded,
            user_message_inbox: UserMessageInbox::Disabled,
        })
    );
}

#[test]
fn weekly_setting_saves_full_selection() {
    let (mut view, mut rx) = settings_view(
        AutomaticAccountSelection::Enabled,
        WeeklyUsageWindowAutoStart::Enabled,
        ModelCapacityRetryMode::Bounded,
    );

    view.handle_key_event(KeyEvent::from(KeyCode::Down));
    view.handle_key_event(KeyEvent::from(KeyCode::Char(' ')));
    view.handle_key_event(KeyEvent::from(KeyCode::Enter));

    assert_matches!(
        rx.try_recv(),
        Ok(AppEvent::PersistCodexPlusPlusSettings {
            automatic_account_selection: AutomaticAccountSelection::Enabled,
            weekly_usage_window_auto_start: Some(WeeklyUsageWindowAutoStart::Disabled),
            model_capacity_retry_mode: ModelCapacityRetryMode::Bounded,
            user_message_inbox: UserMessageInbox::Disabled,
        })
    );
}

#[test]
fn capacity_setting_saves_indefinite_mode() {
    let (mut view, mut rx) = settings_view(
        AutomaticAccountSelection::Enabled,
        WeeklyUsageWindowAutoStart::Enabled,
        ModelCapacityRetryMode::Bounded,
    );

    view.handle_key_event(KeyEvent::from(KeyCode::Down));
    view.handle_key_event(KeyEvent::from(KeyCode::Down));
    view.handle_key_event(KeyEvent::from(KeyCode::Char(' ')));
    view.handle_key_event(KeyEvent::from(KeyCode::Down));
    view.handle_key_event(KeyEvent::from(KeyCode::Char(' ')));
    view.handle_key_event(KeyEvent::from(KeyCode::Enter));

    assert_matches!(
        rx.try_recv(),
        Ok(AppEvent::PersistCodexPlusPlusSettings {
            automatic_account_selection: AutomaticAccountSelection::Enabled,
            weekly_usage_window_auto_start: Some(WeeklyUsageWindowAutoStart::Enabled),
            model_capacity_retry_mode: ModelCapacityRetryMode::Indefinite,
            user_message_inbox: UserMessageInbox::Enabled,
        })
    );
}

#[test]
fn escape_cancels_without_writing() {
    let (mut view, mut rx) = settings_view(
        AutomaticAccountSelection::Enabled,
        WeeklyUsageWindowAutoStart::Enabled,
        ModelCapacityRetryMode::Bounded,
    );

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
            persistence_success_message(),
            persistence_error_message("permission denied".to_string()),
            persistence_overridden_message(),
            persistence_verification_failed_message("connection closed".to_string()),
        ]
        .join("\n")
    );
}
