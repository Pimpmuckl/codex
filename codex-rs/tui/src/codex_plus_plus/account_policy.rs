//! Persistence for the Codex++ automatic account-selection policy.

use crate::app::App;
use crate::app_server_session::AppServerSession;
use codex_app_server_protocol::WriteStatus;
use codex_config::types::AutomaticAccountSelection;

pub(crate) async fn persist_automatic_account_selection(
    app: &mut App,
    app_server: &AppServerSession,
    selection: AutomaticAccountSelection,
) {
    let value = match selection {
        AutomaticAccountSelection::Enabled => "enabled",
        AutomaticAccountSelection::Disabled => "disabled",
    };
    match crate::config_update::write_config_batch(
        app_server.request_handle(),
        vec![crate::config_update::replace_config_value(
            "automatic_account_selection",
            serde_json::json!(value),
        )],
    )
    .await
    {
        Ok(response) => {
            let overridden_selection = response
                .overridden_metadata
                .as_ref()
                .and_then(|metadata| metadata.effective_value.as_str())
                .and_then(|value| match value {
                    "enabled" => Some(AutomaticAccountSelection::Enabled),
                    "disabled" => Some(AutomaticAccountSelection::Disabled),
                    _ => None,
                })
                .filter(|_| response.status == WriteStatus::OkOverridden);
            if let Some(effective) = overridden_selection {
                app.config.automatic_account_selection = effective;
                app.chat_widget
                    .automatic_account_selection_persistence_overridden(effective);
            } else {
                let cwd = app.config.cwd.display().to_string();
                match crate::config_update::read_effective_config(app_server.request_handle(), cwd)
                    .await
                {
                    Ok(response) => {
                        let effective = match response
                            .config
                            .additional
                            .get("automatic_account_selection")
                            .and_then(serde_json::Value::as_str)
                        {
                            Some("disabled") => AutomaticAccountSelection::Disabled,
                            _ => AutomaticAccountSelection::Enabled,
                        };
                        app.config.automatic_account_selection = effective;
                        if effective == selection {
                            app.chat_widget
                                .automatic_account_selection_persisted(selection);
                        } else {
                            app.chat_widget
                                .automatic_account_selection_persistence_overridden(effective);
                        }
                    }
                    Err(err) => {
                        tracing::error!(
                            error = %err,
                            "failed to verify automatic account selection"
                        );
                        app.chat_widget
                            .automatic_account_selection_verification_failed(err.to_string());
                    }
                }
            }
        }
        Err(err) => {
            tracing::error!(
                error = %err,
                "failed to persist automatic account selection"
            );
            app.chat_widget
                .automatic_account_selection_persistence_failed(err.to_string());
        }
    }
}
