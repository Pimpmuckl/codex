//! Codex++ settings exposed through fork slash commands.

mod accounts;
mod dcg;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use AutomaticAccountSelection::Disabled as AutomaticOff;
use AutomaticAccountSelection::Enabled as AutomaticOn;
use ModelCapacityRetryMode::Bounded as CapacityBounded;
use ModelCapacityRetryMode::Indefinite as CapacityIndefinite;
use ToolActivityPresentation::Compact as ActivityCompact;
use ToolActivityPresentation::Full as ActivityFull;
use UserMessageInbox::Disabled as InboxOff;
use UserMessageInbox::Enabled as InboxOn;
use WeeklyUsageWindowAutoStart::Disabled as WeeklyOff;
use WeeklyUsageWindowAutoStart::Enabled as WeeklyOn;
use codex_config::ModelCapacityRetryMode;
use codex_config::ToolActivityPresentation;
use codex_config::UserMessageInbox;
use codex_config::WeeklyUsageWindowAutoStart;
use codex_config::types::AutomaticAccountSelection;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use ratatui::text::Line;

use super::super::*;
use crate::bottom_pane::ListSelectionView;
use crate::bottom_pane::SelectionToggle;
use crate::codex_plus_plus::destructive_command_guard::DcgStatus;
use crate::key_hint;
use crate::keymap::ListKeymap;
use crate::keymap::primary_binding;

impl ChatWidget {
    pub(super) fn show_user_message_inbox(&mut self) {
        let enabled =
            crate::codex_plus_plus::user_message_inbox_enabled(&self.config.config_layer_stack);
        self.add_to_history(self.user_message_inbox.history_cell(enabled));
        self.request_redraw();
    }

