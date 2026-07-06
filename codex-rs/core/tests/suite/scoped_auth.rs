use anyhow::Context;
use anyhow::Result;
use base64::Engine;
use chrono::Duration;
use chrono::Utc;
use codex_app_server_protocol::AuthMode;
use codex_core::AuthManager;
use codex_core::ModelProviderInfo;
use codex_core::NewThread;
use codex_core::ThreadManager;
use codex_core::auth::AuthCredentialsStoreMode;
use codex_core::auth::AuthDotJson;
use codex_core::auth::CODEX_API_KEY_ENV_VAR;
use codex_core::auth::REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR;
use codex_core::auth::load_auth_dot_json;
use codex_core::auth::save_auth;
use codex_core::built_in_model_providers;
use codex_core::models_manager::collaboration_mode_presets::CollaborationModesConfig;
use codex_core::token_data::IdTokenInfo;
use codex_core::token_data::TokenData;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::SessionSource;
use codex_protocol::user_input::UserInput;
use core_test_support::load_default_config_for_test;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_models_once_with_etag;
use core_test_support::responses::mount_response_once;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::sse_response;
use core_test_support::skip_if_no_network;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde::Serialize;
use serde_json::json;
use std::ffi::OsString;
use std::sync::Arc;
use tempfile::TempDir;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scoped_account_routing_can_use_different_imported_accounts_without_root_auth_mutation()
-> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let response = sse(vec![ev_response_created("resp1"), ev_completed("resp1")]);
    let requests = mount_sse_sequence(&server, vec![response.clone(), response]).await;
    let codex_home = TempDir::new()?;
    let root_auth = auth_dot_json(
        "root-account",
        "root@example.com",
        "root-access",
        "root-refresh",
    )?;
    save_auth(
        codex_home.path(),
        &root_auth,
        AuthCredentialsStoreMode::File,
    )?;
    write_account_auth(
        &codex_home,
        "acct_first",
        &auth_dot_json("account-a", "a@example.com", "access-a", "refresh-a")?,
    )?;
    write_account_auth_at_path(
        &codex_home,
        "accounts/routed_second/auth.json",
        &auth_dot_json("account-b", "b@example.com", "access-b", "refresh-b")?,
    )?;

    write_account_index(&codex_home, &[("acct_first", 0), ("acct_second", 1)])?;
    let first = start_thread(&codex_home, &server).await?;
    submit_turn(&first).await?;

    write_account_index_with_paths(
        &codex_home,
        &[
            ("acct_second", 0, "accounts/routed_second/auth.json"),
            ("acct_first", 1, "accounts/acct_first/auth.json"),
        ],
    )?;
    let second = start_thread(&codex_home, &server).await?;
    submit_turn(&second).await?;

    let requests = requests.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        request_headers(&requests, "chatgpt-account-id")?,
        vec!["account-a".to_string(), "account-b".to_string()]
    );
    assert_eq!(
        request_headers(&requests, "authorization")?,
        vec!["Bearer access-a".to_string(), "Bearer access-b".to_string()]
    );
    let root_after = load_auth_dot_json(codex_home.path(), AuthCredentialsStoreMode::File)?
        .context("root auth should exist")?;
    assert_eq!(root_after, root_auth);

    Ok(())
}

