//! Startup account selection for Codex++.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

use codex_cloud_config::stop_cloud_config_refresh_before_account_picker;
use codex_config::types::AutomaticAccountSelection;
use codex_login::AccountCandidate;
use codex_login::AccountId;
use codex_login::AccountStore;
use codex_login::CodexAuth;
use codex_login::load_auth_dot_json;
use codex_protocol::auth::AuthMode;
use codex_protocol::config_types::ForcedLoginMethod;
use tracing::warn;

use crate::AppServerTarget;
use crate::TerminalRestoreGuard;
use crate::account_picker;
use crate::account_usage;
use crate::legacy_core::config::Config;
use crate::tui;
use crate::tui::Tui;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum StartupAccountSelection {
    Continue {
        selected_account_id: Option<AccountId>,
        reload_cloud_config: bool,
    },
    Exit,
}

pub(crate) async fn run_startup_account_picker(
    config: &Config,
    app_server_target: &AppServerTarget,
) -> color_eyre::Result<StartupAccountSelection> {
    let mut initialized_terminal = tui::init()?;
    initialized_terminal.terminal.clear()?;
    let mut tui = Tui::new(
        initialized_terminal.terminal,
        initialized_terminal.enhanced_keys_supported,
        initialized_terminal.stderr_guard,
    );
    let mut restore_guard = TerminalRestoreGuard::new();
    let selection = maybe_run_startup_account_picker(&mut tui, config, app_server_target).await;
    let _ = tui.terminal.clear();
    restore_guard.restore()?;
    selection
}

