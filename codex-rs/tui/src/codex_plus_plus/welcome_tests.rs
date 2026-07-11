use ratatui::Terminal;

use super::*;
use crate::test_backend::VT100Backend;

#[test]
fn welcome_help_snapshot() {
    let mut terminal =
        Terminal::new(VT100Backend::new(/*width*/ 84, /*height*/ 2)).expect("terminal");
    terminal
        .draw(|frame| frame.render_widget(welcome_help_line(), frame.area()))
        .expect("render welcome help");

    insta::assert_snapshot!(
        "codex_plus_plus_welcome_help",
        terminal.backend().to_string()
    );
}
