use super::*;
use codex_protocol::auth::RefreshTokenFailedError;
use codex_protocol::protocol::RateLimitWindow;

#[test]
fn maps_five_hour_and_weekly_windows_to_picker_usage() {
    let response = RateLimitsWithResetCredits {
        rate_limits: vec![RateLimitSnapshot {
            limit_id: Some("codex".to_string()),
            limit_name: None,
            primary: Some(RateLimitWindow {
                used_percent: 67.6,
                window_minutes: Some(300),
                resets_at: Some(1_749_950_000),
            }),
            secondary: Some(RateLimitWindow {
                used_percent: 12.4,
                window_minutes: Some(10_080),
                resets_at: Some(1_750_000_000),
            }),
            credits: None,
            individual_limit: None,
            plan_type: None,
            rate_limit_reached_type: None,
        }],
        rate_limit_reset_credits: None,
    };

    assert_eq!(
        account_usage(&response),
        AccountUsage {
            primary_window_minutes: Some(300),
            five_hour_reset_at: Some(1_749_950_000),
            five_hour_remaining_percent: Some(32),
            five_hour_exhausted: false,
            weekly_reset_at: Some(1_750_000_000),
            weekly_remaining_percent: Some(88),
            weekly_exhausted: false,
        }
    );
}

#[test]
fn exhausted_until_uses_the_later_exhausted_window_reset() {
    let usage = AccountUsage {
        primary_window_minutes: Some(300),
        five_hour_reset_at: Some(1_749_950_000),
        five_hour_remaining_percent: Some(0),
        five_hour_exhausted: true,
        weekly_reset_at: Some(1_750_000_000),
        weekly_remaining_percent: Some(0),
        weekly_exhausted: true,
    };

    assert_eq!(usage.exhausted_until(), Some(1_750_000_000));
}

#[test]
fn rounded_zero_remaining_does_not_mark_window_exhausted() {
    let response = RateLimitsWithResetCredits {
        rate_limits: vec![RateLimitSnapshot {
            limit_id: Some("codex".to_string()),
            limit_name: None,
            primary: Some(RateLimitWindow {
                used_percent: 99.6,
                window_minutes: Some(300),
                resets_at: Some(1_749_950_000),
            }),
            secondary: None,
            credits: None,
            individual_limit: None,
            plan_type: None,
            rate_limit_reached_type: None,
        }],
        rate_limit_reset_credits: None,
    };

    assert_eq!(
        account_usage(&response),
        AccountUsage {
            primary_window_minutes: Some(300),
            five_hour_reset_at: Some(1_749_950_000),
            five_hour_remaining_percent: Some(0),
            five_hour_exhausted: false,
            weekly_reset_at: None,
            weekly_remaining_percent: None,
            weekly_exhausted: false,
        }
    );
}

#[test]
fn reused_refresh_token_marks_imported_account_as_login_required() {
    let error = anyhow!(RefreshTokenFailedError::new(
        RefreshTokenFailedReason::Exhausted,
        "refresh token already used",
    ));

    assert!(login_required(&error));
}
