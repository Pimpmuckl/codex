//! Embedded TUI scheduler for starting unused imported-account weekly windows.

use std::collections::HashSet;
use std::future::Future;
use std::time::Duration;

use chrono::Utc;
use codex_config::WeeklyUsageWindowAutoStart;
use codex_config::types::AuthCredentialsStoreMode;
use codex_login::AccountStore;
use codex_login::CodexAuth;
use codex_login::WeeklyWindowAttemptDecision;
use codex_login::WeeklyWindowAttemptOutcome;
use codex_login::WeeklyWindowRetryableError;
use codex_login::WeeklyWindowUsage;
use codex_model_provider::WeeklyWindowPingOutcome;
use codex_model_provider::WeeklyWindowPingRequest;
use codex_model_provider::ping_weekly_window;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

use crate::account_usage;
use crate::legacy_core::config::Config;

const SCAN_INTERVAL: Duration = Duration::from_secs(5 * 60);

pub(crate) struct WeeklyWindowScheduler {
    task: JoinHandle<()>,
}

impl WeeklyWindowScheduler {
    pub(crate) fn spawn(config: Config, model: String) -> Self {
        debug_assert_eq!(
            config.weekly_usage_window_auto_start,
            WeeklyUsageWindowAutoStart::Enabled
        );
        // Keep the validated model/provider pair isolated from foreground thread changes.
        let task = tokio::spawn(run_schedule(move || scan(config.clone(), model.clone())));
        Self { task }
    }
}

impl Drop for WeeklyWindowScheduler {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn run_schedule<F, Fut>(mut scan: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now(), SCAN_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        scan().await;
    }
}

async fn scan(config: Config, model: String) {
    let store = AccountStore::new(config.codex_home.to_path_buf());
    let eligible = match store.list() {
        Ok(accounts) => accounts
            .into_iter()
            .filter(|account| {
                account.enabled && account.automation_enabled && !account.login_required
            })
            .map(|account| account.id)
            .collect::<HashSet<_>>(),
        Err(err) => {
            tracing::warn!(%err, "weekly-window scheduler could not read account policy");
            return;
        }
    };
    let accounts = match store.enabled_file_accounts() {
        Ok(accounts) => accounts
            .into_iter()
            .filter(|(account_id, _)| eligible.contains(account_id))
            .collect::<Vec<_>>(),
        Err(err) => {
            tracing::warn!(%err, "weekly-window scheduler could not read imported accounts");
            return;
        }
    };
    let accounts = accounts_matching_workspace(&config, accounts).await;
    let loaded = account_usage::load_without_refresh(&config, &accounts, &store).await;

    for (account_id, account_home) in accounts {
        if loaded.login_required.contains_key(&account_id) {
            continue;
        }
        let usage = loaded
            .usage
            .get(&account_id)
            .map_or(WeeklyWindowUsage::Missing, |usage| {
                match usage.weekly_unused {
                    Some(unused) => WeeklyWindowUsage::Present {
                        unused,
                        resets_at: usage.weekly_reset_at,
                    },
                    None => WeeklyWindowUsage::Missing,
                }
            });
        let attempt = match store.begin_weekly_window_attempt(
            &account_id,
            usage,
            Utc::now().timestamp(),
        ) {
            Ok(WeeklyWindowAttemptDecision::Ready(attempt)) => attempt,
            Ok(
                WeeklyWindowAttemptDecision::NotDue
                | WeeklyWindowAttemptDecision::Locked
                | WeeklyWindowAttemptDecision::StateUnavailable,
            ) => continue,
            Err(err) => {
                tracing::warn!(%account_id, %err, "weekly-window scheduler could not begin attempt");
                continue;
            }
        };
        let outcome = ping_weekly_window(WeeklyWindowPingRequest {
            account_codex_home: account_home,
            model: model.clone(),
            model_provider_id: config.model_provider_id.clone(),
            model_provider: config.model_provider.clone(),
            chatgpt_base_url: config.chatgpt_base_url.clone(),
            auth_route_config: config.auth_route_config(),
            forced_chatgpt_workspace_id: config.forced_chatgpt_workspace_id.clone(),
            http_client_factory: config.http_client_factory(),
        })
        .await;
        let outcome = match outcome {
            WeeklyWindowPingOutcome::Completed => WeeklyWindowAttemptOutcome::Completed {
                refreshed_usage: WeeklyWindowUsage::Missing,
            },
            WeeklyWindowPingOutcome::DefiniteRejection => WeeklyWindowAttemptOutcome::Retryable {
                error: WeeklyWindowRetryableError::Rejected,
            },
            WeeklyWindowPingOutcome::LoginRequired => WeeklyWindowAttemptOutcome::Retryable {
                error: WeeklyWindowRetryableError::LoginRequired,
            },
            WeeklyWindowPingOutcome::RecoveryRequired => {
                tracing::warn!(%account_id, "weekly-window ping requires account recovery");
                WeeklyWindowAttemptOutcome::Retryable {
                    error: WeeklyWindowRetryableError::LoginRequired,
                }
            }
            WeeklyWindowPingOutcome::Ambiguous => WeeklyWindowAttemptOutcome::Ambiguous,
            WeeklyWindowPingOutcome::UnsupportedConfiguration
            | WeeklyWindowPingOutcome::UnsupportedRouting => {
                tracing::warn!(%account_id, ?outcome, "weekly-window ping is unsupported");
                WeeklyWindowAttemptOutcome::Retryable {
                    error: WeeklyWindowRetryableError::Rejected,
                }
            }
        };
        if let Err(err) = attempt.finish(outcome, Utc::now().timestamp()) {
            tracing::warn!(%account_id, %err, "weekly-window scheduler could not finish attempt");
        }
    }
}

async fn accounts_matching_workspace(
    config: &Config,
    accounts: Vec<(codex_login::AccountId, std::path::PathBuf)>,
) -> Vec<(codex_login::AccountId, std::path::PathBuf)> {
    if config.forced_chatgpt_workspace_id.is_none() {
        return accounts;
    }
    let mut matching = Vec::with_capacity(accounts.len());
    let auth_route_config = config.auth_route_config();
    for (account_id, account_home) in accounts {
        let auth = CodexAuth::from_auth_storage(
            &account_home,
            AuthCredentialsStoreMode::File,
            config.forced_chatgpt_workspace_id.as_deref(),
            Some(&config.chatgpt_base_url),
            config.auth_keyring_backend_kind(),
            auth_route_config.as_ref(),
        )
        .await
        .ok()
        .flatten();
        if auth.as_ref().is_some_and(CodexAuth::is_chatgpt_auth) {
            matching.push((account_id, account_home));
        }
    }
    matching
}

#[cfg(test)]
#[path = "weekly_window_scheduler_tests.rs"]
mod tests;
