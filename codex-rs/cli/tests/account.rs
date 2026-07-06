use anyhow::Result;
use predicates::str::contains;
use serde_json::json;
use std::path::Path;
use tempfile::TempDir;

fn codex_command(codex_home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    cmd.env("CODEX_HOME", codex_home);
    Ok(cmd)
}

#[tokio::test]
async fn account_import_current_and_list() -> Result<()> {
    let codex_home = TempDir::new()?;
    write_auth_json(codex_home.path(), "acct-1", "user@example.com")?;

    let mut import = codex_command(codex_home.path())?;
    import
        .args(["account", "import-current", "Work"])
        .assert()
        .success()
        .stdout(contains("Imported account acct_"));

    let mut list = codex_command(codex_home.path())?;
    list.args(["account", "list"])
        .assert()
        .success()
        .stdout(contains("Work"));

    Ok(())
}

fn write_auth_json(codex_home: &Path, account_id: &str, email: &str) -> Result<()> {
    std::fs::create_dir_all(codex_home)?;
    let auth = json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "id_token": minimal_jwt(account_id, email)?,
            "access_token": "access",
            "refresh_token": "refresh",
            "account_id": account_id
        },
        "last_refresh": null,
    });
    std::fs::write(
        codex_home.join("auth.json"),
        serde_json::to_string_pretty(&auth)?,
    )?;
    Ok(())
}

fn minimal_jwt(account_id: &str, email: &str) -> Result<String> {
    let header = json!({
        "alg": "none",
        "typ": "JWT",
    });
    let payload = json!({
        "email": email,
        "https://api.openai.com/auth": {
            "chatgpt_account_id": account_id,
            "user_id": format!("user-{account_id}")
        }
    });
    let header_b64 = b64url_no_pad(&serde_json::to_vec(&header)?);
    let payload_b64 = b64url_no_pad(&serde_json::to_vec(&payload)?);
    let signature_b64 = b64url_no_pad(b"sig");
    Ok(format!("{header_b64}.{payload_b64}.{signature_b64}"))
}

fn b64url_no_pad(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | b2 as u32;
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 0x3f) as usize] as char);
        }
    }
    out
}
