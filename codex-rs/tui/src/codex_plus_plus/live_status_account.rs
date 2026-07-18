use crate::app_server_session::app_server_rate_limit_snapshots;
use crate::status::StatusAccountDisplay;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::Account;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::GetAccountParams;
use codex_app_server_protocol::GetAccountRateLimitsResponse;
use codex_app_server_protocol::GetAccountResponse;
use codex_app_server_protocol::RateLimitSnapshot;
use codex_app_server_protocol::RequestId;
use codex_protocol::account::PlanType;
use color_eyre::eyre::Result;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::eyre;
use std::time::Duration;
use uuid::Uuid;

const LIVE_STATUS_ACCOUNT_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 15);

#[derive(Debug)]
pub(crate) struct LiveStatusAccountSnapshot {
    pub(crate) account_display: StatusAccountDisplay,
    pub(crate) plan_type: PlanType,
    pub(crate) rate_limits: Vec<RateLimitSnapshot>,
}

pub(crate) async fn fetch_live_status_account_snapshot(
    request_handle: AppServerRequestHandle,
) -> Result<LiveStatusAccountSnapshot> {
    tokio::time::timeout(LIVE_STATUS_ACCOUNT_TIMEOUT, async {
        let account_before = read_account(&request_handle).await?;
        let rate_limits: GetAccountRateLimitsResponse = request_handle
            .request_typed(ClientRequest::GetAccountRateLimits {
                request_id: RequestId::String(format!(
                    "status-account-rate-limits-{}",
                    Uuid::new_v4()
                )),
                params: None,
            })
            .await
            .wrap_err("account/rateLimits/read failed during /status refresh")?;
        let account_after = read_account(&request_handle).await?;

        if account_before.account != account_after.account {
            return Err(eyre!("active account changed during /status refresh"));
        }

        let Some(Account::Chatgpt { email, plan_type }) = account_after.account else {
            return Err(eyre!("active account is not a ChatGPT account"));
        };

        Ok(LiveStatusAccountSnapshot {
            account_display: StatusAccountDisplay::ChatGpt {
                email,
                plan: Some(crate::status::plan_type_display_name(plan_type)),
            },
            plan_type,
            rate_limits: app_server_rate_limit_snapshots(rate_limits),
        })
    })
    .await
    .map_err(|_| eyre!("live /status account refresh timed out"))?
}

async fn read_account(request_handle: &AppServerRequestHandle) -> Result<GetAccountResponse> {
    request_handle
        .request_typed(ClientRequest::GetAccount {
            request_id: RequestId::String(format!("status-account-{}", Uuid::new_v4())),
            params: GetAccountParams {
                refresh_token: false,
            },
        })
        .await
        .wrap_err("account/read failed during /status refresh")
}
