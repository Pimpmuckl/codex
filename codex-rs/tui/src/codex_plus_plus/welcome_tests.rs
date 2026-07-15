use ratatui::Terminal;
use ratatui::text::Text;

use super::*;
use crate::test_backend::VT100Backend;
use pretty_assertions::assert_eq;

#[test]
fn replaces_only_upstream_app_promo() {
    let app_promo = "Run `codex app` to open Codex Desktop";
    assert_eq!(WELCOME_TIP, replace_upstream_app_promo(app_promo));

    let unrelated_tip = "Use /fast for faster inference";
    assert_eq!(unrelated_tip, replace_upstream_app_promo(unrelated_tip));
}

#[test]
fn welcome_help_snapshot() {
    let mut terminal =
        Terminal::new(VT100Backend::new(/*width*/ 80, /*height*/ 3)).expect("terminal");
    terminal
        .draw(|frame| frame.render_widget(Text::from(welcome_help_lines()), frame.area()))
        .expect("render welcome help");

    insta::assert_snapshot!(
        "codex_plus_plus_welcome_help",
        terminal.backend().to_string()
    );
}
