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
            Some("work".to_string()),
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

#[test]
fn import_current_uses_email_label_when_label_is_omitted() {
    let codex_home = tempdir().expect("tempdir");
    save_auth(
        codex_home.path(),
        &test_auth("account-a", "user-a", "a@example.com"),
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )
    .expect("save root auth");

    let store = AccountStore::new(codex_home.path().to_path_buf());
    let profile = store
        .import_current(
            None,
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
        )
        .expect("import account");

    assert_eq!(profile.label, "a@example.com");
}

#[test]
fn import_current_reenables_existing_account_profile() {
    let codex_home = tempdir().expect("tempdir");
    let store = AccountStore::new(codex_home.path().to_path_buf());
    let first = import_test_account(&store, codex_home.path(), "first", "account-a");
    assert!(store.disable_all().expect("disable accounts"));
    save_root_test_auth(codex_home.path(), "account-a");

    let profile = store
        .import_current(
            Some("first again".to_string()),
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
        )
        .expect("re-import account");

    assert_eq!(
        store.candidates().expect("candidates"),
        vec![AccountCandidate {
            id: profile.id.clone(),
            display_label: "first again".to_string(),
            priority: first.priority,
            enabled: true,
            usage_limit_resets_at: None,
            blocked: false,
        }]
    );
    assert!(
        load_auth_dot_json(
            &store.account_home(&profile.id),
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
        )
        .expect("load reimported auth")
        .is_some()
    );
}

#[test]
fn candidates_include_usage_limit_blocked_state() {
    let codex_home = tempdir().expect("tempdir");
    let store = AccountStore::new(codex_home.path().to_path_buf());
    let first = import_test_account(&store, codex_home.path(), "first", "account-a");
    let second = import_test_account(&store, codex_home.path(), "second", "account-b");

    assert!(
        store
            .record_usage_limit_resets_at(&first.id, /*resets_at*/ 2_000)
            .expect("record reset")
    );

    assert_eq!(
        store.candidates_at(/*now*/ 1_000).expect("candidates"),
        vec![
            AccountCandidate {
                id: first.id,
                display_label: "first".to_string(),
                priority: first.priority,
                enabled: true,
                usage_limit_resets_at: Some(2_000),
                blocked: true,
            },
            AccountCandidate {
                id: second.id,
                display_label: "second".to_string(),
                priority: second.priority,
                enabled: true,
                usage_limit_resets_at: None,
                blocked: false,
            },
        ]
    );
}

#[test]
fn account_lease_marks_account_in_use_until_released() {
    let codex_home = tempdir().expect("tempdir");
    let store = AccountStore::new(codex_home.path().to_path_buf());
    let account = import_test_account(&store, codex_home.path(), "first", "account-a");

    let lease = store
        .try_acquire_lease(&account.id)
        .expect("acquire lease")
        .expect("account should be free");
    assert!(store.account_in_use(&account.id).expect("inspect lease"));

    drop(lease);
    assert!(!store.account_in_use(&account.id).expect("inspect lease"));
}

#[test]
fn apply_imported_account_to_root_auth_switches_root_auth() {
    let codex_home = tempdir().expect("tempdir");
    let store = AccountStore::new(codex_home.path().to_path_buf());
    let first = import_test_account(&store, codex_home.path(), "first", "account-a");
    let _second = import_test_account(&store, codex_home.path(), "second", "account-b");

    let selected = store
        .apply_imported_account_to_root_auth(
            &first.id,
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
        )
        .expect("select imported account");

    let root_auth = load_auth_dot_json(
        codex_home.path(),
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )
    .expect("load root auth")
    .expect("root auth");
    assert_eq!(selected, first);
    assert_eq!(
        root_auth.tokens.and_then(|tokens| tokens.account_id),
        Some("account-a".to_string())
    );
}

#[tokio::test]
async fn startup_prefers_imported_account_matching_root_chatgpt_auth() {
    let codex_home = tempdir().expect("tempdir");
    let store = AccountStore::new(codex_home.path().to_path_buf());
    let _first = import_test_account(&store, codex_home.path(), "first", "account-a");
    let second = import_test_account(&store, codex_home.path(), "second", "account-b");

    let manager = test_auth_manager(codex_home.path()).await;

    assert_eq!(manager.active_account_id(), Some(second.id));
}

#[tokio::test]
async fn startup_avoids_account_leased_by_another_manager() {
    let codex_home = tempdir().expect("tempdir");
    let store = AccountStore::new(codex_home.path().to_path_buf());
    let first = import_test_account(&store, codex_home.path(), "first", "account-a");
    let second = import_test_account(&store, codex_home.path(), "second", "account-b");

    let owner = test_auth_manager(codex_home.path()).await;
    assert_eq!(owner.active_account_id(), Some(second.id));

    let other = test_auth_manager(codex_home.path()).await;
    assert_eq!(other.active_account_id(), Some(first.id));
}

