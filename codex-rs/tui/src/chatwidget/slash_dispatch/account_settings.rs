use codex_config::types::AutomaticAccountSelection;

use super::*;

impl ChatWidget {
    pub(super) fn open_accounts_popup(&mut self) {
        self.bottom_pane
            .show_selection_view(account_settings_params(
                self.config.automatic_account_selection,
            ));
        self.request_redraw();
    }

    pub(crate) fn automatic_account_selection_persisted(
        &mut self,
        selection: AutomaticAccountSelection,
    ) {
        self.config.automatic_account_selection = selection;
        self.add_info_message(persistence_success_message(selection), /*hint*/ None);
    }

    pub(crate) fn automatic_account_selection_persistence_failed(&mut self, err: String) {
        self.add_error_message(persistence_error_message(err));
    }

    pub(crate) fn automatic_account_selection_persistence_overridden(
        &mut self,
        selection: AutomaticAccountSelection,
    ) {
        self.config.automatic_account_selection = selection;
        self.add_error_message(persistence_overridden_message(selection));
    }

    pub(crate) fn automatic_account_selection_verification_failed(&mut self, err: String) {
        self.add_error_message(persistence_verification_failed_message(err));
    }
}

fn account_settings_params(current: AutomaticAccountSelection) -> SelectionViewParams {
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
            actions: vec![selection_action(selection)],
            dismiss_on_select: true,
            ..Default::default()
        })
        .collect(),
        ..Default::default()
    }
}

fn selection_action(selection: AutomaticAccountSelection) -> SelectionAction {
    Box::new(move |tx| {
        tx.send(AppEvent::PersistAutomaticAccountSelection { selection });
    })
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

fn persistence_overridden_message(selection: AutomaticAccountSelection) -> String {
    format!(
        "Automatic account selection remains {} because this setting is controlled elsewhere.",
        selection_label(selection)
    )
}

fn persistence_verification_failed_message(err: String) -> String {
    format!(
        "Automatic account selection was saved, but Codex could not verify it for this project: {err}"
    )
}

#[cfg(test)]
#[path = "account_settings_tests.rs"]
mod tests;
