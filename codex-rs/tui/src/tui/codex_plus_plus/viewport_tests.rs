use crate::custom_terminal::Terminal;
use crate::test_backend::VT100Backend;
use crate::tui::Tui;
use crate::tui::scrollback::ScrollbackStrategy;
use pretty_assertions::assert_eq;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::layout::Size;
use ratatui::style::Style;
use std::io::Write;

#[test]
fn popup_cycles_restore_visible_history_without_accumulating_blank_rows() {
    let size = Size::new(/*width*/ 24, /*height*/ 12);
    let mut first_closed = None;
    for strategy in [
        ScrollbackStrategy::Standard,
        ScrollbackStrategy::Zellij,
        ScrollbackStrategy::FullScreen,
    ] {
        let backend = VT100Backend::new(size.width, size.height);
        let mut terminal = Terminal::with_screen_size_and_cursor_position_for_test(
            backend,
            size,
            Position::default(),
        );
        terminal.set_viewport_area(Rect::new(
            /*x*/ 0, /*y*/ 10, /*width*/ 24, /*height*/ 2,
        ));
        for row in 0..10 {
            write!(terminal.backend_mut(), "\x1b[{};1Hhistory {row}", row + 1).unwrap();
        }
        for _ in 0..10 {
            for (height, text) in [(5, "popup"), (2, "composer")] {
                Tui::update_inline_viewport_for_resize_reflow(
                    &mut terminal,
                    height,
                    size,
                    strategy,
                )
                .unwrap();
                terminal
                    .draw_with_size(size, |frame| {
                        let area = frame.area();
                        for y in area.top()..area.bottom() {
                            frame
                                .buffer_mut()
                                .set_string(/*x*/ 0, y, text, Style::default());
                        }
                    })
                    .unwrap();
            }
            let closed = terminal
                .backend()
                .vt100()
                .screen()
                .rows(/*start*/ 0, size.width)
                .map(|row| match row.trim_end() {
                    "" => "<blank>".to_string(),
                    row => row.to_string(),
                })
                .collect::<Vec<_>>()
                .join("\n");
            if let Some(first) = &first_closed {
                assert_eq!(&closed, first);
            } else {
                insta::assert_snapshot!(closed, @r"
                <blank>
                <blank>
                <blank>
                history 3
                history 4
                history 5
                history 6
                history 7
                history 8
                history 9
                composer
                composer
                ");
                first_closed = Some(closed);
            }
        }
    }
}
