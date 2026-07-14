//! TUI presentation helpers for Codex++ model-capacity retries.

const WARNING_PREFIX: &str = "The selected model is at capacity. ";

pub(crate) fn status_details(message: &str) -> Option<&str> {
    message
        .strip_prefix(WARNING_PREFIX)
        .filter(|details| details.starts_with("Retrying in "))
}
