use super::*;
use pretty_assertions::assert_eq;

fn account_candidate(id: &str, automation_enabled: bool) -> AccountCandidate {
    AccountCandidate {
        id: serde_json::from_value(id.into()).expect("account id"),
        display_label: format!("{id}@example.com"),
        priority: 0,
        enabled: true,
        automation_enabled,
        usage_limit_resets_at: None,
        blocked: false,
    }
}

fn picker_candidate(id: &str, is_current: bool) -> account_picker::AccountPickerCandidate {
    account_picker::AccountPickerCandidate {
        id: id.to_string(),
        email: format!("{id}@example.com"),
        primary_window_label: "5h".to_string(),
        five_hour_reset: None,
        five_hour_usage_left_percent: Some(100),
        five_hour_exhausted: false,
        weekly_reset: None,
        weekly_usage_left_percent: Some(100),
        weekly_exhausted: false,
        blocked_until: None,
        blocked: false,
        in_use: false,
        is_current,
        is_default: false,
    }
}

#[test]
fn automatic_default_preserves_automation_disabled_current_account() {
    let current = account_candidate("acct_current", /*automation_enabled*/ false);
    let alternative = account_candidate("acct_alternative", /*automation_enabled*/ true);
    let candidates = vec![current.clone(), alternative];
    let picker_candidates = vec![
        picker_candidate("acct_current", /*is_current*/ true),
        picker_candidate("acct_alternative", /*is_current*/ false),
    ];

    assert_eq!(
        automatic_default_index(&candidates, &picker_candidates, Some(&current.id)),
        Some(0)
    );
}

#[test]
fn automatic_default_ignores_automation_disabled_alternative() {
    let disabled = account_candidate("acct_disabled", /*automation_enabled*/ false);
    let enabled = account_candidate("acct_enabled", /*automation_enabled*/ true);
    let candidates = vec![disabled, enabled.clone()];
    let picker_candidates = vec![
        picker_candidate("acct_disabled", /*is_current*/ false),
        picker_candidate("acct_enabled", /*is_current*/ false),
    ];

    assert_eq!(
        automatic_default_index(&candidates, &picker_candidates, Some(&enabled.id)),
        Some(1)
    );
}

#[test]
fn automatic_default_avoids_unavailable_disabled_current_account() {
    let current = account_candidate("acct_current", /*automation_enabled*/ false);
    let alternative = account_candidate("acct_alternative", /*automation_enabled*/ true);
    let candidates = vec![current.clone(), alternative];
    let mut picker_candidates = vec![
        picker_candidate("acct_current", /*is_current*/ true),
        picker_candidate("acct_alternative", /*is_current*/ false),
    ];
    picker_candidates[0].five_hour_exhausted = true;

    assert_eq!(
        automatic_default_index(&candidates, &picker_candidates, Some(&current.id)),
        Some(1)
    );
}