#[serial_test::serial(openai_base_url)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scoped_account_routing_uses_selected_account_for_model_catalog() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let models =
        mount_models_once_with_etag(&server, ModelsResponse { models: Vec::new() }, "etag-1").await;
    let codex_home = TempDir::new()?;
    save_auth(
        codex_home.path(),
        &auth_dot_json(
            "root-account",
            "root@example.com",
            "root-access",
            "root-refresh",
        )?,
        AuthCredentialsStoreMode::File,
    )?;
    write_account_auth(
        &codex_home,
        "acct_selected",
        &auth_dot_json(
            "selected-account",
            "selected@example.com",
            "selected-access",
            "selected-refresh",
        )?,
    )?;
    write_account_auth(
        &codex_home,
        "acct_other",
        &auth_dot_json(
            "other-account",
            "other@example.com",
            "other-access",
            "other-refresh",
        )?,
    )?;
    write_account_index(&codex_home, &[("acct_selected", 0)])?;

    let _base_url_guard = EnvGuard::set("OPENAI_BASE_URL", format!("{}/v1", server.uri()));
    let auth_manager = AuthManager::shared(
        codex_home.path().to_path_buf(),
        false,
        AuthCredentialsStoreMode::File,
    );
    let model_provider = ModelProviderInfo {
        base_url: Some(format!("{}/v1", server.uri())),
        ..built_in_model_providers()["openai"].clone()
    };
    let mut config = load_default_config_for_test(&codex_home).await;
    config.model_catalog = None;
    config.model_provider = model_provider;
    let thread_manager = ThreadManager::new(
        &config,
        auth_manager,
        SessionSource::Exec,
        CollaborationModesConfig::default(),
    );
    let NewThread { thread: codex, .. } = thread_manager.start_thread(config).await?;

    let requests = models.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        wiremock_request_header(&requests[0], "authorization")?.as_deref(),
        Some("Bearer selected-access")
    );
    assert_eq!(
        wiremock_request_header(&requests[0], "chatgpt-account-id")?.as_deref(),
        Some("selected-account")
    );
    write_account_index(&codex_home, &[("acct_other", 0), ("acct_selected", 1)])?;
    let refresh_models =
        mount_models_once_with_etag(&server, ModelsResponse { models: Vec::new() }, "etag-2").await;
    let request = mount_response_once(
        &server,
        sse_response(sse(vec![
            ev_response_created("resp1"),
            ev_completed("resp1"),
        ]))
        .insert_header("X-Models-Etag", "etag-2"),
    )
    .await;
    submit_turn(&codex).await?;

    let refresh_requests = refresh_models.requests();
    assert_eq!(refresh_requests.len(), 1);
    assert_eq!(
        wiremock_request_header(&refresh_requests[0], "authorization")?.as_deref(),
        Some("Bearer selected-access")
    );
    assert_eq!(
        wiremock_request_header(&refresh_requests[0], "chatgpt-account-id")?.as_deref(),
        Some("selected-account")
    );
    let request = request.single_request();
    assert_eq!(
        request.header("authorization").as_deref(),
        Some("Bearer selected-access")
    );
    assert_eq!(
        request.header("chatgpt-account-id").as_deref(),
        Some("selected-account")
    );

    Ok(())
}

#[serial_test::serial(codex_api_key_env)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scoped_account_routing_preserves_codex_api_key_env_precedence() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let request = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp1"), ev_completed("resp1")]),
    )
    .await;
    let codex_home = TempDir::new()?;
    write_account_auth(
        &codex_home,
        "acct_selected",
        &auth_dot_json(
            "selected-account",
            "selected@example.com",
            "selected-access",
            "selected-refresh",
        )?,
    )?;
    write_account_index(&codex_home, &[("acct_selected", 0)])?;

    let _env_guard = EnvGuard::set(CODEX_API_KEY_ENV_VAR, "sk-env-scoped".to_string());
    let codex = start_thread_with_codex_api_key_env(&codex_home, &server, true).await?;
    submit_turn(&codex).await?;

    let request = request.single_request();
    assert_eq!(
        request.header("authorization").as_deref(),
        Some("Bearer sk-env-scoped")
    );
    assert_eq!(request.header("chatgpt-account-id"), None);
    assert_eq!(
        load_auth_dot_json(codex_home.path(), AuthCredentialsStoreMode::File)?,
        None
    );

    Ok(())
}

#[serial_test::serial(external_auth)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scoped_account_routing_preserves_external_chatgpt_auth_precedence() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let request = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp1"), ev_completed("resp1")]),
    )
    .await;
    let codex_home = TempDir::new()?;
    save_auth(
        codex_home.path(),
        &external_auth_dot_json(
            "external-account",
            "external@example.com",
            "external-access",
        )?,
        AuthCredentialsStoreMode::Ephemeral,
    )?;
    write_account_auth(
        &codex_home,
        "acct_selected",
        &auth_dot_json(
            "selected-account",
            "selected@example.com",
            "selected-access",
            "selected-refresh",
        )?,
    )?;
    write_account_index(&codex_home, &[("acct_selected", 0)])?;

    let codex = start_thread(&codex_home, &server).await?;
    submit_turn(&codex).await?;

    let request = request.single_request();
    assert_eq!(
        request.header("authorization").as_deref(),
        Some("Bearer external-access")
    );
    assert_eq!(
        request.header("chatgpt-account-id").as_deref(),
        Some("external-account")
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scoped_account_routing_requires_active_root_chatgpt_auth() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let request = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp1"), ev_completed("resp1")]),
    )
    .await;
    let codex_home = TempDir::new()?;
    write_account_auth(
        &codex_home,
        "acct_selected",
        &auth_dot_json(
            "selected-account",
            "selected@example.com",
            "selected-access",
            "selected-refresh",
        )?,
    )?;
    write_account_index(&codex_home, &[("acct_selected", 0)])?;

    let codex = start_thread(&codex_home, &server).await?;
    submit_turn(&codex).await?;

    let request = request.single_request();
    assert_eq!(request.header("authorization"), None);
    assert_eq!(request.header("chatgpt-account-id"), None);

    Ok(())
}

