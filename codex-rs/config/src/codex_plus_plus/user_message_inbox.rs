use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// Controls whether agents can leave durable, non-blocking messages for the user.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum UserMessageInbox {
    Enabled,
    #[default]
    Disabled,
}
