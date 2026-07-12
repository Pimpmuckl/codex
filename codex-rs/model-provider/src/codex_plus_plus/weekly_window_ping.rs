use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use codex_api::ApiError;
use codex_api::Compression;
use codex_api::ResponseEvent;
use codex_api::ResponsesClient;
use codex_api::RetryConfig;
use codex_api::TransportError;
use codex_http_client::ClientRouteClass;
use codex_http_client::HttpClientFactory;
use codex_http_client::HttpTransport;
use codex_http_client::Request;
use codex_http_client::ReqwestTransport;
use codex_http_client::Response;
use codex_http_client::StreamResponse;
use codex_login::AuthManager;
use codex_login::AuthRouteConfig;
use codex_login::CodexAuth;
use codex_login::default_client::build_default_reqwest_client_for_route_async;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::OPENAI_PROVIDER_ID;
use http::HeaderMap;
use serde_json::json;
use tokio::time::Instant;

use crate::auth::auth_provider_from_auth_manager;
use crate::provider::provider_uses_first_party_auth_path;

const PING_TIMEOUT: Duration = Duration::from_secs(30);

struct DeadlineTransport(ReqwestTransport, Instant);
impl HttpTransport for DeadlineTransport {
    async fn execute(&self, request: Request) -> Result<Response, TransportError> {
        self.0.execute(request).await
    }
    async fn stream(&self, mut request: Request) -> Result<StreamResponse, TransportError> {
        request.timeout = Some(self.1.saturating_duration_since(Instant::now()));
        self.0.stream(request).await
    }
}