#[serial_test::serial(auth_refresh)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scoped_account_routing_respects_forced_workspace() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "blocked-refreshed-access",
            "refresh_token": "blocked-refreshed-refresh"
        })))
        .expect(0)
        .mount(&server)
        .await;
    let _env_guard = EnvGuard::set(
        REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR,
        format!("{}/oauth/token", server.uri()),
    );
    let request = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp1"), ev_completed("resp1")]),
    )
    .await;
    let codex_home = TempDir::new()?;
    save_auth(
        codex_home.path(),
        &auth_dot_json(
            "allowed-account",
            "allowed@example.com",
            "allowed-access",
            "allowed-refresh",
        )?,
        AuthCredentialsStoreMode::File,
    )?;
    let mut blocked_auth = auth_dot_json(
        "blocked-account",
        "blocked@example.com",
        "blocked-access",
        "blocked-refresh",
    )?;
    blocked_auth.last_refresh = Some(Utc::now() - Duration::days(9));
    write_account_auth(&codex_home, "acct_selected", &blocked_auth)?;
    write_account_index(&codex_home, &[("acct_selected", 0)])?;
    let auth_manager = AuthManager::shared(
        codex_home.path().to_path_buf(),
        false,
        AuthCredentialsStoreMode::File,
    );
    auth_manager.set_forced_chatgpt_workspace_id(Some("allowed-account".to_string()));

    let codex = start_thread_with_auth_manager(&codex_home, &server, auth_manager).await?;
    submit_turn(&codex).await?;

    let request = request.single_request();
    assert_eq!(request.header("authorization"), None);
    assert_eq!(request.header("chatgpt-account-id"), None);
    server.verify().await;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scoped_account_routing_does_not_survive_root_logout() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let response = sse(vec![ev_response_created("resp1"), ev_completed("resp1")]);
    let requests = mount_sse_sequence(&server, vec![response.clone(), response]).await;
    let codex_home = TempDir::new()?;
    save_auth(
        codex_home.path(),
        &auth_dot_json(
            "root-account",
            "root@example.com",
            "root-access",
            "root-refresh",
        )?,
        AuthCredentialsStoreMode::File,
    )?;
    write_account_auth(
        &codex_home,
        "acct_selected",
        &auth_dot_json(
            "selected-account",
            "selected@example.com",
            "selected-access",
            "selected-refresh",
        )?,
    )?;
    write_account_index(&codex_home, &[("acct_selected", 0)])?;
    let auth_manager = AuthManager::shared(
        codex_home.path().to_path_buf(),
        false,
        AuthCredentialsStoreMode::File,
    );

    let codex =
        start_thread_with_auth_manager(&codex_home, &server, Arc::clone(&auth_manager)).await?;
    submit_turn(&codex).await?;
    auth_manager.logout()?;
    submit_turn(&codex).await?;

    let requests = requests.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].header("authorization").as_deref(),
        Some("Bearer selected-access")
    );
    assert_eq!(
        requests[0].header("chatgpt-account-id").as_deref(),
        Some("selected-account")
    );
    assert_eq!(requests[1].header("authorization"), None);
    assert_eq!(requests[1].header("chatgpt-account-id"), None);

    Ok(())
}

