//! Runtime activation, failover, and lease state for imported Codex++ accounts.

use std::collections::HashSet;
use std::path::PathBuf;

use chrono::Utc;
use codex_config::types::AuthCredentialsStoreMode;
use codex_config::types::AutomaticAccountSelection;

use super::super::AuthManager;
use super::super::CodexAuth;
use super::imported_account_startup::imported_account_blocked;
use super::imported_account_startup::load_imported_account_auth;
use crate::account::AccountCandidate;
use crate::account::AccountId;
use crate::account::AccountStore;
use crate::auth::storage::AuthKeyringBackendKind;

impl AuthManager {
    pub(in crate::auth::manager) fn active_auth_home(&self) -> PathBuf {
        self.active_auth_home
            .read()
            .map(|home| home.clone())
            .unwrap_or_else(|_| self.codex_home.clone())
    }

    pub(in crate::auth::manager) fn active_auth_credentials_store_mode(
        &self,
    ) -> AuthCredentialsStoreMode {
        if self.active_auth_home() != self.codex_home {
            AuthCredentialsStoreMode::File
        } else {
            self.auth_credentials_store_mode
        }
    }

    pub(in crate::auth::manager) fn active_keyring_backend_kind(&self) -> AuthKeyringBackendKind {
        if self.active_auth_home() != self.codex_home {
            AuthKeyringBackendKind::default()
        } else {
            self.keyring_backend_kind
        }
    }

    pub fn active_account_id(&self) -> Option<AccountId> {
        self.active_account_id
            .read()
            .ok()
            .and_then(|account_id| account_id.clone())
    }

    pub fn has_imported_accounts(&self) -> bool {
        AccountStore::new(self.codex_home.clone())
            .enabled_file_accounts()
            .is_ok_and(|accounts| !accounts.is_empty())
    }

    pub fn account_candidates(&self) -> std::io::Result<Vec<AccountCandidate>> {
        AccountStore::new(self.codex_home.clone()).candidates()
    }

    pub async fn activate_imported_account(&self, account_id: &AccountId) -> std::io::Result<()> {
        let _refresh_guard = self
            .refresh_lock
            .acquire()
            .await
            .map_err(|_| std::io::Error::other("auth refresh lock is closed"))?;
        if self.active_account_id().as_ref() == Some(account_id) {
            return Ok(());
        }
        let (account, account_home) = AccountStore::new(self.codex_home.clone())
            .file_account_profiles()?
            .into_iter()
            .find(|(account, _)| account.enabled && &account.id == account_id)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("imported account {account_id} is not enabled or does not exist"),
                )
            })?;

        if account.login_required {
            self.set_active_imported_account_source(account.id, self.codex_home.clone());
            self.set_cached_auth(/*new_auth*/ None);
            return Ok(());
        }
        let forced_chatgpt_workspace_id = self.forced_chatgpt_workspace_id();
        let auth = load_imported_account_auth(
            &account_home,
            forced_chatgpt_workspace_id.as_deref(),
            self.chatgpt_base_url.as_deref(),
            self.agent_identity_authapi_base_url.as_deref(),
            self.auth_route_config.as_ref(),
        )
        .await
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("imported account {account_id} does not contain usable ChatGPT auth"),
            )
        })?;

        self.set_active_imported_account(account.id, account_home, auth);
        Ok(())
    }

    pub fn record_imported_account_usage_limit_resets_at(
        &self,
        account_id: &AccountId,
        resets_at: i64,
    ) -> std::io::Result<bool> {
        AccountStore::new(self.codex_home.clone())
            .record_usage_limit_resets_at(account_id, resets_at)
    }

    pub async fn switch_to_next_imported_account(
        &self,
        attempted_account_ids: &HashSet<String>,
    ) -> bool {
        if self.automatic_account_selection == AutomaticAccountSelection::Disabled {
            return false;
        }
        let Ok(_refresh_guard) = self.refresh_lock.acquire().await else {
            return false;
        };
        self.switch_to_next_imported_account_unlocked(attempted_account_ids)
            .await
    }

    pub(super) async fn switch_to_next_imported_account_unlocked(
        &self,
        attempted_account_ids: &HashSet<String>,
    ) -> bool {
        let store = AccountStore::new(self.codex_home.clone());
        let accounts = store.enabled_file_account_profiles().unwrap_or_default();
        let active_account_id = self.active_account_id();
        if accounts.is_empty() {
            return false;
        }

        let current_id = active_account_id.map(|account_id| account_id.to_string());
        let start = current_id
            .as_deref()
            .and_then(|current| {
                accounts
                    .iter()
                    .position(|(account, _)| account.id.as_str() == current)
            })
            .map_or(0, |index| index + 1);

        let now = Utc::now().timestamp();
        for blocked in [false, true] {
            for in_use in [false, true] {
                for offset in 0..accounts.len() {
                    let (account, account_home) = &accounts[(start + offset) % accounts.len()];
                    if current_id.as_deref() == Some(account.id.as_str())
                        || attempted_account_ids.contains(account.id.as_str())
                        || imported_account_blocked(account, now) != blocked
                        || store.account_in_use(&account.id).unwrap_or(false) != in_use
                    {
                        continue;
                    }
                    let forced_chatgpt_workspace_id = self.forced_chatgpt_workspace_id();
                    let Some(auth) = load_imported_account_auth(
                        account_home,
                        forced_chatgpt_workspace_id.as_deref(),
                        self.chatgpt_base_url.as_deref(),
                        self.agent_identity_authapi_base_url.as_deref(),
                        self.auth_route_config.as_ref(),
                    )
                    .await
                    else {
                        continue;
                    };
                    self.set_active_imported_account(
                        account.id.clone(),
                        account_home.clone(),
                        auth,
                    );
                    return true;
                }
            }
        }

        false
    }

    fn set_active_imported_account(
        &self,
        account_id: AccountId,
        account_home: PathBuf,
        auth: CodexAuth,
    ) {
        self.set_active_imported_account_source(account_id, account_home);
        self.set_cached_auth(Some(auth));
    }

    pub(in crate::auth::manager) fn set_active_imported_account_source(
        &self,
        account_id: AccountId,
        account_home: PathBuf,
    ) {
        let account_lease = AccountStore::new(self.codex_home.clone())
            .try_acquire_lease(&account_id)
            .ok()
            .flatten();
        if let Ok(mut active_account_id) = self.active_account_id.write() {
            *active_account_id = Some(account_id);
        }
        if let Ok(mut active_account_lease) = self.active_account_lease.lock() {
            *active_account_lease = account_lease;
        }
        if let Ok(mut active_auth_home) = self.active_auth_home.write() {
            *active_auth_home = account_home;
        }
    }

    pub fn automatic_account_selection(&self) -> AutomaticAccountSelection {
        self.automatic_account_selection
    }

    pub(in crate::auth::manager) fn clear_active_imported_account(&self) {
        if let Ok(mut active_account_id) = self.active_account_id.write() {
            *active_account_id = None;
        }
        if let Ok(mut active_account_lease) = self.active_account_lease.lock() {
            *active_account_lease = None;
        }
        if let Ok(mut active_auth_home) = self.active_auth_home.write() {
            *active_auth_home = self.codex_home.clone();
        }
    }
}
