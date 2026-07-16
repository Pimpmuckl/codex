use super::*;
use pretty_assertions::assert_eq;

#[test]
fn normalizes_and_bounds_messages_by_character_count() {
    assert_eq!(normalize_message("  hello  "), Ok("hello"));
    assert_eq!(
        normalize_message(" \n\t ").unwrap_err(),
        "`message` must not be empty."
    );
    assert!(normalize_message(&"é".repeat(MAX_MESSAGE_CHARS)).is_ok());
    assert_eq!(
        normalize_message(&"é".repeat(MAX_MESSAGE_CHARS + 1)).unwrap_err(),
        "`message` must be at most 2000 characters."
    );
}
