//! Session-local account identity freshness after imported-account failover.

use codex_app_server_protocol::CodexErrorInfo;
use codex_app_server_protocol::ErrorNotification;

pub(crate) const MAY_BE_STALE_NOTE: &str =
    "Account identity may be stale after automatic failover.";

#[derive(Default)]
pub(crate) struct AccountIdentityFreshness {
    may_be_stale: bool,
}

impl AccountIdentityFreshness {
    pub(crate) fn observe_live_error(&mut self, notification: &ErrorNotification) {
        if notification.will_retry
            && matches!(
                notification.error.codex_error_info.as_ref(),
                Some(CodexErrorInfo::UsageLimitExceeded)
            )
        {
            self.may_be_stale = true;
        }
    }

    pub(crate) fn may_be_stale(&self) -> bool {
        self.may_be_stale
    }
}
