use super::*;

#[derive(Clone, Debug, PartialEq)]
pub enum AccountHandoffOutcome {
    NoHandoff,
    UnavailableForEphemeralStore,
    Completed(AccountProfile),
    PreservedNewerProfile(AccountProfile),
    RootRetained(AccountProfile),
}

impl AccountStore {
    pub fn export_selected_account_to_root_auth(
        &self,
        root_store_mode: AuthCredentialsStoreMode,
        root_keyring_backend_kind: AuthKeyringBackendKind,
    ) -> io::Result<AccountHandoffOutcome> {
        if root_store_mode == AuthCredentialsStoreMode::Ephemeral {
            return Ok(AccountHandoffOutcome::UnavailableForEphemeralStore);
        }
        let root_refresh_guard = AuthRefreshGuard::acquire(&self.codex_home)?;
        let Some(root_marker) = load_auth_dot_json_with_guard(
            &self.codex_home,
            root_store_mode,
            root_keyring_backend_kind,
            &root_refresh_guard,
        )?
        else {
            return Ok(AccountHandoffOutcome::NoHandoff);
        };
        if !is_root_account_marker(&root_marker) {
            return Ok(AccountHandoffOutcome::NoHandoff);
        }
        let Ok(selected_account_id) = account_id_for_auth(&root_marker) else {
            return Ok(AccountHandoffOutcome::NoHandoff);
        };

        let account_home = self.account_home(&selected_account_id);
        let account_refresh_guard = AuthRefreshGuard::acquire(&account_home)?;
        let _index_guard = self.acquire_index_lock()?;
        let Some(profile) = self.load_index()?.accounts.into_iter().find(|profile| {
            profile.id == selected_account_id && profile.enabled && !profile.login_required
        }) else {
            return Ok(AccountHandoffOutcome::NoHandoff);
        };
        let Some(auth) = load_auth_dot_json_with_guard(
            &account_home,
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
            &account_refresh_guard,
        )?
        else {
            return Ok(AccountHandoffOutcome::NoHandoff);
        };
        if full_chatgpt_account_id(&auth).as_ref() != Some(&selected_account_id) {
            return Ok(AccountHandoffOutcome::NoHandoff);
        }

        if root_store_mode == AuthCredentialsStoreMode::File {
            save_file_auth_if_unchanged(
                &self.codex_home,
                &root_marker,
                &auth,
                &root_refresh_guard,
            )?;
        } else {
            save_auth_with_guard(
                &self.codex_home,
                &auth,
                root_store_mode,
                root_keyring_backend_kind,
                &root_refresh_guard,
            )?;
        }
        Ok(AccountHandoffOutcome::Completed(profile))
    }