#[serial_test::serial(auth_refresh)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scoped_account_refresh_ignores_root_auth_mismatch() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "new-selected-access",
            "refresh_token": "new-selected-refresh"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let _env_guard = EnvGuard::set(
        REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR,
        format!("{}/oauth/token", server.uri()),
    );
    let response = sse(vec![ev_response_created("resp1"), ev_completed("resp1")]);
    let request = mount_sse_once(&server, response).await;

    let codex_home = TempDir::new()?;
    let mut selected_auth = auth_dot_json(
        "selected-account",
        "selected@example.com",
        "old-selected-access",
        "old-selected-refresh",
    )?;
    selected_auth.last_refresh = Some(Utc::now() - Duration::days(9));
    write_account_auth(&codex_home, "acct_selected", &selected_auth)?;
    write_account_index(&codex_home, &[("acct_selected", 0)])?;

    let root_auth = auth_dot_json(
        "root-account",
        "root@example.com",
        "root-access",
        "root-refresh",
    )?;
    save_auth(
        codex_home.path(),
        &root_auth,
        AuthCredentialsStoreMode::File,
    )?;

    let codex = start_thread(&codex_home, &server).await?;
    submit_turn(&codex).await?;

    let request = request.single_request();
    assert_eq!(
        request.header("authorization").as_deref(),
        Some("Bearer new-selected-access")
    );
    assert_eq!(
        request.header("chatgpt-account-id").as_deref(),
        Some("selected-account")
    );
    let root_after = load_auth_dot_json(codex_home.path(), AuthCredentialsStoreMode::File)?
        .context("root auth should exist")?;
    assert_eq!(root_after, root_auth);

    let selected_after = load_auth_dot_json(
        &codex_home.path().join("accounts").join("acct_selected"),
        AuthCredentialsStoreMode::File,
    )?
    .context("selected account auth should exist")?;
    let selected_tokens = selected_after
        .tokens
        .context("selected tokens should exist")?;
    assert_eq!(selected_tokens.access_token, "new-selected-access");
    assert_eq!(selected_tokens.refresh_token, "new-selected-refresh");
    server.verify().await;

    Ok(())
}

async fn start_thread(
    codex_home: &TempDir,
    server: &MockServer,
) -> Result<Arc<codex_core::CodexThread>> {
    start_thread_with_codex_api_key_env(codex_home, server, false).await
}

async fn start_thread_with_codex_api_key_env(
    codex_home: &TempDir,
    server: &MockServer,
    enable_codex_api_key_env: bool,
) -> Result<Arc<codex_core::CodexThread>> {
    let auth_manager = AuthManager::shared(
        codex_home.path().to_path_buf(),
        enable_codex_api_key_env,
        AuthCredentialsStoreMode::File,
    );
    start_thread_with_auth_manager(codex_home, server, auth_manager).await
}

async fn start_thread_with_auth_manager(
    codex_home: &TempDir,
    server: &MockServer,
    auth_manager: Arc<AuthManager>,
) -> Result<Arc<codex_core::CodexThread>> {
    let model_provider = ModelProviderInfo {
        base_url: Some(format!("{}/v1", server.uri())),
        ..built_in_model_providers()["openai"].clone()
    };
    let mut config = load_default_config_for_test(codex_home).await;
    config.model_provider = model_provider;
    let thread_manager = ThreadManager::new(
        &config,
        auth_manager,
        SessionSource::Exec,
        CollaborationModesConfig::default(),
    );
    let NewThread { thread, .. } = thread_manager.start_thread(config).await?;
    Ok(thread)
}

async fn submit_turn(codex: &Arc<codex_core::CodexThread>) -> Result<()> {
    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello".to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
        })
        .await
        .context("submit turn")?;
    wait_for_event(codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;
    Ok(())
}

fn write_account_auth(codex_home: &TempDir, account_id: &str, auth: &AuthDotJson) -> Result<()> {
    write_account_auth_at_path(
        codex_home,
        &format!("accounts/{account_id}/auth.json"),
        auth,
    )
}

fn write_account_auth_at_path(
    codex_home: &TempDir,
    auth_path: &str,
    auth: &AuthDotJson,
) -> Result<()> {
    let auth_path = codex_home.path().join(auth_path);
    let account_home = auth_path
        .parent()
        .context("account auth path should have parent")?;
    save_auth(account_home, auth, AuthCredentialsStoreMode::File)?;
    Ok(())
}

