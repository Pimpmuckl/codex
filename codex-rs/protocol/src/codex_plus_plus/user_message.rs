pub const USER_MESSAGE_ITEM_ID_PREFIX: &str = "user-message:";
pub const USER_MESSAGE_ENVELOPE_PREFIX: &str = "[Message for you]\n\n";

pub fn user_message_item_id(tool_call_id: &str) -> String {
    format!("{USER_MESSAGE_ITEM_ID_PREFIX}{tool_call_id}")
}

pub fn user_message_envelope(message: &str) -> String {
    format!("{USER_MESSAGE_ENVELOPE_PREFIX}{message}")
}

#[cfg(test)]
#[path = "user_message_tests.rs"]
mod tests;
