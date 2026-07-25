use anyhow::Context;
use anyhow::Result;
use std::path::Path;
use std::path::PathBuf;
use std::ptr;
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
use windows_sys::Win32::System::Threading::CREATE_UNICODE_ENVIRONMENT;
use windows_sys::Win32::System::Threading::CreateProcessW;
use windows_sys::Win32::System::Threading::EXTENDED_STARTUPINFO_PRESENT;
use windows_sys::Win32::System::Threading::PROCESS_INFORMATION;
use windows_sys::Win32::System::Threading::STARTUPINFOW;

pub const CODEX_COMMAND_RUNNER_ARG1: &str = "--codex-run-as-command-runner";

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct RunnerCommand {
    pub executable: PathBuf,
    pub internal_arg: Option<&'static str>,
}

fn select_runner_command(helper: Option<PathBuf>, current_exe: PathBuf) -> RunnerCommand {
    match helper {
        Some(executable) => RunnerCommand {
            executable,
            internal_arg: None,
        },
        None => RunnerCommand {
            executable: current_exe,
            internal_arg: Some(CODEX_COMMAND_RUNNER_ARG1),
        },
    }
}

pub(crate) fn resolve_runner_command(helper: &Path) -> Result<RunnerCommand> {
    let helper = helper
        .is_file()
        .then(|| helper.to_path_buf())
        .or_else(|| which::which(helper).ok());
    let current_exe = if helper.is_none() {
        std::env::current_exe().context("resolve current Codex executable for command runner")?
    } else {
        PathBuf::new()
    };
    Ok(select_runner_command(helper, current_exe))
}

/// # Safety
/// All pointers and inherited handles must remain valid for the call.
pub(crate) unsafe fn create_process(
    executable: &[u16],
    command_line: &mut [u16],
    cwd: &[u16],
    startup_info: &STARTUPINFOW,
    process_info: &mut PROCESS_INFORMATION,
) -> i32 {
    CreateProcessW(
        executable.as_ptr(),
        command_line.as_mut_ptr(),
        ptr::null_mut(),
        ptr::null_mut(),
        /*bInheritHandles*/ 1,
        CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
        ptr::null(),
        cwd.as_ptr(),
        startup_info,
        process_info,
    )
}

#[cfg(test)]
#[path = "current_user_runner_tests.rs"]
mod tests;
