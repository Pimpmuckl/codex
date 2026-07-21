use std::num::NonZeroU64;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// Thresholds for automatic redemption of earned usage-reset credits.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AutoRedeemResets {
    pub before_expiry_minutes: NonZeroU64,
    pub weekly_exhausted_min_wait_hours: NonZeroU64,
}

impl Default for AutoRedeemResets {
    fn default() -> Self {
        Self {
            before_expiry_minutes: NonZeroU64::new(60).unwrap_or(NonZeroU64::MIN),
            weekly_exhausted_min_wait_hours: NonZeroU64::new(72).unwrap_or(NonZeroU64::MIN),
        }
    }
}
