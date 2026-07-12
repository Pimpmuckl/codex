use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// Controls whether Codex++ starts unused weekly usage windows automatically.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WeeklyUsageWindowAutoStart {
    #[default]
    Enabled,
    Disabled,
}
