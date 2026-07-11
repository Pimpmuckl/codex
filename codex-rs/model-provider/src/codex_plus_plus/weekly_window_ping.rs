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
use codex_http_client::ReqwestTransport;
use codex_login::AccountId;
use codex_login::AccountStore;
use codex_login::AuthCredentialsStoreMode;
use codex_login::AuthKeyringBackendKind;
use codex_login::AuthManager;
use codex_login::AuthRouteConfig;
use codex_login::CodexAuth;
use codex_login::RefreshTokenError;
use codex_login::default_client::build_default_reqwest_client_for_route;
use codex_login::load_auth_dot_json;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::OPENAI_PROVIDER_ID;
use http::HeaderMap;
use serde_json::json;

use crate::auth::auth_provider_from_auth_manager;
use crate::provider::provider_uses_first_party_auth_path;

const PING_TIMEOUT: Duration = Duration::from_secs(30);

/// Inputs for one isolated Codex++ weekly-window request.
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

/// Scheduler-safe result categories for a one-shot weekly-window request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeeklyWindowPingOutcome {
    Completed,
    DefiniteRejection,
    Ambiguous,
    LoginRequired,
}

/// Sends one bounded first-party Responses request using only the imported account's auth home.
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
    match tokio::time::timeout(timeout, ping_weekly_window_inner(request)).await {
        Ok(outcome) => outcome,
        Err(_) => WeeklyWindowPingOutcome::Ambiguous,
    }
}

async fn ping_weekly_window_inner(request: WeeklyWindowPingRequest) -> WeeklyWindowPingOutcome {
    let auth_manager = Arc::new(
        AuthManager::new(
            request.account_codex_home.clone(),
            /*enable_codex_api_key_env*/ false,
            AuthCredentialsStoreMode::File,
            request.forced_chatgpt_workspace_id.clone(),
            Some(request.chatgpt_base_url.clone()),
            AuthKeyringBackendKind::default(),
            request.auth_route_config.clone(),
        )
        .await,
    );
    if auth_manager.active_account_id().is_some() {
        return WeeklyWindowPingOutcome::DefiniteRejection;
    }
    let Some(auth) = auth_manager.auth().await else {
        return WeeklyWindowPingOutcome::LoginRequired;
    };
    if auth_manager.refresh_failure_for_auth(&auth).is_some() {
        mark_login_required_if_identity_matches(&request, &auth_manager, &auth);
        return WeeklyWindowPingOutcome::LoginRequired;
    }

    let mut unauthorized_recovery = auth_manager.unauthorized_recovery();
    loop {
        match send_once(&request, &auth_manager, &auth).await {
            AttemptOutcome::Finished(outcome) => return outcome,
            AttemptOutcome::Unauthorized => {}
        }
        if !unauthorized_recovery.has_next() {
            return WeeklyWindowPingOutcome::DefiniteRejection;
        }
        match unauthorized_recovery.next().await {
            Ok(_) => {}
            Err(RefreshTokenError::Transient(_)) => {
                return WeeklyWindowPingOutcome::DefiniteRejection;
            }
            Err(RefreshTokenError::Permanent(_)) => {
                mark_login_required_if_identity_matches(&request, &auth_manager, &auth);
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
    provider.stream_idle_timeout = PING_TIMEOUT;
    let request_url = provider.url_for_path("responses");
    let Ok(client) = build_default_reqwest_client_for_route(
        &request.http_client_factory,
        &request_url,
        ClientRouteClass::Api,
    ) else {
        return AttemptOutcome::Finished(WeeklyWindowPingOutcome::DefiniteRejection);
    };
    let auth = auth_provider_from_auth_manager(Arc::clone(auth_manager), expected_auth);
    let client = ResponsesClient::new(ReqwestTransport::new(client), provider, auth);
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
        | ApiError::Retryable { .. }
        | ApiError::RateLimit(_)
        | ApiError::InvalidRequest { .. }
        | ApiError::CyberPolicy { .. }
        | ApiError::ServerOverloaded => WeeklyWindowPingOutcome::DefiniteRejection,
        ApiError::Transport(_)
        | ApiError::Api { .. }
        | ApiError::Stream(_)
        | ApiError::ContextWindowExceeded
        | ApiError::UsageNotIncluded => WeeklyWindowPingOutcome::Ambiguous,
    }
}

fn mark_login_required_if_identity_matches(
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
    let Ok(Some(current_auth_json)) = load_auth_dot_json(
        &request.account_codex_home,
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    ) else {
        return;
    };
    let Some(tokens) = current_auth_json.tokens.as_ref() else {
        return;
    };
    if tokens.account_id != expected_auth.get_account_id()
        || tokens.id_token.chatgpt_user_id != expected_auth.get_chatgpt_user_id()
        || tokens.id_token.is_workspace_account() != expected_auth.is_workspace_account()
    {
        return;
    }
    if let Err(error) = AccountStore::new(request.root_codex_home.clone())
        .record_login_required_if_auth_matches(&request.account_id, &current_auth_json)
    {
        tracing::warn!(%error, account_id = %request.account_id, "failed to mark weekly-window account as login required");
    }
}

#[cfg(test)]
#[path = "weekly_window_ping_tests.rs"]
mod tests;