async fn maybe_run_startup_account_picker(
    tui: &mut Tui,
    config: &Config,
    app_server_target: &AppServerTarget,
) -> color_eyre::Result<StartupAccountSelection> {
    if !app_server_target.supports_startup_account_picker()
        || !config.model_provider.requires_openai_auth
        || config.forced_login_method == Some(ForcedLoginMethod::Api)
        || !root_auth_allows_imported_account_picker(config)
    {
        return Ok(StartupAccountSelection::Continue {
            selected_account_id: None,
            reload_cloud_config: false,
        });
    }

    let store = AccountStore::new(config.codex_home.to_path_buf());
    let current_account_id = store
        .current_root_account_id(
            config.cli_auth_credentials_store_mode,
            config.auth_keyring_backend_kind(),
        )
        .ok()
        .flatten();
    let root_auth_is_marker = load_auth_dot_json(
        config.codex_home.as_path(),
        config.cli_auth_credentials_store_mode,
        config.auth_keyring_backend_kind(),
    )
    .is_ok_and(|auth| {
        auth.and_then(|auth| auth.tokens)
            .is_some_and(|tokens| tokens.refresh_token.is_empty())
    });
    let mut selectable_accounts = store.enabled_file_accounts()?;
    if config.automatic_account_selection == AutomaticAccountSelection::Disabled
        && root_auth_is_marker
        && let Some(current_account_id) = current_account_id.as_ref()
        && let Some(current_account) = store
            .list()?
            .into_iter()
            .find(|account| account.login_required && &account.id == current_account_id)
    {
        let auth_path = config.codex_home.join(current_account.auth.path);
        if let Some(account_home) = auth_path.parent().filter(|_| auth_path.is_file()) {
            selectable_accounts.push((current_account.id, account_home.to_path_buf()));
        }
    }
    if config.forced_chatgpt_workspace_id.is_some() {
        let auth_route_config = config.auth_route_config();
        let mut allowed_accounts = Vec::with_capacity(selectable_accounts.len());
        for (account_id, account_home) in selectable_accounts {
            let auth = CodexAuth::from_auth_storage(
                &account_home,
                codex_config::types::AuthCredentialsStoreMode::File,
                config.forced_chatgpt_workspace_id.as_deref(),
                Some(&config.chatgpt_base_url),
                config.auth_keyring_backend_kind(),
                auth_route_config.as_ref(),
            )
            .await
            .ok()
            .flatten();
            if auth.as_ref().is_some_and(CodexAuth::is_chatgpt_auth) {
                allowed_accounts.push((account_id, account_home));
            }
        }
        selectable_accounts = allowed_accounts;
    }
    let selectable_homes: HashMap<AccountId, PathBuf> =
        selectable_accounts.iter().cloned().collect();
    let mut candidates: Vec<AccountCandidate> = store
        .candidates()?
        .into_iter()
        .filter(|candidate| candidate.enabled && selectable_homes.contains_key(&candidate.id))
        .collect();
    if candidates.is_empty() {
        return Ok(StartupAccountSelection::Continue {
            selected_account_id: None,
            reload_cloud_config: false,
        });
    }

    let usage = account_usage::load(config, &selectable_accounts, &store).await;
    let store_for_update = store.clone();
    let login_required_updates = usage.login_required.clone();
    let login_required = tokio::task::spawn_blocking(move || {
        let mut login_required = HashSet::new();
        for (account_id, attempted_auth) in &login_required_updates {
            if store_for_update.record_login_required_if_auth_matches(account_id, attempted_auth)? {
                login_required.insert(account_id.clone());
            }
        }
        Ok::<_, std::io::Error>(login_required)
    })
    .await??;
    candidates.retain(|candidate| {
        !login_required.contains(&candidate.id)
            || (config.automatic_account_selection == AutomaticAccountSelection::Disabled
                && root_auth_is_marker
                && current_account_id.as_ref() == Some(&candidate.id))
    });
    if candidates.is_empty() {
        return Ok(StartupAccountSelection::Continue {
            selected_account_id: None,
            reload_cloud_config: false,
        });
    }
    stop_cloud_config_refresh_before_account_picker().await;
    let mut picker_candidates: Vec<_> = candidates
        .iter()
        .map(|candidate| {
            account_picker_candidate(
                candidate,
                usage.usage.get(&candidate.id),
                store.account_in_use(&candidate.id).unwrap_or(false),
                current_account_id.as_ref() == Some(&candidate.id),
            )
        })
        .collect();
    let automatic_default_idx =
        automatic_default_index(&candidates, &picker_candidates, current_account_id.as_ref());
    let manual_default_idx = account_picker::recommended_candidate_index(&picker_candidates);
    let (default_idx, mode) = match (config.automatic_account_selection, automatic_default_idx) {
        (AutomaticAccountSelection::Enabled, Some(default_idx)) => {
            (default_idx, account_picker::StartupAccountPickerMode::Timed)
        }
        (AutomaticAccountSelection::Enabled, None) | (AutomaticAccountSelection::Disabled, _) => (
            manual_default_idx,
            account_picker::StartupAccountPickerMode::Manual,
        ),
    };
    picker_candidates[default_idx].is_default = true;
    let Some(selection) =
        account_picker::run_startup_account_picker(tui, picker_candidates, mode).await?
    else {
        return Ok(StartupAccountSelection::Exit);
    };
    let (selected_id, reload_cloud_config) = match selection {
        account_picker::StartupAccountPickerSelection::Automatic(account_id) => (account_id, true),
        account_picker::StartupAccountPickerSelection::User(account_id) => (account_id, true),
    };

    let selected_account = candidates
        .iter()
        .find(|candidate| candidate.id.as_str() == selected_id);
    if let Some(candidate) = selected_account {
        let store = store.clone();
        let account_id = candidate.id.clone();
        let store_mode = config.cli_auth_credentials_store_mode;
        let keyring_backend_kind = config.auth_keyring_backend_kind();
        tokio::task::spawn_blocking(move || {
            store.apply_imported_account_to_root_auth(&account_id, store_mode, keyring_backend_kind)
        })
        .await??;
    }

    Ok(StartupAccountSelection::Continue {
        selected_account_id: selected_account.map(|candidate| candidate.id.clone()),
        reload_cloud_config,
    })
}

