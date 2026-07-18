use crate::status::StatusAccountDisplay;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::GetAuthStatusParams;
use codex_app_server_protocol::GetAuthStatusResponse;
use codex_app_server_protocol::RateLimitSnapshot;
use codex_app_server_protocol::RequestId;
use codex_backend_client::Client as BackendClient;
use codex_login::CodexAuth;
use codex_login::token_data::parse_chatgpt_jwt_claims;
use codex_protocol::account::PlanType;
use codex_protocol::auth::PlanType as AuthPlanType;
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

#[derive(Debug)]
struct CapturedStatusAuth {
    access_token: String,
    account_id: String,
    email: Option<String>,
    plan_type: PlanType,
    raw_plan_type: Option<String>,
}

pub(crate) async fn fetch_live_status_account_snapshot(
    request_handle: AppServerRequestHandle,
    chatgpt_base_url: String,
) -> Result<LiveStatusAccountSnapshot> {
    tokio::time::timeout(LIVE_STATUS_ACCOUNT_TIMEOUT, async {
        let access_token = read_access_token(&request_handle).await?;
        let captured_auth = captured_status_auth(access_token)?;
        let auth = CodexAuth::from_external_chatgpt_tokens(
            &captured_auth.access_token,
            &captured_auth.account_id,
            captured_auth.raw_plan_type.as_deref(),
        )
        .wrap_err("could not capture the active ChatGPT auth for /status")?;
        let response = BackendClient::from_auth(chatgpt_base_url, &auth)
            .map_err(|err| eyre!("could not construct the /status rate-limit client: {err}"))?
            .get_rate_limits_with_reset_credits()
            .await
            .map_err(|err| eyre!("rate-limit read failed during /status refresh: {err}"))?;
        if response.rate_limits.is_empty() {
            return Err(eyre!(
                "rate-limit read returned no snapshots during /status refresh"
            ));
        }

        let current_access_token = read_access_token(&request_handle).await?;
        if current_access_token != captured_auth.access_token {
            return Err(eyre!("active account changed during /status refresh"));
        }

        Ok(LiveStatusAccountSnapshot {
            account_display: StatusAccountDisplay::ChatGpt {
                email: captured_auth.email,
                plan: Some(crate::status::plan_type_display_name(
                    captured_auth.plan_type,
                )),
            },
            plan_type: captured_auth.plan_type,
            rate_limits: response
                .rate_limits
                .into_iter()
                .map(RateLimitSnapshot::from)
                .collect(),
        })
    })
    .await
    .map_err(|_| eyre!("live /status account refresh timed out"))?
}

fn captured_status_auth(access_token: String) -> Result<CapturedStatusAuth> {
    let claims = parse_chatgpt_jwt_claims(&access_token)
        .wrap_err("active ChatGPT access token has invalid claims")?;
    let account_id = claims
        .chatgpt_account_id
        .clone()
        .ok_or_else(|| eyre!("active ChatGPT access token has no stable account identity"))?;
    let raw_plan_type = claims.get_chatgpt_plan_type_raw();
    let plan_type = claims
        .chatgpt_plan_type
        .unwrap_or_else(|| AuthPlanType::Unknown("unknown".to_string()))
        .into();
    Ok(CapturedStatusAuth {
        access_token,
        account_id,
        email: claims.email,
        plan_type,
        raw_plan_type,
    })
}

async fn read_access_token(request_handle: &AppServerRequestHandle) -> Result<String> {
    let response: GetAuthStatusResponse = request_handle
        .request_typed(ClientRequest::GetAuthStatus {
            request_id: RequestId::String(format!("status-account-{}", Uuid::new_v4())),
            params: GetAuthStatusParams {
                include_token: Some(true),
                refresh_token: Some(false),
            },
        })
        .await
        .wrap_err("account/getAuthStatus failed during /status refresh")?;
    response
        .auth_token
        .ok_or_else(|| eyre!("active ChatGPT access token is unavailable"))
}

#[cfg(test)]
#[path = "live_status_account_tests.rs"]
mod tests;
