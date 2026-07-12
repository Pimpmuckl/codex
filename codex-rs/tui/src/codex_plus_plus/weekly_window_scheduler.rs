use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use chrono::Utc;
use codex_config::WeeklyUsageWindowAutoStart;
use codex_login::AccountId;
use codex_login::AccountStore;
use codex_login::WeeklyWindowAttemptDecision;
use codex_login::WeeklyWindowAttemptOutcome;
use codex_login::WeeklyWindowRetryableError;
use codex_login::WeeklyWindowUsage;
use codex_model_provider::WeeklyWindowPingOutcome;
use codex_model_provider::WeeklyWindowPingRequest;
use codex_model_provider::ping_weekly_window;
use codex_model_provider::preflight_weekly_window_ping;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

use crate::account_usage;
use crate::legacy_core::config::Config;

const SCAN_INTERVAL: Duration = Duration::from_secs(5 * 60);
const MAX_STATUS_ACCOUNTS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WeeklyWindowStatus {
    Waiting(Option<u8>),
    Started(Option<u8>),
    Retrying(Option<u8>),
    SignInRequired,
    Failed,
}

pub(crate) struct WeeklyWindowScheduler {
    state: watch::Sender<bool>,
    statuses: Arc<Mutex<HashMap<AccountId, WeeklyWindowStatus>>>,
    _task: JoinHandle<()>,
}

impl WeeklyWindowScheduler {
    pub(crate) fn spawn(config: Config, model: String) -> Self {
        let enabled = config.weekly_usage_window_auto_start == WeeklyUsageWindowAutoStart::Enabled;
        let (state, receiver) = watch::channel(enabled);
        let statuses = Arc::new(Mutex::new(HashMap::new()));
        let task_statuses = Arc::clone(&statuses);
        let task = tokio::spawn(run_schedule(
            move |scan_control| {
                scan(
                    config.clone(),
                    model.clone(),
                    scan_control,
                    Arc::clone(&task_statuses),
                )
            },
            receiver,
        ));
        Self {
            state,
            statuses,
            _task: task,
        }
    }

    pub(crate) fn set_enabled(&self, on: bool) {
        if !on {
            self.statuses
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear();
        }
        let _ = self
            .state
            .send_if_modified(|v| std::mem::replace(v, on) != on);
    }

    pub(crate) fn statuses(&self) -> HashMap<AccountId, WeeklyWindowStatus> {
        self.statuses
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

async fn run_schedule<F, Fut>(mut scan: F, mut control: watch::Receiver<bool>)
where
    F: FnMut(watch::Receiver<bool>) -> Fut,
    Fut: Future<Output = ()>,
{
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now(), SCAN_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = interval.tick(), if *control.borrow() => scan(control.clone()).await,
            changed = control.changed() => match changed {
                Err(_) => return,
                Ok(()) if *control.borrow_and_update() => {
                    scan(control.clone()).await;
                    interval.reset();
                }
                Ok(()) => {}
            }
        }
    }
}

