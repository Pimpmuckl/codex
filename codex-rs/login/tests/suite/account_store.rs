use anyhow::Context;
use anyhow::Result;
use base64::Engine;
use chrono::Duration;
use chrono::Utc;
use codex_app_server_protocol::AuthMode;
use codex_core::auth::AuthCredentialsStoreMode;
use codex_core::auth::AuthDotJson;
use codex_core::auth::REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR;
use codex_core::auth::load_auth_dot_json;
use codex_core::auth::save_auth;
use codex_core::token_data::IdTokenInfo;
use codex_core::token_data::TokenData;
use codex_login::AccountStore;
use pretty_assertions::assert_eq;
use serde::Serialize;
use serde_json::json;
use std::ffi::OsString;
use tempfile::TempDir;
use tokio::sync::Mutex;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

static REFRESH_ENV_LOCK: Mutex<()> = Mutex::const_new(());

#[test]
fn import_current_writes_account_auth_and_dedupes_by_account_id() -> Result<()> {
    let codex_home = TempDir::new()?;
    let store = AccountStore::new(codex_home.path().to_path_buf());
    let auth = auth_dot_json("acct-1", "first@example.com", "access-1", "refresh-1")?;
    save_auth(codex_home.path(), &auth, AuthCredentialsStoreMode::File)?;

    let first = store.import_current("First", AuthCredentialsStoreMode::File)?;
    let second_auth = auth_dot_json("acct-1", "second@example.com", "access-2", "refresh-2")?;
    save_auth(
        codex_home.path(),
        &second_auth,
        AuthCredentialsStoreMode::File,
    )?;
    let second = store.import_current("Renamed", AuthCredentialsStoreMode::File)?;

    assert_eq!(first.id, second.id);
    let accounts = store.list()?;
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].label, "Renamed");
    assert_eq!(
        accounts[0].auth.path,
        format!("accounts/{}/auth.json", first.id)
    );

    let scoped_auth = load_auth_dot_json(
        &codex_home.path().join("accounts").join(first.id.as_str()),
        AuthCredentialsStoreMode::File,
    )?
    .context("account auth should exist")?;
    assert_eq!(scoped_auth, second_auth);

    Ok(())
}

#[tokio::test]
async fn account_auth_manager_refreshes_selected_storage_when_root_auth_differs() -> Result<()> {
    let _env_lock = REFRESH_ENV_LOCK.lock().await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "new-account-access",
            "refresh_token": "new-account-refresh"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let _env_guard = EnvGuard::set(
        REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR,
        format!("{}/oauth/token", server.uri()),
    );

    let codex_home = TempDir::new()?;
    let store = AccountStore::new(codex_home.path().to_path_buf());
    let selected_auth = stale_auth_dot_json("selected-account", "selected@example.com")?;
    save_auth(
        codex_home.path(),
        &selected_auth,
        AuthCredentialsStoreMode::File,
    )?;
    let selected = store.import_current("Selected", AuthCredentialsStoreMode::File)?;

    let root_auth = stale_auth_dot_json("root-account", "root@example.com")?;
    save_auth(
        codex_home.path(),
        &root_auth,
        AuthCredentialsStoreMode::File,
    )?;

    let auth_manager = store.auth_manager_for_account(&selected.id);
    auth_manager
        .refresh_token()
        .await
        .context("selected account refresh should succeed")?;

    let selected_home = codex_home
        .path()
        .join("accounts")
        .join(selected.id.as_str());
    let refreshed = load_auth_dot_json(&selected_home, AuthCredentialsStoreMode::File)?
        .context("selected account auth should exist")?;
    let refreshed_tokens = refreshed.tokens.context("selected tokens should exist")?;
    assert_eq!(refreshed_tokens.access_token, "new-account-access");
    assert_eq!(refreshed_tokens.refresh_token, "new-account-refresh");

    let root_after = load_auth_dot_json(codex_home.path(), AuthCredentialsStoreMode::File)?
        .context("root auth should still exist")?;
    assert_eq!(root_after, root_auth);
    server.verify().await;

    Ok(())
}

fn stale_auth_dot_json(account_id: &str, email: &str) -> Result<AuthDotJson> {
    let mut auth = auth_dot_json(account_id, email, "old-access", "old-refresh")?;
    auth.last_refresh = Some(Utc::now() - Duration::days(9));
    Ok(auth)
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

fn id_token(account_id: &str, email: &str) -> Result<IdTokenInfo> {
    let raw_jwt = minimal_jwt(account_id, email)?;
    let mut token = IdTokenInfo::default();
    token.raw_jwt = raw_jwt;
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

struct EnvGuard {
    key: &'static str,
    original: Option<OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: String) -> Self {
        let original = std::env::var_os(key);
        // SAFETY: REFRESH_ENV_LOCK serializes tests that mutate this process env var.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: REFRESH_ENV_LOCK serializes tests that mutate this process env var.
        unsafe {
            match &self.original {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}
