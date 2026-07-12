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
use codex_login::AccountId;
use codex_login::AccountStore;
use codex_login::AuthCredentialsStoreMode;
use codex_login::AuthKeyringBackendKind;
use codex_login::AuthManager;
use codex_login::AuthRouteConfig;
use codex_login::CodexAuth;
use codex_login::RefreshTokenError;
use codex_login::default_client::build_default_reqwest_client_for_route_async;
use codex_login::load_auth_dot_json;
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
    pub root_codex_home: PathBuf,
    pub account_id: AccountId,
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
}

pub async fn ping_weekly_window(request: WeeklyWindowPingRequest) -> WeeklyWindowPingOutcome {
    if request.model_provider_id != OPENAI_PROVIDER_ID
        || !request.model_provider.is_openai()
        || !provider_uses_first_party_auth_path(&request.model_provider)
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
    let Some(auth) = auth_manager.auth().await else {
        return WeeklyWindowPingOutcome::LoginRequired;
    };
    if auth_manager.refresh_failure_for_auth(&auth).is_some() {
        mark_login_required_if_identity_matches(&request, &auth_manager, &auth).await;
        return WeeklyWindowPingOutcome::LoginRequired;
    }

    let mut unauthorized_recovery = auth_manager.unauthorized_recovery();
    loop {
        match send_once(&request, &auth_manager, &auth, deadline).await {
            AttemptOutcome::Finished(outcome) => return outcome,
            AttemptOutcome::Unauthorized => {}
        }
        if !unauthorized_recovery.has_next() {
            return WeeklyWindowPingOutcome::DefiniteRejection;
        }
        let recovery_task = tokio::spawn(async move {
            let result = unauthorized_recovery.next().await;
            (unauthorized_recovery, result)
        });
        let Ok((next_recovery, result)) = recovery_task.await else {
            return WeeklyWindowPingOutcome::Ambiguous;
        };
        unauthorized_recovery = next_recovery;
        match result {
            Ok(_) => {}
            Err(RefreshTokenError::Transient(_)) => {
                return WeeklyWindowPingOutcome::DefiniteRejection;
            }
            Err(RefreshTokenError::Permanent(_)) => {
                mark_login_required_if_identity_matches(&request, &auth_manager, &auth).await;
                return WeeklyWindowPingOutcome::LoginRequired;
            }
        }
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

async fn mark_login_required_if_identity_matches(
    request: &WeeklyWindowPingRequest,
    auth_manager: &AuthManager,
    expected_auth: &CodexAuth,
) {
    let Some(current_auth) = auth_manager.auth_cached() else {
        return;
    };
    if expected_auth.get_account_id() != current_auth.get_account_id()
        || expected_auth.get_chatgpt_user_id() != current_auth.get_chatgpt_user_id()
        || expected_auth.is_workspace_account() != current_auth.is_workspace_account()
    {
        return;
    }
    let account_home = request.account_codex_home.clone();
    let root_codex_home = request.root_codex_home.clone();
    let account_id = request.account_id.clone();
    let expected_account_id = expected_auth.get_account_id();
    let expected_user_id = expected_auth.get_chatgpt_user_id();
    let expected_workspace = expected_auth.is_workspace_account();
    let error = tokio::task::spawn_blocking(move || {
        let Ok(Some(current_auth_json)) = load_auth_dot_json(
            &account_home,
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
        ) else {
            return None;
        };
        let tokens = current_auth_json.tokens.as_ref()?;
        if tokens.account_id != expected_account_id
            || tokens.id_token.chatgpt_user_id != expected_user_id
            || tokens.id_token.is_workspace_account() != expected_workspace
        {
            return None;
        }
        AccountStore::new(root_codex_home)
            .record_login_required_if_auth_matches(&account_id, &current_auth_json)
            .err()
    })
    .await;
    if let Ok(Some(error)) = error {
        tracing::warn!(%error, account_id = %request.account_id, "failed to mark weekly-window account as login required");
    }
}

#[cfg(test)]
#[path = "weekly_window_ping_tests.rs"]
mod tests;
