//! Codex++ onboarding guidance.

use ratatui::style::Stylize;
use ratatui::text::Line;

pub(crate) const WELCOME_TIP: &str = "Welcome to **Codex++**. Use **/codexplusplus** for settings and **/accounts** to enable or disable accounts.";

pub(crate) fn replace_upstream_app_promo(tip: &'static str) -> &'static str {
    if tip.contains("codex app") {
        WELCOME_TIP
    } else {
        tip
    }
}

pub(crate) fn welcome_help_lines() -> Vec<Line<'static>> {
    let tip = WELCOME_TIP.replace("**", "");
    textwrap::wrap(
        &tip,
        textwrap::Options::new(/*width*/ 78)
            .initial_indent("  ")
            .subsequent_indent("  "),
    )
    .into_iter()
    .map(|line| line.into_owned().dim().into())
    .collect()
}

#[cfg(test)]
#[path = "welcome_tests.rs"]
mod tests;
