use super::*;
use crate::legacy_core::config::ConfigBuilder;
use codex_protocol::auth::RefreshTokenFailedError;
use codex_protocol::protocol::RateLimitWindow;
use wiremock::MockServer;

const TEST_ID_TOKEN: &str = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJlbWFpbCI6InVzZXJAZXhhbXBsZS5jb20iLCJlbWFpbF92ZXJpZmllZCI6dHJ1ZSwiaHR0cHM6Ly9hcGkub3BlbmFpLmNvbS9hdXRoIjp7ImNoYXRncHRfdXNlcl9pZCI6InVzZXItMTIzNDUiLCJ1c2VyX2lkIjoidXNlci0xMjM0NSIsImNoYXRncHRfcGxhbl90eXBlIjoicHJvIiwiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjb3VudC0xMjMifX0.c2ln";

#[tokio::test]
async fn fetch_enforces_forced_workspace_before_network() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("auth.json"), format!(r#"{{"OPENAI_API_KEY":null,"tokens":{{"id_token":"{TEST_ID_TOKEN}","access_token":"token","refresh_token":"refresh","account_id":"account-123"}},"last_refresh":"2099-01-01T00:00:00Z"}}"#)).unwrap();
    let server = MockServer::start().await;
    let mut config = ConfigBuilder::default()
        .codex_home(home.path().into())
        .build()
        .await
        .unwrap();
    config.chatgpt_base_url = server.uri();
    config.forced_chatgpt_workspace_id = Some(vec!["other".to_string()]);
    assert!(fetch(home.path().into(), config).await.is_err());
    assert!(server.received_requests().await.unwrap().is_empty());
}

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
                used_percent: 0.4,
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
            weekly_unused: Some(false),
            weekly_remaining_percent: Some(100),
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
        weekly_unused: Some(false),
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
            weekly_unused: None,
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
