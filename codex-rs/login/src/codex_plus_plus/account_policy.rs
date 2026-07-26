//! Per-account automation policy for Codex++.

use super::AccountId;
use super::AccountStore;
use crate::account_lease::AccountLease;
use std::time::Duration;
use std::time::Instant;

const RESET_DRAIN_TIMEOUT: Duration = Duration::from_secs(90);
const RESET_DRAIN_POLL_INTERVAL: Duration = Duration::from_millis(50);

impl AccountStore {
    pub(crate) fn acquire_account_topology_lease(&self) -> std::io::Result<AccountLease> {
        AccountLease::acquire(&self.codex_home.join("account-topology.lock"))
    }

    pub fn set_automation_enabled(
        &self,
        account_id: &AccountId,
        automation_enabled: bool,
    ) -> std::io::Result<bool> {
        self.set_automation_enabled_batch([(account_id, automation_enabled)])
    }

    pub fn set_automation_enabled_batch<'a>(
        &self,
        updates: impl IntoIterator<Item = (&'a AccountId, bool)>,
    ) -> std::io::Result<bool> {
        let _index_guard = self.acquire_index_lock()?;
        let mut index = self.load_index()?;
        let mut changed = false;
        for (account_id, automation_enabled) in updates {
            let Some(account) = index
                .accounts
                .iter_mut()
                .find(|account| &account.id == account_id)
            else {
                return Ok(false);
            };
            if account.automation_enabled != automation_enabled {
                account.automation_enabled = automation_enabled;
                changed = true;
            }
        }
        if changed {
            self.save_index(&index)?;
        }
        Ok(true)
    }

    pub(crate) fn wait_for_reset_mutation_home_idle(
        &self,
        account_home: &std::path::Path,
    ) -> std::io::Result<crate::ResetMutationLease> {
        let deadline = Instant::now() + RESET_DRAIN_TIMEOUT;
        loop {
            if let Some(lease) = self.try_acquire_reset_mutation_lease_for_home(account_home)? {
                return Ok(lease);
            }
            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "timed out waiting for active reset mutation",
                ));
            }
            std::thread::sleep(RESET_DRAIN_POLL_INTERVAL);
        }
    }
}

#[cfg(test)]
#[path = "account_policy_tests.rs"]
mod tests;
