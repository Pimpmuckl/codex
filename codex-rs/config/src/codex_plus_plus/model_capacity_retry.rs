use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// Controls whether Codex++ stops after its bounded model-capacity retry schedule.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ModelCapacityRetryMode {
    #[default]
    Bounded,
    Indefinite,
}
