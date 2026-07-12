//! File-only auth loading for isolated Codex++ account work.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use codex_config::types::AuthCredentialsStoreMode;
use codex_config::types::AutomaticAccountSelection;

use super::super::AuthKeyringBackendKind;
use super::super::AuthManager;
use super::super::CodexAuth;
use super::super::agent_identity_authapi_base_url;
use super::super::chatgpt_auth_workspace_allowed;
use super::super::load_auth_from_storage;
use super::super::load_auth_from_storage_with_guard;
use crate::account_lease::AuthRefreshGuard;
use crate::outbound_proxy::AuthRouteConfig;

pub(in crate::auth::manager) async fn new_manager(
    codex_home: PathBuf,
    forced_chatgpt_workspace_id: Option<Vec<String>>,
    chatgpt_base_url: Option<String>,
    auth_route_config: Option<AuthRouteConfig>,
) -> std::io::Result<Option<AuthManager>> {
    let agent_identity_authapi_base_url =
        agent_identity_authapi_base_url(chatgpt_base_url.as_deref()).ok();
    let Some(auth) = load_auth_from_storage(
        &codex_home,
        AuthCredentialsStoreMode::File,
        forced_chatgpt_workspace_id.as_deref(),
        chatgpt_base_url.as_deref(),
        AuthKeyringBackendKind::default(),
        agent_identity_authapi_base_url.as_deref(),
        auth_route_config.as_ref(),
    )
    .await?
    .filter(|auth| {
        !auth.is_chatgpt_auth()
            || chatgpt_auth_workspace_allowed(auth, forced_chatgpt_workspace_id.as_deref())
    }) else {
        return Ok(None);
    };
    let mut manager = Arc::into_inner(AuthManager::from_auth_with_home(auth, codex_home))
        .ok_or_else(|| std::io::Error::other("file auth manager is unexpectedly shared"))?;
    manager.auth_storage_only = true;
    manager.automatic_account_selection = AutomaticAccountSelection::Disabled;
    manager.forced_chatgpt_workspace_id = RwLock::new(forced_chatgpt_workspace_id);
    manager.chatgpt_base_url = chatgpt_base_url;
    manager.agent_identity_authapi_base_url = agent_identity_authapi_base_url;
    manager.auth_route_config = auth_route_config;
    Ok(Some(manager))
}

pub(in crate::auth::manager) async fn load(
    manager: &AuthManager,
    guard: Option<&AuthRefreshGuard>,
) -> Option<CodexAuth> {
    let forced_chatgpt_workspace_id = manager.forced_chatgpt_workspace_id();
    load_auth_from_storage_with_guard(
        &manager.active_auth_home(),
        manager.active_auth_credentials_store_mode(),
        forced_chatgpt_workspace_id.as_deref(),
        manager.chatgpt_base_url.as_deref(),
        manager.active_keyring_backend_kind(),
        manager.agent_identity_authapi_base_url.as_deref(),
        manager.auth_route_config.as_ref(),
        guard,
    )
    .await
    .ok()
    .flatten()
    .filter(|auth| {
        !auth.is_chatgpt_auth()
            || chatgpt_auth_workspace_allowed(auth, forced_chatgpt_workspace_id.as_deref())
    })
}
