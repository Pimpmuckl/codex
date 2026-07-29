//! Minimal exec-server fixture for Bazel-only integration tests.
//!
//! Linking only exec-server avoids depending on the full Codex CLI binary
//! when a test only needs a WebSocket executor endpoint. It handles the arg0
//! helper mode because sandboxed process requests re-exec this binary.

use codex_exec_server::ExecServerRuntimePaths;
use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use std::ffi::OsStr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut args = std::env::args_os();
    let _ = args.next();
    let argv1 = args.next();
    #[cfg(unix)]
    if argv1.as_deref() == Some(OsStr::new(codex_exec_server::CODEX_ARG0_EXEC_HELPER_ARG1)) {
        codex_exec_server::run_arg0_exec_helper_main();
    }
    #[cfg(target_os = "windows")]
    if argv1.as_deref() == Some(OsStr::new(codex_windows_sandbox::CODEX_COMMAND_RUNNER_ARG1)) {
        codex_windows_sandbox::run_command_runner_main()?;
        return Ok(());
    }

    let current_exe = std::env::current_exe()?;
    let runtime_paths =
        ExecServerRuntimePaths::new(current_exe, /*codex_linux_sandbox_exe*/ None)?;
    codex_exec_server::run_main(
        "ws://127.0.0.1:0",
        runtime_paths,
        HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
    )
    .await
}
