use super::*;
use crate::AuthManager;
use crate::token_data::IdTokenInfo;
use crate::token_data::TokenData;
use base64::Engine;
use chrono::Utc;
use codex_protocol::auth::AuthMode;
use pretty_assertions::assert_eq;
use serde::Serialize;
use std::collections::HashSet;
use tempfile::tempdir;

#[test]
fn import_current_copies_chatgpt_auth_into_account_home() {
    let codex_home = tempdir().expect("tempdir");
    let root_auth = test_auth("account-a", "user-a", "a@example.com");
    save_auth(
        codex_home.path(),
        &root_auth,
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )
    .expect("save root auth");

    let store = AccountStore::new(codex_home.path().to_path_buf());
    let profile = store
        .import_current(
            "work",
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
        )
        .expect("import account");

    assert_eq!(store.list().expect("list accounts"), vec![profile.clone()]);
    let imported_auth = load_auth_dot_json(
        &store.account_home(&profile.id),
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )
    .expect("load account auth");
    assert_eq!(imported_auth, Some(root_auth));
}

#[tokio::test]
async fn switch_to_next_imported_account_skips_attempted_local_account_ids() {
    let codex_home = tempdir().expect("tempdir");
    let store = AccountStore::new(codex_home.path().to_path_buf());
    let first = import_test_account(&store, codex_home.path(), "first", "account-a");
    let second = import_test_account(&store, codex_home.path(), "second", "account-b");

    let manager = AuthManager::shared(
        codex_home.path().to_path_buf(),
        /*enable_codex_api_key_env*/ false,
        AuthCredentialsStoreMode::File,
        /*forced_chatgpt_workspace_id*/ None,
        /*chatgpt_base_url*/ None,
        AuthKeyringBackendKind::default(),
        /*auth_route_config*/ None,
    )
    .await;
    assert_eq!(manager.active_account_id(), Some(first.id.clone()));

    let attempted = HashSet::from([first.id.to_string()]);
    assert!(manager.switch_to_next_imported_account(&attempted).await);
    assert_eq!(manager.active_account_id(), Some(second.id.clone()));

    let attempted = HashSet::from([first.id.to_string(), second.id.to_string()]);
    assert!(!manager.switch_to_next_imported_account(&attempted).await);
    assert_eq!(manager.active_account_id(), Some(second.id));
}

fn import_test_account(
    store: &AccountStore,
    codex_home: &std::path::Path,
    label: &str,
    account_id: &str,
) -> AccountProfile {
    save_auth(
        codex_home,
        &test_auth(
            account_id,
            &format!("user-{account_id}"),
            &format!("{account_id}@example.com"),
        ),
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )
    .expect("save root auth");
    store
        .import_current(
            label,
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
        )
        .expect("import account")
}

fn test_auth(account_id: &str, user_id: &str, email: &str) -> AuthDotJson {
    AuthDotJson {
        auth_mode: Some(AuthMode::Chatgpt),
        openai_api_key: None,
        tokens: Some(TokenData {
            id_token: IdTokenInfo {
                email: Some(email.to_string()),
                chatgpt_user_id: Some(user_id.to_string()),
                chatgpt_account_id: Some(account_id.to_string()),
                raw_jwt: fake_jwt(account_id, user_id, email),
                ..IdTokenInfo::default()
            },
            access_token: format!("access-{account_id}"),
            refresh_token: format!("refresh-{account_id}"),
            account_id: Some(account_id.to_string()),
        }),
        last_refresh: Some(Utc::now()),
        agent_identity: None,
        personal_access_token: None,
        bedrock_api_key: None,
    }
}

fn fake_jwt(account_id: &str, user_id: &str, email: &str) -> String {
    #[derive(Serialize)]
    struct Header {
        alg: &'static str,
        typ: &'static str,
    }

    let header = Header {
        alg: "none",
        typ: "JWT",
    };
    let payload = serde_json::json!({
        "email": email,
        "https://api.openai.com/auth": {
            "chatgpt_account_id": account_id,
            "chatgpt_user_id": user_id
        }
    });
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&header).expect("serialize header"));
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&payload).expect("serialize payload"));
    let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"sig");
    format!("{header}.{payload}.{signature}")
}
