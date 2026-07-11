use std::time::Duration;

use codex_protocol::error::CodexErr;

pub(crate) const DELAY: Duration = Duration::from_secs(60);

pub(crate) fn applies_to(err: &CodexErr) -> bool {
    matches!(err, CodexErr::ServerOverloaded)
}

pub(crate) fn warning(retry_count: u64, max_retries: u64) -> String {
    format!(
        "The selected model is at capacity. Retrying in one minute ({retry_count}/{max_retries})."
    )
}
