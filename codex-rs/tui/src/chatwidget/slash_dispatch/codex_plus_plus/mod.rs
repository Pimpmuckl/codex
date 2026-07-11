//! Codex++ settings exposed through fork slash commands.

mod accounts;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use codex_config::types::AutomaticAccountSelection;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use ratatui::text::Line;

use super::super::*;
use crate::bottom_pane::ListSelectionView;
use crate::bottom_pane::SelectionToggle;
use crate::key_hint;
use crate::keymap::ListKeymap;
use crate::keymap::primary_binding;

impl ChatWidget {
    pub(super) fn open_codex_plus_plus_popup(&mut self) {
        let list_keymap = settings_list_keymap(self.bottom_pane.list_keymap());
        let params =
            codex_plus_plus_settings_params(self.config.automatic_account_selection, &list_keymap);
        let view = ListSelectionView::new(params, self.app_event_tx.clone(), list_keymap);
        self.bottom_pane.show_view(Box::new(view));
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

fn settings_list_keymap(mut keymap: ListKeymap) -> ListKeymap {
    keymap
        .accept
        .retain(|binding| !binding.is_press(KeyEvent::from(KeyCode::Char(' '))));
    if keymap.accept.is_empty() {
        keymap.accept.push(key_hint::plain(KeyCode::Enter));
    }
    keymap
}

fn codex_plus_plus_settings_params(
    current: AutomaticAccountSelection,
    list_keymap: &ListKeymap,
) -> SelectionViewParams {
    let enabled = Arc::new(AtomicBool::new(matches!(
        current,
        AutomaticAccountSelection::Enabled
    )));
    let enabled_on_toggle = Arc::clone(&enabled);
    let enabled_on_save = enabled;

    SelectionViewParams {
        title: Some("Codex++ Settings".to_string()),
        subtitle: Some("Select the settings to enable.".to_string()),
        footer_hint: Some(settings_hint_line(list_keymap)),
        items: vec![SelectionItem {
            name: "Automatic account selection".to_string(),
            description: Some("Choose and switch accounts when needed.".to_string()),
            toggle: Some(SelectionToggle {
                is_on: enabled_on_toggle.load(Ordering::Relaxed),
                action: Box::new(move |is_on, _tx| {
                    enabled_on_toggle.store(is_on, Ordering::Relaxed);
                }),
            }),
            actions: vec![Box::new(move |tx| {
                tx.send(AppEvent::PersistAutomaticAccountSelection {
                    selection: if enabled_on_save.load(Ordering::Relaxed) {
                        AutomaticAccountSelection::Enabled
                    } else {
                        AutomaticAccountSelection::Disabled
                    },
                });
            })],
            dismiss_on_select: true,
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn settings_hint_line(list_keymap: &ListKeymap) -> Line<'static> {
    let mut spans = vec![
        "Press ".into(),
        key_hint::plain(KeyCode::Char(' ')).into(),
        " to toggle".into(),
    ];
    if let Some(accept) = primary_binding(&list_keymap.accept) {
        spans.extend(["; ".into(), accept.into(), " to save".into()]);
    }
    if let Some(cancel) = primary_binding(&list_keymap.cancel) {
        spans.extend(["; ".into(), cancel.into(), " to cancel".into()]);
    }
    spans.into()
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
#[path = "codex_plus_plus_tests.rs"]
mod tests;
