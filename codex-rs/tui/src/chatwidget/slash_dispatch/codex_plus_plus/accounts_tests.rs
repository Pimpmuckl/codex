use assert_matches::assert_matches;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::style::Modifier;
use tokio::sync::mpsc::unbounded_channel;

use super::*;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPaneView;
use crate::keymap::RuntimeKeymap;
use crate::test_backend::VT100Backend;

fn account_id(id: &str) -> AccountId {
    serde_json::from_str(&format!("\"{id}\"")).expect("account id")
}

fn account_row(
    id: &str,
    label: &str,
    enabled: bool,
    automation_enabled: bool,
    login_required: bool,
    is_current: bool,
    in_use: bool,
    weekly_status: Option<WeeklyWindowStatus>,
) -> AccountAutomationRow {
    AccountAutomationRow {
        id: account_id(id),
        label: label.to_string(),
        enabled,
        automation_enabled,
        login_required,
        is_current,
        in_use,
        weekly_status,
    }
}

fn accounts_view(
    rows: Vec<AccountAutomationRow>,
    codex_home: PathBuf,
) -> (
    ListSelectionView,
    tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) {
    let (tx, rx) = unbounded_channel();
    let keymap = settings_list_keymap(RuntimeKeymap::defaults().list);
    (
        ListSelectionView::new(
            accounts_settings_params(rows, codex_home, &keymap),
            AppEventSender::new(tx),
            keymap,
        ),
        rx,
    )
}

#[test]
fn mixed_account_automation_snapshot() {
    let rows = vec![
        account_row(
            "acct_1",
            "one@example.com",
            true,
            true,
            false,
            true,
            true,
            Some(WeeklyWindowStatus::Waiting(Some(100))),
        ),
        account_row(
            "acct_2",
            "two@example.com",
            true,
            false,
            true,
            false,
            false,
            Some(WeeklyWindowStatus::Started(Some(80))),
        ),
        account_row(
            "acct_3",
            "three@example.com",
            true,
            true,
            true,
            false,
            true,
            Some(WeeklyWindowStatus::Retrying(Some(9))),
        ),
        account_row(
            "acct_4",
            "four@example.com",
            false,
            true,
            false,
            false,
            false,
            Some(WeeklyWindowStatus::SignInRequired),
        ),
    ];
    let (view, _rx) = accounts_view(rows, PathBuf::new());
    let mut terminal =
        Terminal::new(VT100Backend::new(/*width*/ 92, /*height*/ 12)).expect("terminal");
    terminal
        .draw(|frame| {
            view.render(frame.area(), frame.buffer_mut());
            let status = &frame.buffer_mut()[(36, 5)];
            assert_eq!(status.symbol(), "L");
            assert!(status.style().add_modifier.contains(Modifier::DIM));
        })
        .expect("render accounts settings");

    insta::assert_snapshot!(
        "codex_plus_plus_accounts_mixed_automation",
        terminal.backend().to_string()
    );
}

#[test]
fn space_stages_and_escape_cancels_without_writing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let rows = vec![account_row(
        "acct_missing",
        "missing@example.com",
        true,
        true,
        false,
        false,
        false,
        None,
    )];
    let (mut view, mut rx) = accounts_view(rows, temp.path().to_path_buf());

    view.handle_key_event(KeyEvent::from(KeyCode::Char(' ')));
    view.handle_key_event(KeyEvent::from(KeyCode::Esc));

    assert_matches!(rx.try_recv(), Err(_));
}

#[tokio::test]
async fn enter_surfaces_save_failure_snapshot() {
    let temp = tempfile::tempdir().expect("temp dir");
    let rows = vec![account_row(
        "acct_missing",
        "missing@example.com",
        true,
        true,
        false,
        false,
        false,
        None,
    )];
    let (mut view, mut rx) = accounts_view(rows, temp.path().to_path_buf());

    view.handle_key_event(KeyEvent::from(KeyCode::Char(' ')));
    view.handle_key_event(KeyEvent::from(KeyCode::Enter));

    let cell = match tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv()).await {
        Ok(Some(AppEvent::InsertHistoryCell(cell))) => cell,
        _ => panic!("expected save result"),
    };
    insta::assert_snapshot!(
        "codex_plus_plus_accounts_save_failure",
        cell.display_lines(/*width*/ 96)
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn unchanged_accounts_are_not_written() {
    let temp = tempfile::tempdir().expect("temp dir");
    let choices = vec![Arc::new(AccountAutomationChoice {
        id: account_id("acct_missing"),
        initial_automation_enabled: true,
        automation_enabled: AtomicBool::new(true),
    })];

    assert_eq!(
        persist_account_automation(&AccountStore::new(temp.path().to_path_buf()), &choices),
        Ok(())
    );
}

#[test]
fn account_status_is_concise() {
    assert_eq!(account_status(true, false, false, None), None);
    assert_eq!(
        account_status(true, true, false, None).as_deref(),
        Some("Login required")
    );
    assert_eq!(
        account_status(true, false, true, None).as_deref(),
        Some("In use")
    );
    assert_eq!(
        account_status(true, true, true, None).as_deref(),
        Some("Login required · In use")
    );
    assert_eq!(
        account_status(false, true, true, None).as_deref(),
        Some("Account disabled · Login required · In use")
    );
}
