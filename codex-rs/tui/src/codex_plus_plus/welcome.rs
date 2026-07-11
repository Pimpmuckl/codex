//! Codex++ onboarding guidance.

use ratatui::style::Stylize;
use ratatui::text::Line;

pub(crate) fn welcome_help_line() -> Line<'static> {
    "  Use /codexplusplus for fork settings and /accounts to manage accounts."
        .dim()
        .into()
}

#[cfg(test)]
#[path = "welcome_tests.rs"]
mod tests;
