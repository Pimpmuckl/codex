use super::*;
use crate::tools::context::ToolPayload;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;

fn exec_target(
    shell_type: ShellType,
    executable_path: impl Into<PathBuf>,
) -> anyhow::Result<PreToolUseExecutionTarget> {
    Ok(PreToolUseExecutionTarget {
        environment_id: "local".to_string(),
        cwd: PathUri::parse("file:///workspace")?,
        exec_command_shell: Some(ExecCommandShellTarget {
            shell_type,
            executable_path: executable_path.into(),
        }),
    })
}

#[test]
fn approval_receipt_binds_exec_command_shell_type_and_path() -> anyhow::Result<()> {
    let payload = ToolPayload::Function {
        arguments: r#"{"cmd":"remove target","shell":"powershell.exe"}"#.to_string(),
    };
    let reviewed_target = exec_target(ShellType::PowerShell, "powershell.exe")?;
    let receipt = PreToolUseApprovalReceipt {
        call_id: "call-1".to_string(),
        payload: payload.clone(),
        execution_target: Some(reviewed_target.clone()),
        reviewed_action_truncated: false,
    };

    assert!(receipt.authorizes("call-1", &payload, Some(&reviewed_target)));
    assert_eq!(
        [
            exec_target(ShellType::Cmd, "powershell.exe")?,
            exec_target(ShellType::PowerShell, "pwsh.exe")?,
        ]
        .map(|target| receipt.authorizes("call-1", &payload, Some(&target))),
        [false, false]
    );
    Ok(())
}