#[tokio::test]
async fn startup_prefers_unblocked_account_over_blocked_root_match() {
    let codex_home = tempdir().expect("tempdir");
    let store = AccountStore::new(codex_home.path().to_path_buf());
    let first = import_test_account(&store, codex_home.path(), "first", "account-a");
    let second = import_test_account(&store, codex_home.path(), "second", "account-b");
    store
        .record_usage_limit_resets_at(&second.id, Utc::now().timestamp() + 60)
        .expect("record reset");

    let manager = test_auth_manager(codex_home.path()).await;

    assert_eq!(manager.active_account_id(), Some(first.id));
}

#[tokio::test]
async fn startup_skips_imported_accounts_outside_forced_workspace() {
    let codex_home = tempdir().expect("tempdir");
    let store = AccountStore::new(codex_home.path().to_path_buf());
    let _first = import_test_account(&store, codex_home.path(), "first", "account-a");
    let second = import_test_account(&store, codex_home.path(), "second", "account-b");
    save_root_test_auth(codex_home.path(), "account-a");

    let manager =
        test_auth_manager_with_forced_workspace(codex_home.path(), vec!["account-b"]).await;

    assert_eq!(manager.active_account_id(), Some(second.id));
}

#[tokio::test]
async fn startup_keeps_root_api_key_auth_over_imported_accounts() {
    let codex_home = tempdir().expect("tempdir");
    let store = AccountStore::new(codex_home.path().to_path_buf());
    import_test_account(&store, codex_home.path(), "first", "account-a");
    save_auth(
        codex_home.path(),
        &api_key_auth(),
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )
    .expect("save api key auth");

    let manager = test_auth_manager(codex_home.path()).await;

    assert_eq!(manager.active_account_id(), None);
    assert_eq!(manager.auth_mode(), Some(AuthMode::ApiKey));
}

#[tokio::test]
async fn switch_to_next_imported_account_skips_attempted_local_account_ids() {
    let codex_home = tempdir().expect("tempdir");
    let store = AccountStore::new(codex_home.path().to_path_buf());
    let first = import_test_account(&store, codex_home.path(), "first", "account-a");
    let second = import_test_account(&store, codex_home.path(), "second", "account-b");
    save_root_test_auth(codex_home.path(), "account-a");

    let manager = test_auth_manager(codex_home.path()).await;
    assert_eq!(manager.active_account_id(), Some(first.id.clone()));

    let attempted = HashSet::from([first.id.to_string()]);
    assert!(manager.switch_to_next_imported_account(&attempted).await);
    assert_eq!(manager.active_account_id(), Some(second.id.clone()));

    let attempted = HashSet::from([first.id.to_string(), second.id.to_string()]);
    assert!(!manager.switch_to_next_imported_account(&attempted).await);
    assert_eq!(manager.active_account_id(), Some(second.id));
}

#[tokio::test]
async fn switch_to_next_imported_account_prefers_unblocked_accounts() {
    let codex_home = tempdir().expect("tempdir");
    let store = AccountStore::new(codex_home.path().to_path_buf());
    let first = import_test_account(&store, codex_home.path(), "first", "account-a");
    let second = import_test_account(&store, codex_home.path(), "second", "account-b");
    let third = import_test_account(&store, codex_home.path(), "third", "account-c");
    save_root_test_auth(codex_home.path(), "account-a");
    store
        .record_usage_limit_resets_at(&second.id, Utc::now().timestamp() + 60)
        .expect("record reset");

    let manager = test_auth_manager(codex_home.path()).await;

    let attempted = HashSet::from([first.id.to_string()]);
    assert!(manager.switch_to_next_imported_account(&attempted).await);
    assert_eq!(manager.active_account_id(), Some(third.id));
}

#[tokio::test]
async fn activate_imported_account_respects_requested_blocked_in_use_account() {
    let codex_home = tempdir().expect("tempdir");
    let store = AccountStore::new(codex_home.path().to_path_buf());
    let first = import_test_account(&store, codex_home.path(), "first", "account-a");
    let second = import_test_account(&store, codex_home.path(), "second", "account-b");
    store
        .record_usage_limit_resets_at(&second.id, Utc::now().timestamp() + 60)
        .expect("record reset");
    let _lease = store
        .try_acquire_lease(&second.id)
        .expect("acquire lease")
        .expect("selected account should initially be free");
    save_root_test_auth(codex_home.path(), "account-a");
    let manager = test_auth_manager(codex_home.path()).await;
    assert_eq!(manager.active_account_id(), Some(first.id));

    manager
        .activate_imported_account(&second.id)
        .await
        .expect("activate account");

    assert_eq!(manager.active_account_id(), Some(second.id));
    assert_eq!(
        manager.auth_cached().and_then(|auth| auth.get_account_id()),
        Some("account-b".to_string())
    );
}

