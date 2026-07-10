use super::*;
use codex_protocol::protocol::RateLimitWindow;

#[test]
fn maps_weekly_window_to_picker_usage() {
    let response = RateLimitsWithResetCredits {
        rate_limits: vec![RateLimitSnapshot {
            limit_id: Some("codex".to_string()),
            limit_name: None,
            primary: None,
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
            weekly_reset_at: Some(1_750_000_000),
            weekly_remaining_percent: Some(88),
        }
    );
}
