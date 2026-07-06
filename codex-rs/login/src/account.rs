use codex_config::types::AuthCredentialsStoreMode;
use codex_protocol::auth::AuthMode;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::fmt;
use std::io::Write;
use std::path::PathBuf;

use crate::AuthDotJson;
use crate::AuthKeyringBackendKind;
use crate::load_auth_dot_json;
use crate::save_auth;

const ACCOUNTS_DIR: &str = "accounts";
const INDEX_FILE: &str = "index.json";

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct AccountId(String);

impl AccountId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AccountProfile {
    pub id: AccountId,
    pub label: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub priority: u32,
    pub auth: AccountAuthStorage,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AccountAuthStorage {
    pub scope: AccountAuthScope,
    pub path: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountAuthScope {
    File,
}

#[derive(Debug)]
pub struct AccountStore {
    codex_home: PathBuf,
}

#[derive(Default, Deserialize, Serialize)]
struct AccountIndex {
    #[serde(default)]
    accounts: Vec<AccountProfile>,
}

impl AccountStore {
    pub fn new(codex_home: PathBuf) -> Self {
        Self { codex_home }
    }

    pub fn import_current(
        &self,
        label: impl Into<String>,
        root_store_mode: AuthCredentialsStoreMode,
        root_keyring_backend_kind: AuthKeyringBackendKind,
    ) -> std::io::Result<AccountProfile> {
        let label = label.into().trim().to_string();
        if label.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "account label cannot be empty",
            ));
        }

        let auth =
            load_auth_dot_json(&self.codex_home, root_store_mode, root_keyring_backend_kind)?
                .ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::NotFound, "not logged in")
                })?;
        if !is_managed_chatgpt_auth(&auth) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "current auth is not a ChatGPT login",
            ));
        }

        let account_id = account_id_for_auth(&auth)?;
        let account_home = self.account_home(&account_id);
        save_auth(
            &account_home,
            &auth,
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
        )?;

        let mut index = self.load_index()?;
        let auth = AccountAuthStorage {
            scope: AccountAuthScope::File,
            path: auth_path_for_id(&account_id),
        };
        let priority = index
            .accounts
            .iter()
            .find(|profile| profile.id == account_id)
            .map(|profile| profile.priority)
            .unwrap_or_else(|| next_priority(&index.accounts));
        let enabled = index
            .accounts
            .iter()
            .find(|profile| profile.id == account_id)
            .map(|profile| profile.enabled)
            .unwrap_or(true);
        let profile = AccountProfile {
            id: account_id,
            label,
            enabled,
            priority,
            auth,
        };

        index.accounts.retain(|existing| existing.id != profile.id);
        index.accounts.push(profile.clone());
        sort_profiles(&mut index.accounts);
        self.save_index(&index)?;
        Ok(profile)
    }

    pub fn list(&self) -> std::io::Result<Vec<AccountProfile>> {
        let mut accounts = self.load_index()?.accounts;
        sort_profiles(&mut accounts);
        Ok(accounts)
    }

    pub fn enabled_file_accounts(&self) -> std::io::Result<Vec<(AccountId, PathBuf)>> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|account| account.enabled)
            .filter_map(|account| match account.auth.scope {
                AccountAuthScope::File => {
                    let home = self.account_home(&account.id);
                    home.join("auth.json")
                        .is_file()
                        .then_some((account.id, home))
                }
            })
            .collect())
    }

    pub(crate) fn account_home(&self, account_id: &AccountId) -> PathBuf {
        self.accounts_dir().join(account_id.as_str())
    }

    fn accounts_dir(&self) -> PathBuf {
        self.codex_home.join(ACCOUNTS_DIR)
    }

    fn index_path(&self) -> PathBuf {
        self.accounts_dir().join(INDEX_FILE)
    }

    fn load_index(&self) -> std::io::Result<AccountIndex> {
        let path = self.index_path();
        let data = match std::fs::read_to_string(&path) {
            Ok(data) => data,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(AccountIndex::default());
            }
            Err(err) => return Err(err),
        };
        serde_json::from_str(&data).map_err(std::io::Error::other)
    }

    fn save_index(&self, index: &AccountIndex) -> std::io::Result<()> {
        let accounts_dir = self.accounts_dir();
        std::fs::create_dir_all(&accounts_dir)?;
        let path = self.index_path();
        let temp_path = accounts_dir.join("index.json.tmp");
        let json = serde_json::to_vec_pretty(index).map_err(std::io::Error::other)?;
        {
            let mut file = std::fs::File::create(&temp_path)?;
            file.write_all(&json)?;
            file.flush()?;
        }
        std::fs::rename(temp_path, path)
    }
}

fn account_id_for_auth(auth: &AuthDotJson) -> std::io::Result<AccountId> {
    let tokens = auth.tokens.as_ref().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "current auth is not a ChatGPT login",
        )
    })?;

    let identity = tokens
        .account_id
        .as_deref()
        .or(tokens.id_token.chatgpt_account_id.as_deref())
        .map(|value| ("account", value.to_string()))
        .or_else(|| {
            tokens
                .id_token
                .chatgpt_user_id
                .as_deref()
                .map(|value| ("user", value.to_string()))
        })
        .or_else(|| {
            tokens
                .id_token
                .email
                .as_deref()
                .map(|value| ("email", value.to_ascii_lowercase()))
        })
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "current ChatGPT auth is missing a stable account identity",
            )
        })?;

    let mut hasher = Sha256::new();
    hasher.update(identity.0.as_bytes());
    hasher.update(b":");
    hasher.update(identity.1.as_bytes());
    let digest = hasher.finalize();
    let hex = format!("{digest:x}");
    Ok(AccountId(format!("acct_{}", &hex[..16])))
}

fn is_managed_chatgpt_auth(auth: &AuthDotJson) -> bool {
    match auth.auth_mode {
        Some(AuthMode::Chatgpt) => true,
        Some(
            AuthMode::ApiKey
            | AuthMode::ChatgptAuthTokens
            | AuthMode::AgentIdentity
            | AuthMode::PersonalAccessToken
            | AuthMode::BedrockApiKey,
        ) => false,
        None => auth.openai_api_key.is_none() && auth.tokens.is_some(),
    }
}

fn auth_path_for_id(account_id: &AccountId) -> String {
    format!("{ACCOUNTS_DIR}/{account_id}/auth.json")
}

fn next_priority(accounts: &[AccountProfile]) -> u32 {
    accounts
        .iter()
        .map(|profile| profile.priority)
        .max()
        .map_or(0, |priority| priority.saturating_add(1))
}

fn sort_profiles(accounts: &mut [AccountProfile]) {
    accounts.sort_by(|a, b| (a.priority, &a.id).cmp(&(b.priority, &b.id)));
}

fn default_enabled() -> bool {
    true
}

#[cfg(test)]
#[path = "account_tests.rs"]
mod tests;
