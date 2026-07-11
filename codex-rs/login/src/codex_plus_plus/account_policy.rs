//! Per-account automation policy for Codex++.

use super::AccountId;
use super::AccountStore;

impl AccountStore {
    pub fn set_automation_enabled(
        &self,
        account_id: &AccountId,
        automation_enabled: bool,
    ) -> std::io::Result<bool> {
        let _index_guard = self.acquire_index_lock()?;
        let mut index = self.load_index()?;
        let Some(account) = index
            .accounts
            .iter_mut()
            .find(|account| &account.id == account_id)
        else {
            return Ok(false);
        };
        if account.automation_enabled != automation_enabled {
            account.automation_enabled = automation_enabled;
            self.save_index(&index)?;
        }
        Ok(true)
    }
}

#[cfg(test)]
#[path = "account_policy_tests.rs"]
mod tests;
