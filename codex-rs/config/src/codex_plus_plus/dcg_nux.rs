use crate::ConfigLayerStack;

/// Returns whether the global Destructive Command Guard introduction was shown.
pub fn shown(config: &ConfigLayerStack) -> bool {
    config
        .effective_user_config()
        .and_then(|config| config.get("codex_plus_plus_dcg_nux_shown").cloned())
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}
