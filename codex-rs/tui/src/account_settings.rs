use std::sync::Arc;

use codex_config::types::AutomaticAccountSelection;

use super::*;
use crate::legacy_core::config::edit::ConfigEdit;
use crate::legacy_core::config::edit::ConfigEditsBuilder;

impl ChatWidget {
    pub(super) fn open_accounts_popup(&mut self) {
        self.bottom_pane
            .show_selection_view(account_settings_params(
                self.config.automatic_account_selection,
                Arc::new(self.config.clone()),
            ));
        self.request_redraw();
    }
}

fn account_settings_params(
    current: AutomaticAccountSelection,
    config: Arc<Config>,
) -> SelectionViewParams {
    let state = selection_label(current);
    SelectionViewParams {
        title: Some("Accounts".to_string()),
        subtitle: Some(format!(
            "Automatic account selection is {state}. Changes apply to new sessions."
        )),
        footer_hint: Some(standard_popup_hint_line()),
        items: [
            (
                AutomaticAccountSelection::Enabled,
                "Enable automatic selection",
                "Choose and switch accounts when needed.",
            ),
            (
                AutomaticAccountSelection::Disabled,
                "Disable automatic selection",
                "Stay on the current or manually selected account.",
            ),
        ]
        .into_iter()
        .map(|(selection, name, description)| SelectionItem {
            name: name.to_string(),
            description: Some(description.to_string()),
            is_current: selection == current,
            actions: vec![selection_action(Arc::clone(&config), selection)],
            dismiss_on_select: true,
            ..Default::default()
        })
        .collect(),
        ..Default::default()
    }
}

fn selection_action(config: Arc<Config>, selection: AutomaticAccountSelection) -> SelectionAction {
    Box::new(move |tx| {
        let config = Arc::clone(&config);
        let tx = tx.clone();
        tokio::spawn(async move {
            let result = persist_automatic_account_selection(&config, selection)
                .await
                .map_err(|err| err.to_string());
            let message = persistence_message(selection, result);
            let cell = match message {
                PersistenceMessage::Success(message) => {
                    history_cell::new_info_event(message, /*hint*/ None)
                }
                PersistenceMessage::Error(message) => history_cell::new_error_event(message),
            };
            tx.send(AppEvent::InsertHistoryCell(Box::new(cell)));
        });
    })
}

async fn persist_automatic_account_selection(
    config: &Config,
    selection: AutomaticAccountSelection,
) -> anyhow::Result<()> {
    let value = format!("\"{}\"", selection_label(selection)).parse()?;
    ConfigEditsBuilder::for_config(config)
        .with_edits([ConfigEdit::SetPath {
            segments: vec!["automatic_account_selection".to_string()],
            value,
        }])
        .apply()
        .await
}

fn selection_label(selection: AutomaticAccountSelection) -> &'static str {
    match selection {
        AutomaticAccountSelection::Enabled => "enabled",
        AutomaticAccountSelection::Disabled => "disabled",
    }
}

enum PersistenceMessage {
    Success(String),
    Error(String),
}

fn persistence_message(
    selection: AutomaticAccountSelection,
    result: Result<(), String>,
) -> PersistenceMessage {
    match result {
        Ok(()) => PersistenceMessage::Success(format!(
            "Automatic account selection {}. New sessions will use this setting.",
            selection_label(selection)
        )),
        Err(err) => PersistenceMessage::Error(format!(
            "Failed to update automatic account selection: {err}"
        )),
    }
}

#[cfg(test)]
#[path = "account_settings_tests.rs"]
mod tests;
