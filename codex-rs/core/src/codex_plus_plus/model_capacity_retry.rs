use std::time::Duration;

use codex_protocol::error::CodexErr;
use codex_protocol::protocol::SessionSource;

pub(crate) const DELAY: Duration = Duration::from_secs(60);

pub(crate) fn applies_to_sampling(err: &CodexErr, session_source: &SessionSource) -> bool {
    matches!(err, CodexErr::ServerOverloaded)
        && !crate::guardian::is_guardian_reviewer_source(session_source)
}

pub(crate) fn warning(retry_count: u64, max_retries: u64) -> String {
    format!(
        "The selected model is at capacity. Retrying in one minute ({retry_count}/{max_retries})."
    )
}
