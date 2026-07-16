use super::*;

#[test]
fn builds_stable_user_message_identity_and_envelope() {
    assert_eq!(user_message_item_id("call-123"), "user-message:call-123");
    assert_eq!(user_message_envelope("note"), "[Message for you]\nnote");
}
