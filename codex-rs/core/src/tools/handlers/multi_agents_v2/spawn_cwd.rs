use crate::config::Config;
use crate::session::turn_context::TurnContext;
use codex_protocol::protocol::TurnEnvironmentSelection;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;

pub(super) fn apply_cwd_override(
    config: &mut Config,
    turn: &TurnContext,
    cwd: Option<&str>,
) -> Vec<TurnEnvironmentSelection> {
    let mut environments = turn.environments.to_selections();
    let Some(cwd) = cwd.map(str::trim).filter(|cwd| !cwd.is_empty()) else {
        return environments;
    };

    #[allow(deprecated)]
    let cwd = AbsolutePathBuf::resolve_path_against_base(cwd, turn.cwd.as_path());
    let cwd_uri = PathUri::from_abs_path(&cwd);
    config.cwd = cwd;
    for environment in &mut environments {
        environment.cwd = cwd_uri.clone();
    }
    environments
}
