//! Cloud configuration loading for an explicitly selected imported account.

use crate::bundle_loader::cloud_config_bundle_loader;
use codex_config::CloudConfigBundleLoader;
use codex_login::AccountId;
use codex_login::AuthManager;
use codex_login::AuthManagerConfig;
use std::sync::Arc;

/// Builds a cloud configuration loader using the explicitly selected imported account.
pub async fn cloud_config_bundle_loader_for_selected_account(
    config: &impl AuthManagerConfig,
    selected_account_id: Option<&AccountId>,
) -> std::io::Result<CloudConfigBundleLoader> {
    let auth_route_config = config.auth_route_config();
    let http_client_factory = auth_route_config.http_client_factory().clone();
    let auth_manager = Arc::new(
        AuthManager::new_with_automatic_account_selection(
            config.codex_home(),
            /*enable_codex_api_key_env*/ false,
            config.cli_auth_credentials_store_mode(),
            /*forced_chatgpt_workspace_id*/ None,
            Some(config.chatgpt_base_url()),
            config.auth_keyring_backend_kind(),
            auth_route_config,
            config.automatic_account_selection(),
        )
        .await,
    );
    if let Some(selected_account_id) = selected_account_id {
        auth_manager
            .activate_imported_account(selected_account_id)
            .await?;
    }
    Ok(cloud_config_bundle_loader(
        auth_manager,
        config.chatgpt_base_url(),
        config.codex_home(),
        http_client_factory,
    ))
}