#[tokio::test]
async fn reactivating_current_imported_account_preserves_its_lease() {
    let codex_home = tempdir().expect("tempdir");
    let store = AccountStore::new(codex_home.path().to_path_buf());
    let account = import_test_account(&store, codex_home.path(), "first", "account-a");
    save_root_test_auth(codex_home.path(), "account-a");
    let manager = test_auth_manager(codex_home.path()).await;
    assert_eq!(manager.active_account_id(), Some(account.id.clone()));
    assert!(store.account_in_use(&account.id).expect("check lease"));

    manager
        .activate_imported_account(&account.id)
        .await
        .expect("reactivate account");

    assert_eq!(manager.active_account_id(), Some(account.id.clone()));
    assert!(store.account_in_use(&account.id).expect("check lease"));
}

#[tokio::test]
async fn logout_clears_imported_accounts_that_startup_would_select() {
    let codex_home = tempdir().expect("tempdir");
    let store = AccountStore::new(codex_home.path().to_path_buf());
    let first = import_test_account(&store, codex_home.path(), "first", "account-a");
    let second = import_test_account(&store, codex_home.path(), "second", "account-b");
    save_root_test_auth(codex_home.path(), "account-a");
    let manager = test_auth_manager(codex_home.path()).await;
    assert_eq!(manager.active_account_id(), Some(first.id.clone()));

    assert!(manager.logout().await.expect("logout"));

    assert_eq!(manager.active_account_id(), None);
    assert!(manager.auth_cached().is_none());
    assert_eq!(
        load_auth_dot_json(
            codex_home.path(),
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
        )
        .expect("load root auth"),
        None
    );
    assert_eq!(
        load_auth_dot_json(
            &store.account_home(&first.id),
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
        )
        .expect("load first account auth"),
        None
    );
    assert_eq!(
        load_auth_dot_json(
            &store.account_home(&second.id),
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
        )
        .expect("load second account auth"),
        None
    );
    assert_eq!(
        store.candidates().expect("candidates"),
        vec![
            AccountCandidate {
                id: first.id.clone(),
                display_label: "first".to_string(),
                priority: first.priority,
                enabled: false,
                usage_limit_resets_at: None,
                blocked: false,
            },
            AccountCandidate {
                id: second.id.clone(),
                display_label: "second".to_string(),
                priority: second.priority,
                enabled: false,
                usage_limit_resets_at: None,
                blocked: false,
            },
        ]
    );
    let restarted = test_auth_manager(codex_home.path()).await;
    assert_eq!(restarted.active_account_id(), None);
    assert_eq!(restarted.auth_mode(), None);
}

async fn test_auth_manager(codex_home: &std::path::Path) -> std::sync::Arc<AuthManager> {
    test_auth_manager_with_forced_workspace(codex_home, Vec::new()).await
}

async fn test_auth_manager_with_forced_workspace(
    codex_home: &std::path::Path,
    forced_workspace_ids: Vec<&str>,
) -> std::sync::Arc<AuthManager> {
    let forced_chatgpt_workspace_id = if forced_workspace_ids.is_empty() {
        None
    } else {
        Some(
            forced_workspace_ids
                .into_iter()
                .map(str::to_string)
                .collect(),
        )
    };
    AuthManager::shared(
        codex_home.to_path_buf(),
        /*enable_codex_api_key_env*/ false,
        AuthCredentialsStoreMode::File,
        forced_chatgpt_workspace_id,
        /*chatgpt_base_url*/ None,
        AuthKeyringBackendKind::default(),
        /*auth_route_config*/ None,
    )
    .await
}

fn import_test_account(
    store: &AccountStore,
    codex_home: &std::path::Path,
    label: &str,
    account_id: &str,
) -> AccountProfile {
    save_root_test_auth(codex_home, account_id);
    store
        .import_current(
            Some(label.to_string()),
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
        )
        .expect("import account")
}

fn save_root_test_auth(codex_home: &std::path::Path, account_id: &str) {
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

fn api_key_auth() -> AuthDotJson {
    AuthDotJson {
        auth_mode: Some(AuthMode::ApiKey),
        openai_api_key: Some("test-api-key".to_string()),
        tokens: None,
        last_refresh: None,
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
