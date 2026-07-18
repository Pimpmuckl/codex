use super::*;
use base64::Engine;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn captured_auth_preserves_stable_identity_when_visible_fields_match() {
    let first = captured_status_auth(access_token("workspace-a")).expect("first auth should parse");
    let second =
        captured_status_auth(access_token("workspace-b")).expect("second auth should parse");

    assert_eq!(first.email, second.email);
    assert_eq!(first.plan_type, second.plan_type);
    assert_ne!(first.account_id, second.account_id);
}

fn access_token(account_id: &str) -> String {
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"{}");
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&json!({
            "email": "same@example.com",
            "https://api.openai.com/auth": {
                "chatgpt_account_id": account_id,
                "chatgpt_plan_type": "plus"
            }
        }))
        .expect("claims should serialize"),
    );
    format!("{header}.{payload}.signature")
}
