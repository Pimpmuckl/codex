//! Persistence for Codex++ global settings.

use crate::app::App;
use crate::app_server_session::AppServerSession;
use codex_config::WeeklyUsageWindowAutoStart;
use codex_config::types::AutomaticAccountSelection;

pub(crate) async fn persist_settings(
    app: &mut App,
    app_server: &AppServerSession,
    automatic_account_selection: AutomaticAccountSelection,
    weekly_usage_window_auto_start: WeeklyUsageWindowAutoStart,
) {
    let automatic = match automatic_account_selection {
        AutomaticAccountSelection::Enabled => "enabled",
        AutomaticAccountSelection::Disabled => "disabled",
    };
    let weekly = match weekly_usage_window_auto_start {
        WeeklyUsageWindowAutoStart::Enabled => "enabled",
        WeeklyUsageWindowAutoStart::Disabled => "disabled",
    };
    let writes = vec![
        crate::config_update::replace_config_value(
            "automatic_account_selection",
            serde_json::json!(automatic),
        ),
        crate::config_update::replace_config_value(
            "weekly_usage_window_auto_start",
            serde_json::json!(weekly),
        ),
    ];
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
    app.config.automatic_account_selection = effective_automatic;
    app.config.weekly_usage_window_auto_start = effective_weekly;
    match effective_weekly {
        WeeklyUsageWindowAutoStart::Enabled
            if app_server.uses_embedded_app_server() && app.weekly_window_scheduler.is_none() =>
        {
            let model = app.chat_widget.current_model().to_string();
            app.weekly_window_scheduler = Some(super::WeeklyWindowScheduler::spawn(
                app.config.clone(),
                model,
            ));
        }
        WeeklyUsageWindowAutoStart::Disabled => {
            app.weekly_window_scheduler = None;
        }
        WeeklyUsageWindowAutoStart::Enabled => {}
    }
    if effective_automatic == automatic_account_selection
        && effective_weekly == weekly_usage_window_auto_start
    {
        app.chat_widget
            .codex_plus_plus_settings_persisted(effective_automatic, effective_weekly);
    } else {
        app.chat_widget
            .codex_plus_plus_settings_persistence_overridden(effective_automatic, effective_weekly);
    }
}