fn write_account_index(codex_home: &TempDir, accounts: &[(&str, u32)]) -> Result<()> {
    let accounts = accounts
        .iter()
        .map(|(id, priority)| (*id, *priority, format!("accounts/{id}/auth.json")))
        .collect::<Vec<_>>();
    write_account_index_entries(codex_home, &accounts)
}

fn write_account_index_with_paths(
    codex_home: &TempDir,
    accounts: &[(&str, u32, &str)],
) -> Result<()> {
    let accounts = accounts
        .iter()
        .map(|(id, priority, auth_path)| (*id, *priority, (*auth_path).to_string()))
        .collect::<Vec<_>>();
    write_account_index_entries(codex_home, &accounts)
}

fn write_account_index_entries(
    codex_home: &TempDir,
    accounts: &[(&str, u32, String)],
) -> Result<()> {
    let accounts_dir = codex_home.path().join("accounts");
    std::fs::create_dir_all(&accounts_dir)?;
    let accounts = accounts
        .iter()
        .map(|(id, priority, auth_path)| {
            json!({
                "id": id,
                "label": id,
                "enabled": true,
                "priority": priority,
                "auth": {
                    "scope": "file",
                    "path": auth_path
                }
            })
        })
        .collect::<Vec<_>>();
    std::fs::write(
        accounts_dir.join("index.json"),
        serde_json::to_vec_pretty(&json!({ "accounts": accounts }))?,
    )?;
    Ok(())
}

fn auth_dot_json(
    account_id: &str,
    email: &str,
    access_token: &str,
    refresh_token: &str,
) -> Result<AuthDotJson> {
    Ok(AuthDotJson {
        auth_mode: Some(AuthMode::Chatgpt),
        openai_api_key: None,
        tokens: Some(TokenData {
            id_token: id_token(account_id, email)?,
            access_token: access_token.to_string(),
            refresh_token: refresh_token.to_string(),
            account_id: Some(account_id.to_string()),
        }),
        last_refresh: Some(Utc::now()),
    })
}

fn external_auth_dot_json(
    account_id: &str,
    email: &str,
    access_token: &str,
) -> Result<AuthDotJson> {
    Ok(AuthDotJson {
        auth_mode: Some(AuthMode::ChatgptAuthTokens),
        openai_api_key: None,
        tokens: Some(TokenData {
            id_token: id_token(account_id, email)?,
            access_token: access_token.to_string(),
            refresh_token: String::new(),
            account_id: Some(account_id.to_string()),
        }),
        last_refresh: Some(Utc::now()),
    })
}

fn id_token(account_id: &str, email: &str) -> Result<IdTokenInfo> {
    let mut token = IdTokenInfo::default();
    token.raw_jwt = minimal_jwt(account_id, email)?;
    token.email = Some(email.to_string());
    token.chatgpt_user_id = Some(format!("user-{account_id}"));
    token.chatgpt_account_id = Some(account_id.to_string());
    Ok(token)
}

fn minimal_jwt(account_id: &str, email: &str) -> Result<String> {
    #[derive(Serialize)]
    struct Header {
        alg: &'static str,
        typ: &'static str,
    }

    let header = Header {
        alg: "none",
        typ: "JWT",
    };
    let payload = json!({
        "email": email,
        "https://api.openai.com/auth": {
            "chatgpt_account_id": account_id,
            "user_id": format!("user-{account_id}")
        }
    });
    let encode = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let header_b64 = encode(&serde_json::to_vec(&header)?);
    let payload_b64 = encode(&serde_json::to_vec(&payload)?);
    let signature_b64 = encode(b"sig");
    Ok(format!("{header_b64}.{payload_b64}.{signature_b64}"))
}

fn request_headers(
    requests: &[core_test_support::responses::ResponsesRequest],
    name: &str,
) -> Result<Vec<String>> {
    requests
        .iter()
        .map(|request| request.header(name).context("request header"))
        .collect()
}

fn wiremock_request_header(request: &wiremock::Request, name: &str) -> Result<Option<String>> {
    request
        .headers
        .get(name)
        .map(|value| {
            value
                .to_str()
                .map(std::string::ToString::to_string)
                .context("request header should be valid UTF-8")
        })
        .transpose()
}

struct EnvGuard {
    key: &'static str,
    original: Option<OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: String) -> Self {
        let original = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.original {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}
