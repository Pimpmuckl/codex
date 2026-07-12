//! Per-account Codex++ automation settings exposed through `/accounts`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use codex_login::AccountId;
use codex_login::AccountStore;
use tracing::warn;

use super::*;
use crate::codex_plus_plus::WeeklyWindowStatus;

struct AccountAutomationRow {
    id: AccountId,
    label: String,
    enabled: bool,
    automation_enabled: bool,
    login_required: bool,
    is_current: bool,
    in_use: bool,
    weekly_status: Option<WeeklyWindowStatus>,
}

struct AccountAutomationChoice {
    id: AccountId,
    initial_automation_enabled: bool,
    automation_enabled: AtomicBool,
}

impl ChatWidget {
    pub(in crate::chatwidget::slash_dispatch) fn open_accounts_popup(&mut self) {
        self.set_queue_autosend_suppressed(/*suppressed*/ true);
        self.app_event_tx.send(AppEvent::OpenCodexPlusPlusAccounts);
    }

    pub(crate) fn open_accounts_popup_with_statuses(
        &mut self,
        weekly_statuses: Option<&HashMap<AccountId, WeeklyWindowStatus>>,
    ) {
        let store = AccountStore::new(self.config.codex_home.to_path_buf());
        let accounts = match store.list() {
            Ok(accounts) => accounts,
            Err(err) => {
                warn!(error = %err, "failed to load accounts for Codex++ settings");
                self.add_error_message("Could not load accounts.".to_string());
                return;
            }
        };
        if accounts.is_empty() {
            self.add_info_message(
                "No imported accounts found.".to_string(),
                /*hint*/ None,
            );
            return;
        }

        let current_account_id = store
            .current_root_account_id(
                self.config.cli_auth_credentials_store_mode,
                self.config.auth_keyring_backend_kind(),
            )
            .ok()
            .flatten();
        let rows = accounts
            .into_iter()
            .map(|account| {
                let in_use = store.account_in_use(&account.id).unwrap_or(false);
                let weekly_status =
                    weekly_statuses.and_then(|statuses| statuses.get(&account.id).copied());
                AccountAutomationRow {
                    is_current: current_account_id.as_ref() == Some(&account.id),
                    id: account.id,
                    label: account.label,
                    enabled: account.enabled,
                    automation_enabled: account.automation_enabled,
                    login_required: account.login_required,
                    in_use,
                    weekly_status,
                }
            })
            .collect();
        let list_keymap = settings_list_keymap(self.bottom_pane.list_keymap());
        let params =
            accounts_settings_params(rows, self.config.codex_home.to_path_buf(), &list_keymap);
        let view = ListSelectionView::new(params, self.app_event_tx.clone(), list_keymap);
        self.bottom_pane.show_view(Box::new(view));
        self.request_redraw();
    }
}

fn accounts_settings_params(
    rows: Vec<AccountAutomationRow>,
    codex_home: PathBuf,
    list_keymap: &ListKeymap,
) -> SelectionViewParams {
    let choices = Arc::new(
        rows.iter()
            .map(|row| {
                Arc::new(AccountAutomationChoice {
                    id: row.id.clone(),
                    initial_automation_enabled: row.automation_enabled,
                    automation_enabled: AtomicBool::new(row.automation_enabled),
                })
            })
            .collect::<Vec<_>>(),
    );
    let items = rows
        .into_iter()
        .zip(choices.iter())
        .map(|(row, choice)| {
            let choice_on_toggle = Arc::clone(choice);
            let choices_on_save = Arc::clone(&choices);
            let codex_home = codex_home.clone();
            SelectionItem {
                name: row.label,
                description: account_status(
                    row.enabled,
                    row.login_required,
                    row.in_use,
                    row.weekly_status,
                ),
                is_current: row.is_current,
                toggle: Some(SelectionToggle {
                    is_on: row.automation_enabled,
                    action: Box::new(move |is_on, _tx| {
                        choice_on_toggle
                            .automation_enabled
                            .store(is_on, Ordering::Relaxed);
                    }),
                }),
                actions: vec![Box::new(move |tx| {
                    let cell = match persist_account_automation(
                        &AccountStore::new(codex_home.clone()),
                        &choices_on_save,
                    ) {
                        Ok(()) => crate::history_cell::new_info_event(
                            "Account automation settings updated.".to_string(),
                            /*hint*/ None,
                        ),
                        Err(()) => crate::history_cell::new_error_event(save_error_message()),
                    };
                    tx.send(AppEvent::InsertHistoryCell(Box::new(cell)));
                })],
                dismiss_on_select: true,
                ..Default::default()
            }
        })
        .collect();

    SelectionViewParams {
        title: Some("Account Automation".to_string()),
        subtitle: Some("Select the accounts Codex++ may choose automatically.".to_string()),
        footer_hint: Some(settings_hint_line(list_keymap)),
        items,
        ..Default::default()
    }
}

fn account_status(
    enabled: bool,
    login_required: bool,
    in_use: bool,
    weekly_status: Option<WeeklyWindowStatus>,
) -> Option<String> {
    let mut statuses = Vec::new();
    if !enabled {
        statuses.push("Account disabled");
    }
    if login_required {
        statuses.push("Login required");
    }
    if in_use {
        statuses.push("In use");
    }
    let weekly = weekly_status.map(|status| match status {
        WeeklyWindowStatus::Waiting(percent) => weekly_status_text(percent, "Waiting"),
        WeeklyWindowStatus::Started(percent) => weekly_status_text(percent, "Started"),
        WeeklyWindowStatus::Retrying(percent) => weekly_status_text(percent, "Retrying"),
        WeeklyWindowStatus::SignInRequired => "Weekly · Sign-in required".to_string(),
        WeeklyWindowStatus::Failed => "Weekly · Unavailable".to_string(),
    });
    if let Some(weekly) = weekly.as_deref() {
        statuses.push(weekly);
    }
    (!statuses.is_empty()).then(|| statuses.join(" · "))
}

fn weekly_status_text(percent: Option<u8>, status: &str) -> String {
    percent.map_or_else(
        || format!("Weekly · {status}"),
        |percent| format!("Weekly {percent:>3}% · {status}"),
    )
}

fn persist_account_automation(
    store: &AccountStore,
    choices: &[Arc<AccountAutomationChoice>],
) -> Result<(), ()> {
    for choice in choices {
        let automation_enabled = choice.automation_enabled.load(Ordering::Relaxed);
        if automation_enabled == choice.initial_automation_enabled {
            continue;
        }
        match store.set_automation_enabled(&choice.id, automation_enabled) {
            Ok(true) => {}
            Ok(false) => {
                warn!(account_id = %choice.id, "account disappeared while saving automation settings");
                return Err(());
            }
            Err(err) => {
                warn!(account_id = %choice.id, error = %err, "failed to save account automation setting");
                return Err(());
            }
        }
    }
    Ok(())
}

fn save_error_message() -> String {
    "Could not update all account automation settings. Reopen /accounts to view saved settings."
        .to_string()
}

#[cfg(test)]
#[path = "accounts_tests.rs"]
mod tests;
