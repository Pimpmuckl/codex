//! Minimal exec-server fixture for Bazel-only integration tests.
//!
//! Linking only exec-server avoids depending on the full Codex CLI binary
//! when a test only needs a WebSocket executor endpoint.

use codex_exec_server::ExecServerRuntimePaths;
#[cfg(target_os = "windows")]
use std::ffi::OsStr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    #[cfg(target_os = "windows")]
    {
        let mut args = std::env::args_os();
        let _ = args.next();
        if args.next().as_deref()
            == Some(OsStr::new(
                codex_windows_sandbox::CODEX_COMMAND_RUNNER_ARG1,
            ))
        {
            codex_windows_sandbox::run_command_runner_main()?;
            return Ok(());
        }
    }

    let current_exe = std::env::current_exe()?;
    let runtime_paths =
        ExecServerRuntimePaths::new(current_exe, /*codex_linux_sandbox_exe*/ None)?;
    codex_exec_server::run_main("ws://127.0.0.1:0", runtime_paths).await
}
