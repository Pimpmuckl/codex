//! Codex++ settings exposed through fork slash commands.

mod accounts;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use AutomaticAccountSelection::Disabled as AutomaticOff;
use AutomaticAccountSelection::Enabled as AutomaticOn;
use WeeklyUsageWindowAutoStart::Disabled as WeeklyOff;
use WeeklyUsageWindowAutoStart::Enabled as WeeklyOn;
use codex_config::WeeklyUsageWindowAutoStart;
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
        let params = codex_plus_plus_settings_params(
            self.config.automatic_account_selection,
            self.config.weekly_usage_window_auto_start,
            self.weekly_start_supported,
            &list_keymap,
        );
        let view = ListSelectionView::new(params, self.app_event_tx.clone(), list_keymap);
        self.bottom_pane.show_view(Box::new(view));
        self.request_redraw();
    }

    pub(crate) fn codex_plus_plus_settings_persisted(
        &mut self,
        automatic: AutomaticAccountSelection,
        weekly: WeeklyUsageWindowAutoStart,
    ) {
        self.config.automatic_account_selection = automatic;
        self.config.weekly_usage_window_auto_start = weekly;
        self.add_info_message(persistence_success_message(), /*hint*/ None);
    }

    pub(crate) fn codex_plus_plus_settings_persistence_failed(&mut self, err: String) {
        self.add_error_message(persistence_error_message(err));
    }

    pub(crate) fn codex_plus_plus_settings_persistence_overridden(
        &mut self,
        automatic: AutomaticAccountSelection,
        weekly: WeeklyUsageWindowAutoStart,
    ) {
        self.config.automatic_account_selection = automatic;
        self.config.weekly_usage_window_auto_start = weekly;
        self.add_error_message(persistence_overridden_message());
    }

    pub(crate) fn codex_plus_plus_settings_verification_failed(&mut self, err: String) {
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
    current_automatic: AutomaticAccountSelection,
    current_weekly: WeeklyUsageWindowAutoStart,
    weekly_supported: bool,
    list_keymap: &ListKeymap,
) -> SelectionViewParams {
    let automatic = Arc::new(AtomicBool::new(current_automatic == AutomaticOn));
    let weekly = Arc::new(AtomicBool::new(current_weekly == WeeklyOn));
    let mut items = vec![settings_item(
        "Automatic account selection",
        "Choose and switch accounts when needed.",
        Arc::clone(&automatic),
        Arc::clone(&automatic),
        Arc::clone(&weekly),
    )];
    if weekly_supported {
        items.push(settings_item(
            "Start unused weekly windows",
            "Keep imported accounts ready to use.",
            Arc::clone(&weekly),
            automatic,
            weekly,
        ));
    }
    SelectionViewParams {
        title: Some("Codex++ Settings".to_string()),
        subtitle: Some("Select the settings to enable.".to_string()),
        footer_hint: Some(settings_hint_line(list_keymap)),
        items,
        ..Default::default()
    }
}

fn settings_item(
    name: &str,
    description: &str,
    toggle: Arc<AtomicBool>,
    automatic: Arc<AtomicBool>,
    weekly: Arc<AtomicBool>,
) -> SelectionItem {
    let toggle_action = Arc::clone(&toggle);
    SelectionItem {
        name: name.to_string(),
        description: Some(description.to_string()),
        toggle: Some(SelectionToggle {
            is_on: toggle.load(Ordering::Relaxed),
            action: Box::new(move |is_on, _tx| toggle_action.store(is_on, Ordering::Relaxed)),
        }),
        actions: vec![Box::new(move |tx| {
            tx.send(AppEvent::PersistCodexPlusPlusSettings {
                automatic_account_selection: automatic
                    .load(Ordering::Relaxed)
                    .then_some(AutomaticOn)
                    .unwrap_or(AutomaticOff),
                weekly_usage_window_auto_start: weekly
                    .load(Ordering::Relaxed)
                    .then_some(WeeklyOn)
                    .unwrap_or(WeeklyOff),
            });
        })],
        dismiss_on_select: true,
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

fn persistence_success_message() -> String {
    "Codex++ settings updated. Automatic account selection applies after restart.".to_string()
}

fn persistence_error_message(err: String) -> String {
    format!("Failed to update Codex++ settings: {err}")
}

fn persistence_overridden_message() -> String {
    "Some Codex++ settings are controlled elsewhere and were not changed.".to_string()
}

fn persistence_verification_failed_message(err: String) -> String {
    format!("Codex++ settings were saved, but Codex could not verify them for this project: {err}")
}

#[cfg(test)]
#[path = "codex_plus_plus_tests.rs"]
mod tests;