pub struct WeeklyWindowPingRequest {
    pub account_codex_home: PathBuf,
    pub model: String,
    pub model_provider_id: String,
    pub model_provider: ModelProviderInfo,
    pub chatgpt_base_url: String,
    pub auth_route_config: Option<AuthRouteConfig>,
    pub forced_chatgpt_workspace_id: Option<Vec<String>>,
    pub http_client_factory: HttpClientFactory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeeklyWindowPingOutcome {
    Completed,
    DefiniteRejection,
    Ambiguous,
    LoginRequired,
    RecoveryRequired,
}

pub async fn ping_weekly_window(request: WeeklyWindowPingRequest) -> WeeklyWindowPingOutcome {
    if request.model_provider_id != OPENAI_PROVIDER_ID
        || !request.model_provider.is_openai()
        || !provider_uses_first_party_auth_path(&request.model_provider)
        || request.model_provider.base_url
            != ModelProviderInfo::create_openai_provider(/*base_url*/ None).base_url
    {
        return WeeklyWindowPingOutcome::DefiniteRejection;
    }

    ping_weekly_window_with_timeout(request, PING_TIMEOUT).await
}

async fn ping_weekly_window_with_timeout(
    request: WeeklyWindowPingRequest,
    timeout: Duration,
) -> WeeklyWindowPingOutcome {
    let deadline = Instant::now() + timeout;
    match tokio::time::timeout_at(deadline, ping_weekly_window_inner(request, deadline)).await {
        Ok(outcome) => outcome,
        Err(_) => WeeklyWindowPingOutcome::Ambiguous,
    }
}

async fn ping_weekly_window_inner(
    request: WeeklyWindowPingRequest,
    deadline: Instant,
) -> WeeklyWindowPingOutcome {
    let Some(auth_manager) = AuthManager::new_from_file_auth(
        request.account_codex_home.clone(),
        request.forced_chatgpt_workspace_id.clone(),
        Some(request.chatgpt_base_url.clone()),
        request.auth_route_config.clone(),
    )
    .await
    .map(Arc::new) else {
        return WeeklyWindowPingOutcome::LoginRequired;
    };
    let Some(auth) = auth_manager.auth_cached() else {
        return WeeklyWindowPingOutcome::LoginRequired;
    };
    let mut unauthorized_recovery = auth_manager.unauthorized_recovery();
    if let AttemptOutcome::Finished(outcome) =
        send_once(&request, &auth_manager, &auth, deadline).await
    {
        return outcome;
    }
    let Ok(reload) = unauthorized_recovery.next().await else {
        return WeeklyWindowPingOutcome::RecoveryRequired;
    };
    if reload.auth_state_changed() != Some(true) {
        return WeeklyWindowPingOutcome::RecoveryRequired;
    }
    let Some(reloaded_auth) = auth_manager.auth_cached() else {
        return WeeklyWindowPingOutcome::RecoveryRequired;
    };
    if auth.get_account_id() != reloaded_auth.get_account_id()
        || auth.get_chatgpt_user_id() != reloaded_auth.get_chatgpt_user_id()
        || auth.is_workspace_account() != reloaded_auth.is_workspace_account()
    {
        return WeeklyWindowPingOutcome::RecoveryRequired;
    }
    match send_once(&request, &auth_manager, &auth, deadline).await {
        AttemptOutcome::Finished(outcome) => outcome,
        AttemptOutcome::Unauthorized => WeeklyWindowPingOutcome::RecoveryRequired,
    }
}

enum AttemptOutcome {
    Finished(WeeklyWindowPingOutcome),
    Unauthorized,
}

async fn send_once(
    request: &WeeklyWindowPingRequest,
    auth_manager: &Arc<AuthManager>,
    expected_auth: &CodexAuth,
    deadline: Instant,
) -> AttemptOutcome {
    let root = request.chatgpt_base_url.trim_end_matches('/');
    let base_url = root.trim_end_matches("/codex");
    let provider_info =
        ModelProviderInfo::create_openai_provider(Some(format!("{base_url}/codex")));
    let Ok(mut provider) = provider_info.to_api_provider(Some(expected_auth.auth_mode())) else {
        return AttemptOutcome::Finished(WeeklyWindowPingOutcome::DefiniteRejection);
    };
    provider.retry = RetryConfig {
        max_attempts: 0,
        base_delay: Duration::ZERO,
        retry_429: false,
        retry_5xx: false,
        retry_transport: false,
    };
    let request_url = provider.url_for_path("responses");
    let Ok(client) = build_default_reqwest_client_for_route_async(
        request.http_client_factory.clone(),
        request_url,
        ClientRouteClass::Api,
    )
    .await
    else {
        return AttemptOutcome::Finished(WeeklyWindowPingOutcome::DefiniteRejection);
    };
    let auth = auth_provider_from_auth_manager(Arc::clone(auth_manager), expected_auth);
    let transport = DeadlineTransport(ReqwestTransport::new(client), deadline);
    let client = ResponsesClient::new(transport, provider, auth);
    let body = json!({
        "model": request.model,
        "instructions": "",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "Reply OK."}]
        }],
        "store": false,
        "stream": true,
        "include": [],
        "max_output_tokens": 8
    });
    let mut stream = match client
        .stream(
            body,
            HeaderMap::new(),
            Compression::None,
            /*turn_state*/ None,
        )
        .await
    {
        Ok(stream) => stream,
        Err(ApiError::Transport(TransportError::Http { status, .. }))
            if status == http::StatusCode::UNAUTHORIZED =>
        {
            return AttemptOutcome::Unauthorized;
        }
        Err(error) => return AttemptOutcome::Finished(classify_error(&error)),
    };

    while let Some(event) = stream.rx_event.recv().await {
        match event {
            Ok(ResponseEvent::Completed { .. }) => {
                return AttemptOutcome::Finished(WeeklyWindowPingOutcome::Completed);
            }
            Ok(_) => {}
            Err(error) => return AttemptOutcome::Finished(classify_error(&error)),
        }
    }
    AttemptOutcome::Finished(WeeklyWindowPingOutcome::Ambiguous)
}

fn classify_error(error: &ApiError) -> WeeklyWindowPingOutcome {
    match error {
        ApiError::Transport(TransportError::Http { status, .. }) | ApiError::Api { status, .. }
            if status.is_client_error() =>
        {
            WeeklyWindowPingOutcome::DefiniteRejection
        }
        ApiError::QuotaExceeded
        | ApiError::UsageLimitReached { .. }
        | ApiError::RateLimit(_)
        | ApiError::InvalidRequest { .. }
        | ApiError::CyberPolicy { .. } => WeeklyWindowPingOutcome::DefiniteRejection,
        ApiError::Transport(_)
        | ApiError::Api { .. }
        | ApiError::Stream(_)
        | ApiError::Retryable { .. }
        | ApiError::ServerOverloaded
        | ApiError::ContextWindowExceeded
        | ApiError::UsageNotIncluded => WeeklyWindowPingOutcome::Ambiguous,
    }
}

#[cfg(test)]
#[path = "weekly_window_ping_tests.rs"]
mod tests;
