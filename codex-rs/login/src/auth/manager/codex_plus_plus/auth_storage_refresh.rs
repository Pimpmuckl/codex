//! Refreshing one stored auth snapshot for Codex++ account management.

use std::fmt::Debug;
use std::path::Path;

use codex_config::types::AuthCredentialsStoreMode;
use thiserror::Error;

use super::super::AuthDotJson;
use super::super::AuthKeyringBackendKind;
use super::super::AuthManager;
use super::super::CodexAuth;
use crate::outbound_proxy::AuthRouteConfig;

#[derive(Error)]
#[error("{source}")]
pub struct RefreshAuthFromStorageError {
    #[source]
    source: std::io::Error,
    attempted_auth: Option<AuthDotJson>,
}

impl Debug for RefreshAuthFromStorageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RefreshAuthFromStorageError")
            .field("source", &self.source)
            .field("attempted_auth_available", &self.attempted_auth.is_some())
            .finish()
    }
}

impl RefreshAuthFromStorageError {
    pub fn attempted_auth(&self) -> Option<&AuthDotJson> {
        self.attempted_auth.as_ref()
    }
}

pub async fn refresh_auth_from_storage(
    codex_home: &Path,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
    forced_chatgpt_workspace_id: Option<&[String]>,
    chatgpt_base_url: Option<&str>,
    keyring_backend_kind: AuthKeyringBackendKind,
    auth_route_config: Option<&AuthRouteConfig>,
) -> Result<Option<CodexAuth>, RefreshAuthFromStorageError> {
    let manager = AuthManager::new(
        codex_home.to_path_buf(),
        /*enable_codex_api_key_env*/ false,
        auth_credentials_store_mode,
        forced_chatgpt_workspace_id.map(<[String]>::to_vec),
        chatgpt_base_url.map(str::to_string),
        keyring_backend_kind,
        auth_route_config.cloned(),
    )
    .await;
    if let Err(err) = manager.refresh_token().await {
        return Err(RefreshAuthFromStorageError {
            source: err.into(),
            attempted_auth: manager
                .auth_cached()
                .and_then(|auth| auth.get_current_auth_json()),
        });
    }
    Ok(manager.auth().await)
}