    pub(crate) fn open_codex_plus_plus_popup(&mut self, dcg_status: DcgStatus) {
        let list_keymap = settings_list_keymap(self.bottom_pane.list_keymap());
        let params = codex_plus_plus_settings_params(
            self.config.automatic_account_selection,
            self.config.weekly_usage_window_auto_start,
            crate::codex_plus_plus::auto_redeem_resets_settings(&self.config.config_layer_stack)
                .is_some(),
            self.config.model_capacity_retry_mode,
            crate::codex_plus_plus::user_message_inbox_enabled(&self.config.config_layer_stack),
            self.config.codex_plus_plus_tool_activity,
            self.weekly_start_supported,
            Some(dcg_status),
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
        capacity: ModelCapacityRetryMode,
        tool_activity: ToolActivityPresentation,
    ) {
        self.config.automatic_account_selection = automatic;
        self.config.weekly_usage_window_auto_start = weekly;
        self.config.model_capacity_retry_mode = capacity;
        self.config.codex_plus_plus_tool_activity = tool_activity;
        self.add_info_message(persistence_success_message(), /*hint*/ None);
    }

    pub(crate) fn sync_codex_plus_plus_settings_config(
        &mut self,
        config: &crate::legacy_core::config::Config,
    ) {
        self.config.automatic_account_selection = config.automatic_account_selection;
        self.config.weekly_usage_window_auto_start = config.weekly_usage_window_auto_start;
        self.config.model_capacity_retry_mode = config.model_capacity_retry_mode;
        self.config.codex_plus_plus_tool_activity = config.codex_plus_plus_tool_activity;
        self.config.config_layer_stack = config.config_layer_stack.clone();
    }

    pub(crate) fn codex_plus_plus_settings_persistence_failed(&mut self, err: String) {
        self.add_error_message(persistence_error_message(err));
    }

    pub(crate) fn codex_plus_plus_settings_persistence_overridden(
        &mut self,
        automatic: AutomaticAccountSelection,
        weekly: WeeklyUsageWindowAutoStart,
        capacity: ModelCapacityRetryMode,
        tool_activity: ToolActivityPresentation,
    ) {
        self.config.automatic_account_selection = automatic;
        self.config.weekly_usage_window_auto_start = weekly;
        self.config.model_capacity_retry_mode = capacity;
        self.config.codex_plus_plus_tool_activity = tool_activity;
        self.add_error_message(persistence_overridden_message());
    }

    pub(crate) fn codex_plus_plus_settings_verification_failed(&mut self, err: String) {
        self.add_error_message(persistence_verification_failed_message(err));
    }
}

#[derive(Clone, Default)]
struct SettingsSelection {
    automatic: Arc<AtomicBool>,
    weekly: Arc<AtomicBool>,
    auto_redeem: Arc<AtomicBool>,
    capacity_indefinite: Arc<AtomicBool>,
    user_message_inbox: Arc<AtomicBool>,
    compact_tool_activity: Arc<AtomicBool>,
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
    current_auto_redeem: bool,
    current_capacity: ModelCapacityRetryMode,
    current_user_message_inbox: bool,
    current_tool_activity: ToolActivityPresentation,
    weekly_supported: bool,
    dcg_status: Option<DcgStatus>,
    list_keymap: &ListKeymap,
) -> SelectionViewParams {
    let selection = SettingsSelection {
        automatic: Arc::new(AtomicBool::new(current_automatic == AutomaticOn)),
        weekly: Arc::new(AtomicBool::new(current_weekly == WeeklyOn)),
        auto_redeem: Arc::new(AtomicBool::new(current_auto_redeem)),
        capacity_indefinite: Arc::new(AtomicBool::new(current_capacity == CapacityIndefinite)),
        user_message_inbox: Arc::new(AtomicBool::new(current_user_message_inbox)),
        compact_tool_activity: Arc::new(AtomicBool::new(current_tool_activity == ActivityCompact)),
    };
    let mut items = vec![settings_item(
        "Automatic account selection",
        "Choose and switch accounts when needed.",
        Arc::clone(&selection.automatic),
        selection.clone(),
        weekly_supported,
    )];
    if let Some(status) = dcg_status {
        items.insert(
            0,
            dcg::settings_item(status, selection.clone(), weekly_supported),
        );
    }
    if weekly_supported {
        items.push(settings_item(
            "Start unused weekly windows",
            "Keep imported accounts ready to use.",
            Arc::clone(&selection.weekly),
            selection.clone(),
            true,
        ));
        items.push(settings_item(
            "Auto-redeem usage resets (Experimental)",
            "Use earned resets before expiry or a distant weekly reset.",
            Arc::clone(&selection.auto_redeem),
            selection.clone(),
            true,
        ));
    }
    items.push(settings_item(
        "Keep retrying at capacity",
        "After 1, 2, 5, and 15 minutes, retry every 15 minutes.",
        Arc::clone(&selection.capacity_indefinite),
        selection.clone(),
        weekly_supported,
    ));
    items.push(settings_item(
        "Compact tool activity",
        "Keep routine tool details in Ctrl+T instead of the main conversation.",
        Arc::clone(&selection.compact_tool_activity),
        selection.clone(),
        weekly_supported,
    ));
    items.push(settings_item(
        "Agent inbox messages (Experimental)",
        "Let Codex leave durable messages you can review with /inbox.",
        Arc::clone(&selection.user_message_inbox),
        selection,
        weekly_supported,
    ));
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
    selection: SettingsSelection,
    save_weekly: bool,
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
            let automatic = if selection.automatic.load(Ordering::Relaxed) {
                AutomaticOn
            } else {
                AutomaticOff
            };
            tx.send(AppEvent::PersistCodexPlusPlusSettings {
                automatic_account_selection: automatic,
                weekly_usage_window_auto_start: save_weekly.then(|| {
                    if selection.weekly.load(Ordering::Relaxed) {
                        WeeklyOn
                    } else {
                        WeeklyOff
                    }
                }),
                auto_redeem_resets: save_weekly
                    .then(|| selection.auto_redeem.load(Ordering::Relaxed)),
                model_capacity_retry_mode: if selection.capacity_indefinite.load(Ordering::Relaxed)
                {
                    CapacityIndefinite
                } else {
                    CapacityBounded
                },
                user_message_inbox: if selection.user_message_inbox.load(Ordering::Relaxed) {
                    InboxOn
                } else {
                    InboxOff
                },
                tool_activity: if selection.compact_tool_activity.load(Ordering::Relaxed) {
                    ActivityCompact
                } else {
                    ActivityFull
                },
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
        spans.extend(["; ".into(), accept.into(), " to save or manage".into()]);
    }
    if let Some(cancel) = primary_binding(&list_keymap.cancel) {
        spans.extend(["; ".into(), cancel.into(), " to cancel".into()]);
    }
    spans.into()
}

fn persistence_success_message() -> String {
    "Codex++ settings updated. Changes apply after restart.".to_string()
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
