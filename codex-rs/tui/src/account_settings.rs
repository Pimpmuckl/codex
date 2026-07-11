use std::path::PathBuf;

use codex_config::CONFIG_TOML_FILE;
use codex_config::TomlValue;
use codex_config::types::AutomaticAccountSelection;

use super::*;
use crate::legacy_core::config::edit::ConfigEdit;
use crate::legacy_core::config::edit::ConfigEditsBuilder;

impl ChatWidget {
    pub(super) fn open_accounts_popup(&mut self) {
        let config_path = self
            .config
            .config_layer_stack
            .get_user_config_file()
            .map(codex_utils_absolute_path::AbsolutePathBuf::to_path_buf)
            .unwrap_or_else(|| self.config.codex_home.join(CONFIG_TOML_FILE).to_path_buf());
        let current = persisted_automatic_account_selection(&config_path)
            .unwrap_or(self.config.automatic_account_selection);
        self.bottom_pane
            .show_selection_view(account_settings_params(current, config_path));
        self.request_redraw();
    }
}

fn account_settings_params(
    current: AutomaticAccountSelection,
    config_path: PathBuf,
) -> SelectionViewParams {
    let state = selection_label(current);
    SelectionViewParams {
        title: Some("Accounts".to_string()),
        subtitle: Some(format!(
            "Automatic account selection is {state}. Restart Codex to use changes."
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
            actions: vec![selection_action(config_path.clone(), selection)],
            dismiss_on_select: true,
            ..Default::default()
        })
        .collect(),
        ..Default::default()
    }
}

fn selection_action(config_path: PathBuf, selection: AutomaticAccountSelection) -> SelectionAction {
    Box::new(move |tx| {
        let cell = match persist_automatic_account_selection(&config_path, selection) {
            Ok(()) => history_cell::new_info_event(
                persistence_success_message(selection),
                /*hint*/ None,
            ),
            Err(err) => history_cell::new_error_event(persistence_error_message(err.to_string())),
        };
        tx.send(AppEvent::InsertHistoryCell(Box::new(cell)));
    })
}

fn persist_automatic_account_selection(
    config_path: &std::path::Path,
    selection: AutomaticAccountSelection,
) -> anyhow::Result<()> {
    let value = format!("\"{}\"", selection_label(selection)).parse()?;
    ConfigEditsBuilder::for_config_path(config_path)
        .with_edits([ConfigEdit::SetPath {
            segments: vec!["automatic_account_selection".to_string()],
            value,
        }])
        .apply_blocking()
}

fn persisted_automatic_account_selection(
    config_path: &std::path::Path,
) -> Option<AutomaticAccountSelection> {
    let contents = std::fs::read_to_string(config_path).ok()?;
    let config = toml::from_str::<TomlValue>(&contents).ok()?;
    match config.get("automatic_account_selection")?.as_str()? {
        "enabled" => Some(AutomaticAccountSelection::Enabled),
        "disabled" => Some(AutomaticAccountSelection::Disabled),
        _ => None,
    }
}

fn selection_label(selection: AutomaticAccountSelection) -> &'static str {
    match selection {
        AutomaticAccountSelection::Enabled => "enabled",
        AutomaticAccountSelection::Disabled => "disabled",
    }
}

fn persistence_success_message(selection: AutomaticAccountSelection) -> String {
    format!(
        "Automatic account selection {}. Restart Codex to use this setting.",
        selection_label(selection)
    )
}

fn persistence_error_message(err: String) -> String {
    format!("Failed to update automatic account selection: {err}")
}

#[cfg(test)]
#[path = "account_settings_tests.rs"]
mod tests;
