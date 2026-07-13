//! Persistence for Codex++ global settings.

use crate::app::App;
use crate::app_server_session::AppServerSession;
use codex_config::ModelCapacityRetryMode;
use codex_config::WeeklyUsageWindowAutoStart;
use codex_config::types::AutomaticAccountSelection;

pub(crate) async fn persist_settings(
    app: &mut App,
    app_server: &AppServerSession,
    automatic_account_selection: AutomaticAccountSelection,
    weekly_usage_window_auto_start: Option<WeeklyUsageWindowAutoStart>,
    model_capacity_retry_mode: Option<ModelCapacityRetryMode>,
) {
    let automatic = match automatic_account_selection {
        AutomaticAccountSelection::Enabled => "enabled",
        AutomaticAccountSelection::Disabled => "disabled",
    };
    let mut writes = vec![crate::config_update::replace_config_value(
        "automatic_account_selection",
        serde_json::json!(automatic),
    )];
    if let Some(weekly) = weekly_usage_window_auto_start {
        writes.push(crate::config_update::replace_config_value(
            "weekly_usage_window_auto_start",
            serde_json::json!(match weekly {
                WeeklyUsageWindowAutoStart::Enabled => "enabled",
                WeeklyUsageWindowAutoStart::Disabled => "disabled",
            }),
        ));
    }
    if let Some(capacity) = model_capacity_retry_mode {
        writes.push(crate::config_update::replace_config_value(
            "model_capacity_retry_mode",
            serde_json::json!(match capacity {
                ModelCapacityRetryMode::Bounded => "bounded",
                ModelCapacityRetryMode::Indefinite => "indefinite",
            }),
        ));
    }
    if let Err(err) =
        crate::config_update::write_config_batch(app_server.request_handle(), writes).await
    {
        tracing::error!(error = %err, "failed to persist Codex++ settings");
        app.chat_widget
            .codex_plus_plus_settings_persistence_failed(err.to_string());
        return;
    }

    let cwd = app.config.cwd.display().to_string();
    let response =
        match crate::config_update::read_effective_config(app_server.request_handle(), cwd).await {
            Ok(response) => response,
            Err(err) => {
                tracing::error!(error = %err, "failed to verify Codex++ settings");
                if weekly_usage_window_auto_start == Some(WeeklyUsageWindowAutoStart::Disabled)
                    && let Some(scheduler) = &app.weekly_window_scheduler
                {
                    scheduler.set_enabled(false);
                }
                app.chat_widget
                    .codex_plus_plus_settings_verification_failed(err.to_string());
                return;
            }
        };
    let effective_automatic = match response
        .config
        .additional
        .get("automatic_account_selection")
        .and_then(serde_json::Value::as_str)
    {
        Some("disabled") => AutomaticAccountSelection::Disabled,
        _ => AutomaticAccountSelection::Enabled,
    };
    let effective_weekly = match response
        .config
        .additional
        .get("weekly_usage_window_auto_start")
        .and_then(serde_json::Value::as_str)
    {
        Some("disabled") => WeeklyUsageWindowAutoStart::Disabled,
        _ => WeeklyUsageWindowAutoStart::Enabled,
    };
    let effective_capacity = match response
        .config
        .additional
        .get("model_capacity_retry_mode")
        .and_then(serde_json::Value::as_str)
    {
        Some("indefinite") => ModelCapacityRetryMode::Indefinite,
        _ => ModelCapacityRetryMode::Bounded,
    };
    app.config.automatic_account_selection = effective_automatic;
    app.config.weekly_usage_window_auto_start = effective_weekly;
    app.config.model_capacity_retry_mode = effective_capacity;
    if let Some(scheduler) = &app.weekly_window_scheduler {
        scheduler.set_enabled(effective_weekly == WeeklyUsageWindowAutoStart::Enabled);
    }
    if effective_automatic == automatic_account_selection
        && weekly_usage_window_auto_start.is_none_or(|weekly| effective_weekly == weekly)
        && model_capacity_retry_mode.is_none_or(|capacity| effective_capacity == capacity)
    {
        app.chat_widget.codex_plus_plus_settings_persisted(
            effective_automatic,
            effective_weekly,
            effective_capacity,
        );
    } else {
        app.chat_widget
            .codex_plus_plus_settings_persistence_overridden(
                effective_automatic,
                effective_weekly,
                effective_capacity,
            );
    }
}