    pub fn reconcile_root_auth_to_matching_account(
        &self,
        root_store_mode: AuthCredentialsStoreMode,
        root_keyring_backend_kind: AuthKeyringBackendKind,
    ) -> io::Result<AccountHandoffOutcome> {
        if root_store_mode == AuthCredentialsStoreMode::Ephemeral {
            return Ok(AccountHandoffOutcome::UnavailableForEphemeralStore);
        }
        let root_refresh_guard = AuthRefreshGuard::acquire(&self.codex_home)?;
        let Some(root_auth) = load_auth_dot_json_with_guard(
            &self.codex_home,
            root_store_mode,
            root_keyring_backend_kind,
            &root_refresh_guard,
        )?
        else {
            return Ok(AccountHandoffOutcome::NoHandoff);
        };
        let Some(account_id) = full_chatgpt_account_id(&root_auth) else {
            return Ok(AccountHandoffOutcome::NoHandoff);
        };

        let account_home = self.account_home(&account_id);
        let account_refresh_guard = AuthRefreshGuard::acquire(&account_home)?;
        let _index_guard = self.acquire_index_lock()?;
        let mut index = self.load_index()?;
        let Some(profile_index) = index
            .accounts
            .iter()
            .position(|profile| profile.id == account_id && profile.enabled)
        else {
            return Ok(AccountHandoffOutcome::NoHandoff);
        };
        let previous_index = index.clone();
        let previous_account_auth = load_auth_dot_json_with_guard(
            &account_home,
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
            &account_refresh_guard,
        )?;
        let preserved_account_auth = previous_account_auth.as_ref().filter(|auth| {
            full_chatgpt_account_id(auth).as_ref() == Some(&account_id)
                && account_auth_is_newer(auth, &root_auth)
        });
        let account_auth_changed = preserved_account_auth.is_none();
        let reconciled_auth = preserved_account_auth.unwrap_or(&root_auth);

        if account_auth_changed {
            save_auth_with_guard(
                &account_home,
                reconciled_auth,
                AuthCredentialsStoreMode::File,
                AuthKeyringBackendKind::default(),
                &account_refresh_guard,
            )?;
        }
        let index_changed = index.accounts[profile_index].login_required;
        index.accounts[profile_index].login_required = false;
        let profile = index.accounts[profile_index].clone();
        if index_changed && let Err(err) = self.save_index(&index) {
            if account_auth_changed
                && let Err(rollback_err) = restore_file_auth(
                    &account_home,
                    previous_account_auth.as_ref(),
                    &account_refresh_guard,
                )
            {
                return Err(io::Error::other(format!(
                    "failed to update reconciled account index: {err}; failed to restore account auth: {rollback_err}"
                )));
            }
            return Err(err);
        }

        if root_store_mode != AuthCredentialsStoreMode::File {
            return Ok(AccountHandoffOutcome::RootRetained(profile));
        }

        let mut marker = reconciled_auth.clone();
        if let Some(tokens) = marker.tokens.as_mut() {
            tokens.refresh_token.clear();
        }
        if let Err(err) =
            save_file_auth_if_unchanged(&self.codex_home, &root_auth, &marker, &root_refresh_guard)
        {
            let mut rollback_errors = Vec::new();
            if index_changed && let Err(rollback_err) = self.save_index(&previous_index) {
                rollback_errors.push(format!("restore account index: {rollback_err}"));
            }
            if account_auth_changed
                && let Err(rollback_err) = restore_file_auth(
                    &account_home,
                    previous_account_auth.as_ref(),
                    &account_refresh_guard,
                )
            {
                rollback_errors.push(format!("restore account auth: {rollback_err}"));
            }
            if err.kind() != io::ErrorKind::WouldBlock {
                match save_file_auth_if_unchanged(
                    &self.codex_home,
                    &marker,
                    &root_auth,
                    &root_refresh_guard,
                ) {
                    Ok(()) => {}
                    Err(rollback_err) if rollback_err.kind() == io::ErrorKind::WouldBlock => {}
                    Err(rollback_err) => {
                        rollback_errors.push(format!("restore root auth: {rollback_err}"));
                    }
                }
            }
            return if rollback_errors.is_empty() {
                Err(err)
            } else {
                Err(io::Error::other(format!(
                    "failed to restore root account marker: {err}; rollback failed: {}",
                    rollback_errors.join("; ")
                )))
            };
        }

        if account_auth_changed {
            Ok(AccountHandoffOutcome::Completed(profile))
        } else {
            Ok(AccountHandoffOutcome::PreservedNewerProfile(profile))
        }
    }
}

fn account_auth_is_newer(account_auth: &AuthDotJson, root_auth: &AuthDotJson) -> bool {
    match (&account_auth.last_refresh, &root_auth.last_refresh) {
        (Some(account_refresh), Some(root_refresh)) => account_refresh > root_refresh,
        (Some(_), None) => true,
        (None, Some(_) | None) => false,
    }
}

fn full_chatgpt_account_id(auth: &AuthDotJson) -> Option<AccountId> {
    if !is_managed_chatgpt_auth(auth)
        || auth
            .tokens
            .as_ref()
            .is_none_or(|tokens| tokens.refresh_token.trim().is_empty())
    {
        return None;
    }
    account_id_for_auth(auth).ok()
}

#[cfg(test)]
#[path = "account_bridge_tests.rs"]
mod tests;
