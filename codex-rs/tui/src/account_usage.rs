use crate::legacy_core::config::Config;
use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use codex_backend_client::Client as BackendClient;
use codex_backend_client::RateLimitsWithResetCredits;
use codex_backend_client::RequestError;
use codex_login::AccountId;
use codex_login::AccountStore;
use codex_login::AuthCredentialsStoreMode;
use codex_login::AuthKeyringBackendKind;
use codex_login::CodexAuth;
use codex_login::refresh_auth_from_storage;
use codex_protocol::protocol::RateLimitSnapshot;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::task::JoinSet;

const FETCH_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AccountUsage {
    pub(crate) primary_window_minutes: Option<i64>,
    pub(crate) five_hour_reset_at: Option<i64>,
    pub(crate) five_hour_remaining_percent: Option<u8>,
    pub(crate) five_hour_exhausted: bool,
    pub(crate) weekly_reset_at: Option<i64>,
    pub(crate) weekly_remaining_percent: Option<u8>,
    pub(crate) weekly_exhausted: bool,
}

impl AccountUsage {
    fn exhausted_until(self) -> Option<i64> {
        [
            self.five_hour_reset_at.filter(|_| self.five_hour_exhausted),
            self.weekly_reset_at.filter(|_| self.weekly_exhausted),
        ]
        .into_iter()
        .flatten()
        .max()
    }
}

pub(crate) async fn load(
    config: &Config,
    accounts: &[(AccountId, PathBuf)],
    store: &AccountStore,
) -> HashMap<AccountId, AccountUsage> {
    let mut tasks = JoinSet::new();
    for (account_id, account_home) in accounts {
        let account_id = account_id.clone();
        let account_home = account_home.clone();
        let chatgpt_base_url = config.chatgpt_base_url.clone();
        let auth_route_config = config.auth_route_config();
        tasks.spawn(async move {
            let result = tokio::time::timeout(
                FETCH_TIMEOUT,
                fetch(account_home, chatgpt_base_url, auth_route_config),
            )
            .await
            .map_err(|_| anyhow!("rate-limit request timed out"))
            .and_then(std::convert::identity);
            (account_id, result)
        });
    }

    let mut usage = HashMap::new();
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok((account_id, Ok(account_usage))) => {
                if let Some(resets_at) = account_usage.exhausted_until()
                    && let Err(err) = store.record_usage_limit_resets_at(&account_id, resets_at)
                {
                    tracing::warn!(
                        %account_id,
                        %err,
                        "failed to persist imported account usage limit reset"
                    );
                }
                usage.insert(account_id, account_usage);
            }
            Ok((account_id, Err(err))) => {
                tracing::warn!(%account_id, %err, "failed to load imported account usage");
            }
            Err(err) => tracing::warn!(%err, "imported account usage task failed"),
        }
    }
    usage
}

async fn fetch(
    account_home: PathBuf,
    chatgpt_base_url: String,
    auth_route_config: Option<codex_login::AuthRouteConfig>,
) -> Result<AccountUsage> {
    let auth = CodexAuth::from_auth_storage(
        &account_home,
        AuthCredentialsStoreMode::File,
        Some(&chatgpt_base_url),
        AuthKeyringBackendKind::default(),
        auth_route_config.as_ref(),
    )
    .await?
    .context("imported account is not authenticated")?;

    match fetch_with_auth(&auth, &chatgpt_base_url).await {
        Err(err) if is_unauthorized(&err) => {
            let auth = refresh_auth_from_storage(
                &account_home,
                AuthCredentialsStoreMode::File,
                Some(&chatgpt_base_url),
                AuthKeyringBackendKind::default(),
                auth_route_config.as_ref(),
            )
            .await?
            .context("imported account is not authenticated")?;
            fetch_with_auth(&auth, &chatgpt_base_url).await
        }
        result => result,
    }
}

async fn fetch_with_auth(auth: &CodexAuth, chatgpt_base_url: &str) -> Result<AccountUsage> {
    let response = BackendClient::from_auth(chatgpt_base_url, auth)?
        .get_rate_limits_with_reset_credits()
        .await?;
    Ok(account_usage(&response))
}

fn is_unauthorized(err: &anyhow::Error) -> bool {
    err.chain().any(|source| {
        source
            .downcast_ref::<RequestError>()
            .is_some_and(RequestError::is_unauthorized)
    })
}

fn account_usage(response: &RateLimitsWithResetCredits) -> AccountUsage {
    response
        .rate_limits
        .iter()
        .find(|snapshot| snapshot.limit_id.as_deref() == Some("codex"))
        .or_else(|| response.rate_limits.first())
        .map(account_usage_from_snapshot)
        .unwrap_or_default()
}

fn account_usage_from_snapshot(snapshot: &RateLimitSnapshot) -> AccountUsage {
    let remaining_percent =
        |used_percent: f64| (100.0 - used_percent).clamp(0.0, 100.0).round() as u8;
    AccountUsage {
        primary_window_minutes: snapshot
            .primary
            .as_ref()
            .and_then(|window| window.window_minutes),
        five_hour_reset_at: snapshot
            .primary
            .as_ref()
            .and_then(|window| window.resets_at),
        five_hour_remaining_percent: snapshot
            .primary
            .as_ref()
            .map(|window| remaining_percent(window.used_percent)),
        five_hour_exhausted: snapshot
            .primary
            .as_ref()
            .is_some_and(|window| window.used_percent >= 100.0),
        weekly_reset_at: snapshot
            .secondary
            .as_ref()
            .and_then(|window| window.resets_at),
        weekly_remaining_percent: snapshot
            .secondary
            .as_ref()
            .map(|window| remaining_percent(window.used_percent)),
        weekly_exhausted: snapshot
            .secondary
            .as_ref()
            .is_some_and(|window| window.used_percent >= 100.0),
    }
}

#[cfg(test)]
#[path = "account_usage_tests.rs"]
mod tests;
