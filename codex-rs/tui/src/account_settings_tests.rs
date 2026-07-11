use assert_matches::assert_matches;
use codex_config::types::AutomaticAccountSelection;
use ratatui::Terminal;
use tokio::sync::mpsc::unbounded_channel;

use super::*;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::ListSelectionView;
use crate::keymap::RuntimeKeymap;
use crate::test_backend::VT100Backend;

fn render_settings(current: AutomaticAccountSelection) -> String {
    let (tx, _rx) = unbounded_channel();
    let view = ListSelectionView::new(
        account_settings_params(current),
        AppEventSender::new(tx),
        RuntimeKeymap::defaults().list,
    );
    let mut terminal =
        Terminal::new(VT100Backend::new(/*width*/ 84, /*height*/ 12)).expect("terminal");
    terminal
        .draw(|frame| view.render(frame.area(), frame.buffer_mut()))
        .expect("render account settings");
    terminal.backend().to_string()
}

#[test]
fn account_settings_enabled_snapshot() {
    insta::assert_snapshot!(
        "account_settings_enabled",
        render_settings(AutomaticAccountSelection::Enabled)
    );
}

#[test]
fn account_settings_disabled_snapshot() {
    insta::assert_snapshot!(
        "account_settings_disabled",
        render_settings(AutomaticAccountSelection::Disabled)
    );
}

#[test]
fn account_settings_toggle_success_snapshot() {
    let success = persistence_success_message(AutomaticAccountSelection::Disabled);
    insta::assert_snapshot!("account_settings_toggle_success", success);
}

#[test]
fn account_settings_toggle_failure_snapshot() {
    let error = persistence_error_message("permission denied".to_string());
    insta::assert_snapshot!("account_settings_toggle_failure", error);
}

#[test]
fn account_settings_toggle_overridden_snapshot() {
    let message = persistence_overridden_message(AutomaticAccountSelection::Enabled);
    insta::assert_snapshot!("account_settings_toggle_overridden", message);
}

#[test]
fn account_settings_toggle_verification_failure_snapshot() {
    let message = persistence_verification_failed_message("connection closed".to_string());
    insta::assert_snapshot!("account_settings_toggle_verification_failure", message);
}

#[test]
fn selection_dispatches_persistence_event() {
    let mut params = account_settings_params(AutomaticAccountSelection::Enabled);
    let action = params.items[1].actions.pop().expect("selection action");
    let (tx, mut rx) = unbounded_channel();

    action(&AppEventSender::new(tx));

    assert_matches!(
        rx.try_recv(),
        Ok(AppEvent::PersistAutomaticAccountSelection {
            selection: AutomaticAccountSelection::Disabled,
        })
    );
}
