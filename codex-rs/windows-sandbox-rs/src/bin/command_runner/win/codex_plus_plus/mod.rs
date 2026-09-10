mod current_user_process;

pub(super) use current_user_process::spawn_current_user_process;

// Matches codex_exec_server::CODEX_FS_HELPER_ARG1 without a dependency cycle.
pub(super) const FS_HELPER_ARG: &str = "--codex-run-as-fs-helper";
