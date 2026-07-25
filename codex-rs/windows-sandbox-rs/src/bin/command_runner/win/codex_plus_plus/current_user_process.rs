use anyhow::Context;
use anyhow::Result;
use codex_windows_sandbox::ProcessExecutionMode;
use codex_windows_sandbox::SpawnRequest;
use codex_windows_sandbox::StderrMode;
use codex_windows_sandbox::StdinMode;
use codex_windows_sandbox::spawn_process_with_pipes;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Threading::ResumeThread;

use super::super::IpcSpawnedProcess;
use super::super::assign_process_to_job;
use super::ProcessTerminationTarget;

pub(in super::super) fn spawn_current_user_process(
    req: &SpawnRequest,
) -> Result<IpcSpawnedProcess> {
    if req.tty {
        anyhow::bail!("runner: current-user execution is non-TTY only");
    }
    let stdin_mode = match req.stdin_open {
        true => StdinMode::Open,
        false => StdinMode::Closed,
    };
    let pipes = spawn_process_with_pipes(
        ProcessExecutionMode::CurrentUser,
        &req.command,
        &req.cwd,
        &req.env,
        stdin_mode,
        StderrMode::InheritOutput,
        req.child_console_mode,
        req.use_private_desktop,
        Some(req.codex_home.as_path()),
    )?;
    let termination_target =
        match ProcessTerminationTarget::required(pipes.process, assign_process_to_job) {
            Ok(target) => target,
            Err(err) => {
                if let Some(stdin_handle) = pipes.stdin_write {
                    unsafe { CloseHandle(stdin_handle) };
                }
                return Err(err);
            }
        };
    if unsafe { ResumeThread(termination_target.thread()) } == u32::MAX {
        termination_target.terminate();
        if let Some(stdin_handle) = pipes.stdin_write {
            unsafe { CloseHandle(stdin_handle) };
        }
        return Err(std::io::Error::last_os_error()).context("ResumeThread failed");
    }
    Ok(IpcSpawnedProcess {
        log_dir: req.codex_home.clone(),
        termination_target,
        stdout_handle: INVALID_HANDLE_VALUE,
        stderr_handle: INVALID_HANDLE_VALUE,
        stdin_handle: pipes.stdin_write,
        conpty_owner: None,
        hpc_handle: None,
        _pipe_handles: Some(pipes),
    })
}
