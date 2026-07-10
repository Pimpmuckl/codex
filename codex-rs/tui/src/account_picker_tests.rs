use super::*;
use crate::test_backend::VT100Backend;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::Terminal;

fn candidates() -> Vec<AccountPickerCandidate> {
    vec![
        AccountPickerCandidate {
            id: "acct_a".to_string(),
            email: "first@example.com".to_string(),
            weekly_reset: Some("Jul 14".to_string()),
            usage_left_percent: Some(12),
            blocked: false,
            in_use: true,
            is_current: true,
            is_default: false,
        },
        AccountPickerCandidate {
            id: "acct_b".to_string(),
            email: "best@example.com".to_string(),
            weekly_reset: None,
            usage_left_percent: Some(84),
            blocked: false,
            in_use: false,
            is_current: false,
            is_default: true,
        },
        AccountPickerCandidate {
            id: "acct_c".to_string(),
            email: "unknown@example.com".to_string(),
            weekly_reset: None,
            usage_left_percent: None,
            blocked: false,
            in_use: false,
            is_current: false,
            is_default: false,
        },
    ]
}

#[test]
fn account_picker_snapshot() {
    let view = new_view(
        &candidates(),
        /*selected_idx*/ 1,
        /*seconds_remaining*/ 15,
    );
    let mut terminal =
        Terminal::new(VT100Backend::new(/*width*/ 100, /*height*/ 10)).expect("terminal");
    terminal
        .draw(|frame| view.render(frame.area(), frame.buffer_mut()))
        .expect("render account picker");

    insta::assert_snapshot!("account_picker_startup", terminal.backend());
}

#[test]
fn default_candidate_prefers_backend_marked_default() {
    assert_eq!(
        default_candidate(&candidates()).map(|candidate| candidate.id.as_str()),
        Some("acct_b")
    );
}

#[test]
fn recommendation_avoids_accounts_used_by_other_sessions() {
    assert_eq!(recommended_candidate_index(&candidates()), 1);
}

#[test]
fn enter_selects_highlighted_candidate() {
    let rows = candidates();
    let mut view = new_view(
        &rows, /*selected_idx*/ 1, /*seconds_remaining*/ 15,
    );

    view.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(view.is_complete());
    assert_eq!(view.take_last_selected_index(), Some(1));
}

#[test]
fn row_description_uses_unknown_for_missing_usage_data() {
    let item = selection_item(&candidates()[2]);

    assert_eq!(
        item.description.as_deref(),
        Some("Weekly Reset: unknown    Usage left: unknown")
    );
}