fn automatic_default_index(
    candidates: &[AccountCandidate],
    picker_candidates: &[account_picker::AccountPickerCandidate],
    current_account_id: Option<&AccountId>,
) -> Option<usize> {
    if let Some(current_index) = candidates.iter().position(|candidate| {
        !candidate.automation_enabled && current_account_id == Some(&candidate.id)
    }) && picker_candidates
        .get(current_index)
        .is_some_and(|candidate| {
            !candidate.blocked
                && !candidate.in_use
                && !candidate.five_hour_exhausted
                && !candidate.weekly_exhausted
        })
    {
        return Some(current_index);
    }

    let automatic_indices = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.automation_enabled)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let automatic_candidates = automatic_indices
        .iter()
        .map(|index| picker_candidates[*index].clone())
        .collect::<Vec<_>>();
    (!automatic_indices.is_empty()).then(|| {
        automatic_indices[account_picker::recommended_candidate_index(&automatic_candidates)]
    })
}

fn root_auth_allows_imported_account_picker(config: &Config) -> bool {
    match load_auth_dot_json(
        config.codex_home.as_path(),
        config.cli_auth_credentials_store_mode,
        config.auth_keyring_backend_kind(),
    ) {
        Ok(Some(auth)) => match auth.auth_mode {
            Some(AuthMode::Chatgpt | AuthMode::ChatgptAuthTokens) => {
                config.automatic_account_selection == AutomaticAccountSelection::Enabled
                    || auth
                        .tokens
                        .is_some_and(|tokens| tokens.refresh_token.is_empty())
            }
            Some(
                AuthMode::ApiKey
                | AuthMode::Headers
                | AuthMode::AgentIdentity
                | AuthMode::PersonalAccessToken
                | AuthMode::BedrockApiKey,
            ) => false,
            None => {
                auth.openai_api_key.is_none()
                    && auth.personal_access_token.is_none()
                    && auth.bedrock_api_key.is_none()
                    && auth.agent_identity.is_none()
                    && (config.automatic_account_selection == AutomaticAccountSelection::Enabled
                        || auth
                            .tokens
                            .is_none_or(|tokens| tokens.refresh_token.is_empty()))
            }
        },
        Ok(None) => true,
        Err(err) => {
            warn!(%err, "Skipping startup account picker because root auth could not be read");
            false
        }
    }
}

fn account_picker_candidate(
    candidate: &AccountCandidate,
    usage: Option<&account_usage::AccountUsage>,
    in_use: bool,
    is_current: bool,
) -> account_picker::AccountPickerCandidate {
    account_picker::AccountPickerCandidate {
        id: candidate.id.to_string(),
        email: candidate.display_label.clone(),
        primary_window_label: crate::chatwidget::limit_label_for_window(
            usage.and_then(|usage| usage.primary_window_minutes),
            /*is_secondary*/ false,
        ),
        five_hour_reset: usage
            .and_then(|usage| usage.five_hour_reset_at)
            .and_then(format_reset_timestamp),
        five_hour_usage_left_percent: usage.and_then(|usage| usage.five_hour_remaining_percent),
        five_hour_exhausted: usage.is_some_and(|usage| usage.five_hour_exhausted),
        weekly_reset: usage
            .and_then(|usage| usage.weekly_reset_at)
            .and_then(format_reset_timestamp),
        weekly_usage_left_percent: usage.and_then(|usage| usage.weekly_remaining_percent),
        weekly_exhausted: usage.is_some_and(|usage| usage.weekly_exhausted),
        blocked_until: (usage.is_none() && candidate.blocked)
            .then_some(candidate.usage_limit_resets_at)
            .flatten()
            .and_then(format_reset_timestamp),
        blocked: candidate.blocked,
        in_use,
        is_current,
        is_default: false,
    }
}

fn format_reset_timestamp(timestamp: i64) -> Option<String> {
    chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, /*nsecs*/ 0)
        .map(|timestamp| timestamp.format("%b %d %H:%MZ").to_string())
}

#[cfg(test)]
#[path = "startup_accounts_tests.rs"]
mod tests;
