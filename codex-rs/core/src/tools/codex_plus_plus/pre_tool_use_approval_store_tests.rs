use super::ExactPreToolUseApproval;
use super::scope;
use super::take;
use crate::shell::ShellType;
use crate::tools::codex_plus_plus::pre_tool_use_review::ExecCommandShellTarget;
use crate::tools::codex_plus_plus::pre_tool_use_review::PreToolUseExecutionTarget;
use codex_protocol::protocol::TurnEnvironmentSelection;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use std::path::PathBuf;
use tokio::task::yield_now;

#[tokio::test]
async fn exact_approval_is_one_shot_and_task_scoped() {
    let approval = ExactPreToolUseApproval {
        execution_target: None,
    };
    let (approved, unapproved) = tokio::join!(
        scope(Some(approval.clone()), async {
            yield_now().await;
            (take(), take())
        }),
        scope(/*approval*/ None, async {
            yield_now().await;
            take()
        }),
    );

    assert_eq!((approved, unapproved), ((Some(approval), None), None));
    assert_eq!(take(), None);
}

#[test]
fn local_exact_approval_requires_the_reviewed_absolute_shell_path() -> anyhow::Result<()> {
    let shell_dir = std::env::current_dir()?;
    let reviewed_shell = ExecCommandShellTarget {
        shell_type: ShellType::PowerShell,
        executable_path: shell_dir.join("reviewed-shell"),
    };
    let mut execution_target = PreToolUseExecutionTarget::from(TurnEnvironmentSelection {
        environment_id: "local".to_string(),
        cwd: PathUri::parse("file:///C:/workspace")?,
        workspace_roots: Vec::new(),
    });
    execution_target.exec_command_shell = Some(reviewed_shell.clone());
    let approval = ExactPreToolUseApproval {
        execution_target: Some(execution_target),
    };

    assert!(approval.matches_local_exec_command_shell(&reviewed_shell));
    assert_eq!(
        [
            ExecCommandShellTarget {
                executable_path: PathBuf::from("pwsh.exe"),
                ..reviewed_shell.clone()
            },
            ExecCommandShellTarget {
                executable_path: shell_dir.join("different-shell"),
                ..reviewed_shell
            },
        ]
        .map(|shell| approval.matches_local_exec_command_shell(&shell)),
        [false, false]
    );
    Ok(())
}
