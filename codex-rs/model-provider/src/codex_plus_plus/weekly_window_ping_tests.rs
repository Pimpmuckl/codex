use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use codex_login::AccountStore;
use codex_login::AuthCredentialsStoreMode;
use codex_login::AuthDotJson;
use codex_login::AuthKeyringBackendKind;
use codex_login::AuthManager;
use codex_login::save_auth;
use codex_model_provider_info::ModelProviderInfo;
use pretty_assertions::assert_eq;
use serde_json::json;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::Request;
use wiremock::ResponseTemplate;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;

use super::*;

const TEST_ID_TOKEN: &str = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJlbWFpbCI6InVzZXJAZXhhbXBsZS5jb20iLCJlbWFpbF92ZXJpZmllZCI6dHJ1ZSwiaHR0cHM6Ly9hcGkub3BlbmFpLmNvbS9hdXRoIjp7ImNoYXRncHRfdXNlcl9pZCI6InVzZXItMTIzNDUiLCJ1c2VyX2lkIjoidXNlci0xMjM0NSIsImNoYXRncHRfcGxhbl90eXBlIjoicHJvIiwiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjb3VudC0xMjMifX0.c2ln";
static NEXT_HOME: AtomicUsize = AtomicUsize::new(0);

struct TestAccount {
    root: PathBuf,
    account_id: AccountId,
    account_home: PathBuf,
}

impl TestAccount {
    fn new() -> Self {
        let id = NEXT_HOME.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "codex-model-provider-weekly-window-{}-{id}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create test home");
        write_auth(&root, "account-123", "one", "refresh-one");
        let profile = AccountStore::new(root.clone())
            .import_current(
                Some("test account".to_string()),
                AuthCredentialsStoreMode::File,
                AuthKeyringBackendKind::default(),
            )
            .expect("import account");
        let account_home = root.join("accounts").join(profile.id.as_str());
        Self {
            root,
            account_id: profile.id,
            account_home,
        }
    }

    fn request(&self, server: &MockServer) -> WeeklyWindowPingRequest {
        WeeklyWindowPingRequest {
            root_codex_home: self.root.clone(),
            account_id: self.account_id.clone(),
            account_codex_home: self.account_home.clone(),
            model: "gpt-test".to_string(),
            model_provider_id: OPENAI_PROVIDER_ID.to_string(),
            model_provider: ModelProviderInfo::create_openai_provider(/*base_url*/ None),
            chatgpt_base_url: server.uri(),
            auth_route_config: None,
            forced_chatgpt_workspace_id: None,
            http_client_factory: HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        }
    }
}

impl Drop for TestAccount {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn write_auth(home: &std::path::Path, account_id: &str, token: &str, refresh: &str) {
    let auth: AuthDotJson = serde_json::from_value(json!({
        "OPENAI_API_KEY": null,
        "tokens": {"id_token": TEST_ID_TOKEN, "access_token": token,
            "refresh_token": refresh, "account_id": account_id},
        "last_refresh": "2099-01-01T00:00:00Z"
    }))
    .expect("valid test auth");
    save_auth(
        home,
        &auth,
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )
    .expect("save test auth");
}

fn completed_response() -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_string(
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-1\"}}\n\n",
        )
}

#[tokio::test]
async fn sends_exact_request_without_mutating_auth_or_exposing_it_to_custom_providers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/codex/responses"))
        .respond_with(completed_response())
        .mount(&server)
        .await;
    let account = TestAccount::new();
    let foreground = AuthManager::new(
        account.root.clone(),
        /*enable_codex_api_key_env*/ false,
        AuthCredentialsStoreMode::File,
        /*forced_chatgpt_workspace_id*/ None,
        Some(server.uri()),
        AuthKeyringBackendKind::default(),
        /*auth_route_config*/ None,
    )
    .await;
    let foreground_id = foreground.active_account_id();
    let root_auth = std::fs::read(account.root.join("auth.json")).expect("root auth");

    assert_eq!(
        ping_weekly_window(account.request(&server)).await,
        WeeklyWindowPingOutcome::Completed
    );
    let requests = server.received_requests().await.expect("received requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&requests[0].body).expect("JSON body"),
        json!({"model": "gpt-test", "instructions": "", "input": [{"type": "message",
            "role": "user", "content": [{"type": "input_text", "text": "Reply OK."}]}],
            "store": false, "stream": true, "include": [], "max_output_tokens": 8})
    );
    assert_eq!(
        requests[0]
            .headers
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
        Some("Bearer one")
    );
    assert_eq!(
        std::fs::read(account.root.join("auth.json")).unwrap(),
        root_auth
    );
    assert_eq!(foreground.active_account_id(), foreground_id);

    let mut custom_request = account.request(&server);
    custom_request.model_provider_id = "custom".to_string();
    assert_eq!(
        ping_weekly_window(custom_request).await,
        WeeklyWindowPingOutcome::DefiniteRejection
    );
}

#[tokio::test]
async fn unauthorized_recovery_preserves_identity_and_login_required_state() {
    let server = MockServer::start().await;
    let account = TestAccount::new();
    let account_home = account.account_home.clone();
    Mock::given(method("POST"))
        .and(path("/codex/responses"))
        .and(header("authorization", "Bearer one"))
        .respond_with(move |_request: &Request| {
            write_auth(&account_home, "account-123", "two", "refresh-two");
            ResponseTemplate::new(401)
        })
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/codex/responses"))
        .and(header("authorization", "Bearer two"))
        .respond_with(completed_response())
        .mount(&server)
        .await;
    assert_eq!(
        ping_weekly_window(account.request(&server)).await,
        WeeklyWindowPingOutcome::Completed
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 2);

    server.reset().await;
    write_auth(&account.account_home, "account-123", "one", "refresh-one");
    let account_home = account.account_home.clone();
    Mock::given(method("POST"))
        .and(path("/codex/responses"))
        .respond_with(move |_request: &Request| {
            write_auth(&account_home, "other-account", "other", "other-refresh");
            ResponseTemplate::new(401)
        })
        .mount(&server)
        .await;
    assert_eq!(
        ping_weekly_window(account.request(&server)).await,
        WeeklyWindowPingOutcome::LoginRequired
    );
    assert!(!AccountStore::new(account.root.clone()).list().unwrap()[0].login_required);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn ambiguous_outcomes_are_not_replayed_and_the_attempt_is_bounded() {
    let server = MockServer::start().await;
    let account = TestAccount::new();
    Mock::given(method("POST"))
        .and(path("/codex/responses"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    assert_eq!(
        ping_weekly_window(account.request(&server)).await,
        WeeklyWindowPingOutcome::Ambiguous
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);

    for response in [
        ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_string("event: response.created\ndata: {\"type\":\"response.created\"}\n\n"),
        ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_string("event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"server_is_overloaded\"}}}\n\n"),
        completed_response().set_delay(Duration::from_secs(1)),
    ] {
        server.reset().await;
        Mock::given(method("POST"))
            .and(path("/codex/responses"))
            .respond_with(response)
            .mount(&server)
            .await;
        assert_eq!(
            ping_weekly_window_with_timeout(account.request(&server), Duration::from_millis(10))
                .await,
            WeeklyWindowPingOutcome::Ambiguous
        );
    }
}
