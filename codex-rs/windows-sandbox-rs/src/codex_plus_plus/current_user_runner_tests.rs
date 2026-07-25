use super::CODEX_COMMAND_RUNNER_ARG1;
use super::RunnerCommand;
use super::select_runner_command;
use pretty_assertions::assert_eq;
use std::path::PathBuf;

#[test]
fn runner_selector_prefers_helper_and_falls_back_to_codex() {
    let helper = PathBuf::from(r"C:\package\codex-command-runner.exe");
    let codex = PathBuf::from(r"C:\build\codex.exe");
    assert_eq!(
        select_runner_command(Some(helper.clone()), codex.clone()),
        RunnerCommand {
            executable: helper,
            internal_arg: None,
        }
    );
    assert_eq!(
        select_runner_command(None, codex.clone()),
        RunnerCommand {
            executable: codex,
            internal_arg: Some(CODEX_COMMAND_RUNNER_ARG1),
        }
    );
}
