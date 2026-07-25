use super::windows_common::start_runner_pipe_writer;
use super::windows_common::start_runner_stdin_writer;
use super::windows_common::start_runner_stdout_reader;
use crate::ipc_framed::ChildConsoleMode;
use crate::ipc_framed::EmptyPayload;
use crate::ipc_framed::FramedMessage;
use crate::ipc_framed::IPC_PROTOCOL_VERSION;
use crate::ipc_framed::Message;
use crate::ipc_framed::RunnerExecutionMode;
use crate::ipc_framed::SpawnRequest;
use crate::runner_client::RunnerLaunch;
use crate::runner_client::spawn_runner_transport;
use anyhow::Result;
use codex_protocol::models::PermissionProfile;
use codex_utils_pty::DirectProcessDriver;
use codex_utils_pty::SpawnedProcess;
use codex_utils_pty::spawn_from_direct_driver;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use tokio::sync::{broadcast, mpsc, oneshot};

fn direct_output_receiver(mut file: File) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel(256);
    tokio::task::spawn_blocking(move || {
        let mut buffer = [0; 8192];
        while let Ok(count) = file.read(&mut buffer) {
            if count == 0 || tx.blocking_send(buffer[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    rx
}

pub async fn spawn_current_user_runner_session(
    codex_home: &Path,
    command: Vec<String>,
    cwd: &Path,
    env: HashMap<String, String>,
    stdin_open: bool,
) -> Result<SpawnedProcess> {
    let codex_home = codex_home.to_path_buf();
    let cwd = cwd.to_path_buf();
    let request = SpawnRequest {
        command,
        cwd: cwd.clone(),
        env,
        execution_mode: RunnerExecutionMode::CurrentUser,
        child_console_mode: ChildConsoleMode::NoWindow,
        permission_profile: PermissionProfile::Disabled,
        workspace_roots: Vec::new(),
        codex_home: codex_home.clone(),
        real_codex_home: codex_home.clone(),
        cap_sids: Vec::new(),
        timeout_ms: None,
        tty: false,
        stdin_open,
        use_private_desktop: false,
    };
    let transport = tokio::task::spawn_blocking(move || {
        spawn_runner_transport(
            &codex_home,
            &cwd,
            RunnerLaunch::CurrentUser,
            /*log_dir*/ None,
            request,
        )
    })
    .await
    .map_err(|err| anyhow::anyhow!("runner handshake task failed: {err}"))??;
    let (pipe_write, pipe_read, stdout_file, stderr_file) =
        transport.into_files_with_output()?;
    let (writer_tx, writer_rx) = mpsc::channel::<Vec<u8>>(128);
    let stdout_rx = direct_output_receiver(stdout_file);
    let stderr_rx = direct_output_receiver(stderr_file);
    let (exit_tx, exit_rx) = oneshot::channel::<i32>();
    let outbound_tx = start_runner_pipe_writer(pipe_write);
    let writer_handle =
        start_runner_stdin_writer(writer_rx, outbound_tx.clone(), false, stdin_open);
    let terminator = Some(Box::new(move || {
        let _ = outbound_tx.send(FramedMessage {
            version: IPC_PROTOCOL_VERSION,
            message: Message::Terminate {
                payload: EmptyPayload::default(),
            },
        });
    }) as Box<dyn FnMut() + Send + Sync>);
    let (discard_output, _) = broadcast::channel(1);
    start_runner_stdout_reader(pipe_read, discard_output, None, exit_tx);
    let spawned = spawn_from_direct_driver(DirectProcessDriver {
        writer_tx,
        stdout_rx,
        stderr_rx,
        exit_rx,
        terminator,
        writer_handle: Some(writer_handle),
    });
    if !stdin_open {
        spawned.session.close_stdin();
    }
    Ok(spawned)
}