async fn scan(
    config: Config,
    model: String,
    control: watch::Receiver<bool>,
    status_sink: Arc<Mutex<HashMap<AccountId, WeeklyWindowStatus>>>,
) {
    status_sink
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    let http_client_factory = config.http_client_factory();
    if let Err(outcome) = preflight_weekly_window_ping(
        &config.model_provider_id,
        &config.model_provider,
        &config.chatgpt_base_url,
        &http_client_factory,
    ) {
        tracing::warn!(?outcome, "weekly-window scheduler unsupported");
        return;
    }
    let store = AccountStore::new(config.codex_home.to_path_buf());
    let _scan_lease = match store.try_acquire_weekly_window_scan() {
        Ok(Some(lease)) => lease,
        Ok(None) => return,
        Err(err) => {
            tracing::warn!(%err, "weekly-window scheduler could not acquire scan lease");
            return;
        }
    };
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
    if control.has_changed().unwrap_or(true) {
        return;
    }
    let loaded = account_usage::load(&config, &accounts, &store).await;
    let mut statuses = HashMap::new();
    for (account_id, attempted_auth) in &loaded.login_required {
        record_status(
            &mut statuses,
            account_id,
            WeeklyWindowStatus::SignInRequired,
        );
        if let Err(err) = store.record_login_required_if_auth_matches(account_id, attempted_auth) {
            tracing::warn!(%account_id, %err, "weekly-window scheduler could not record login failure");
        }
    }

    for (account_id, account_home) in accounts {
        if control.has_changed().unwrap_or(true) {
            return;
        }
        if loaded.login_required.contains_key(&account_id) {
            continue;
        }
        let account_usage = loaded.usage.get(&account_id);
        let remaining = account_usage.and_then(|usage| usage.weekly_remaining_percent);
        let usage = weekly_usage(account_usage);
        let attempt = match store.begin_weekly_window_attempt(
            &account_id,
            usage,
            Utc::now().timestamp(),
        ) {
            Ok(WeeklyWindowAttemptDecision::Ready(attempt)) => attempt,
            Ok(WeeklyWindowAttemptDecision::NotDue | WeeklyWindowAttemptDecision::Locked) => {
                record_status(
                    &mut statuses,
                    &account_id,
                    if account_usage.is_some() {
                        WeeklyWindowStatus::Waiting(remaining)
                    } else {
                        WeeklyWindowStatus::Failed
                    },
                );
                continue;
            }
            Ok(WeeklyWindowAttemptDecision::StateUnavailable) => {
                record_status(&mut statuses, &account_id, WeeklyWindowStatus::Failed);
                continue;
            }
            Err(err) => {
                tracing::warn!(%account_id, %err, "weekly-window scheduler could not begin attempt");
                record_status(&mut statuses, &account_id, WeeklyWindowStatus::Failed);
                continue;
            }
        };
        let outcome = ping_weekly_window(WeeklyWindowPingRequest {
            account_codex_home: account_home.clone(),
            model: model.clone(),
            model_provider_id: config.model_provider_id.clone(),
            model_provider: config.model_provider.clone(),
            chatgpt_base_url: config.chatgpt_base_url.clone(),
            auth_route_config: config.auth_route_config(),
            forced_chatgpt_workspace_id: config.forced_chatgpt_workspace_id.clone(),
            http_client_factory: http_client_factory.clone(),
        })
        .await;
        let (refreshed_usage, refreshed_remaining) = if outcome
            == WeeklyWindowPingOutcome::Completed
        {
            let refreshed =
                account_usage::load(&config, &[(account_id.clone(), account_home)], &store).await;
            let refreshed_account_usage = refreshed.usage.get(&account_id);
            (
                weekly_usage(refreshed_account_usage),
                refreshed_account_usage.and_then(|usage| usage.weekly_remaining_percent),
            )
        } else {
            (WeeklyWindowUsage::Missing, remaining)
        };
        let status = match outcome {
            WeeklyWindowPingOutcome::Completed => WeeklyWindowStatus::Started(refreshed_remaining),
            WeeklyWindowPingOutcome::LoginRequired | WeeklyWindowPingOutcome::RecoveryRequired => {
                WeeklyWindowStatus::SignInRequired
            }
            WeeklyWindowPingOutcome::DefiniteRejection | WeeklyWindowPingOutcome::Ambiguous => {
                WeeklyWindowStatus::Retrying(remaining)
            }
            WeeklyWindowPingOutcome::UnsupportedConfiguration
            | WeeklyWindowPingOutcome::UnsupportedRouting => WeeklyWindowStatus::Failed,
        };
        if let Err(err) = attempt.finish(
            attempt_outcome(outcome, refreshed_usage),
            Utc::now().timestamp(),
        ) {
            tracing::warn!(%account_id, %err, "weekly-window scheduler could not finish attempt");
            record_status(&mut statuses, &account_id, WeeklyWindowStatus::Failed);
        } else {
            record_status(&mut statuses, &account_id, status);
        }
    }
    if !control.has_changed().unwrap_or(true) {
        *status_sink
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = statuses;
    }
}

fn record_status(
    statuses: &mut HashMap<AccountId, WeeklyWindowStatus>,
    account_id: &AccountId,
    status: WeeklyWindowStatus,
) {
    if statuses.len() < MAX_STATUS_ACCOUNTS || statuses.contains_key(&account_id) {
        statuses.insert(account_id.clone(), status);
    }
}

fn weekly_usage(usage: Option<&account_usage::AccountUsage>) -> WeeklyWindowUsage {
    let Some(usage) = usage else {
        return WeeklyWindowUsage::Missing;
    };
    match usage.weekly_unused {
        Some(unused) => WeeklyWindowUsage::Present {
            unused,
            resets_at: usage.weekly_reset_at,
        },
        None => WeeklyWindowUsage::Missing,
    }
}

fn attempt_outcome(
    outcome: WeeklyWindowPingOutcome,
    refreshed_usage: WeeklyWindowUsage,
) -> WeeklyWindowAttemptOutcome {
    match outcome {
        WeeklyWindowPingOutcome::Completed => {
            WeeklyWindowAttemptOutcome::Completed { refreshed_usage }
        }
        WeeklyWindowPingOutcome::UnsupportedConfiguration
        | WeeklyWindowPingOutcome::UnsupportedRouting => WeeklyWindowAttemptOutcome::Completed {
            refreshed_usage: WeeklyWindowUsage::Missing,
        },
        WeeklyWindowPingOutcome::DefiniteRejection => WeeklyWindowAttemptOutcome::Retryable {
            error: WeeklyWindowRetryableError::Rejected,
        },
        WeeklyWindowPingOutcome::LoginRequired | WeeklyWindowPingOutcome::RecoveryRequired => {
            WeeklyWindowAttemptOutcome::Retryable {
                error: WeeklyWindowRetryableError::LoginRequired,
            }
        }
        WeeklyWindowPingOutcome::Ambiguous => WeeklyWindowAttemptOutcome::Ambiguous,
    }
}

#[cfg(test)]
#[path = "weekly_window_scheduler_tests.rs"]
mod tests;
