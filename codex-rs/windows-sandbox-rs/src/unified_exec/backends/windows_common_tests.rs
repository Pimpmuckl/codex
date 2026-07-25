use super::start_runner_stdout_reader;
use crate::ipc_framed as ipc;
use pretty_assertions::assert_eq;

fn collect_control_failure(mut pipe: std::fs::File) -> (Vec<u8>, i32) {
    std::io::Seek::rewind(&mut pipe).unwrap();
    let (output_tx, mut output_rx) = tokio::sync::mpsc::channel(1);
    let (exit_tx, exit_rx) = tokio::sync::oneshot::channel();
    start_runner_stdout_reader(
        pipe,
        tokio::sync::broadcast::channel(1).0,
        None,
        Some(output_tx),
        exit_tx,
    );
    let diagnostic = output_rx.blocking_recv().unwrap();
    (diagnostic, exit_rx.blocking_recv().unwrap())
}

#[test]
fn runner_control_failures_are_emitted_on_stderr_before_exit() {
    let mut runner_error = tempfile::tempfile().unwrap();
    ipc::write_frame(
        &mut runner_error,
        &ipc::FramedMessage {
            version: ipc::IPC_PROTOCOL_VERSION,
            message: ipc::Message::Error {
                payload: ipc::ErrorPayload {
                    message: "child failed".to_string(),
                    stage: ipc::ErrorStage::SpawnChild,
                    windows_error_code: None,
                },
            },
        },
    )
    .unwrap();
    assert_eq!(
        collect_control_failure(runner_error),
        (b"runner error: child failed\n".to_vec(), -1)
    );
    assert_eq!(
        collect_control_failure(tempfile::tempfile().unwrap()),
        (
            b"runner error: runner pipe closed before exit\n".to_vec(),
            -1
        )
    );
    let mut malformed = tempfile::tempfile().unwrap();
    std::io::Write::write_all(&mut malformed, &1_u32.to_le_bytes()).unwrap();
    std::io::Write::write_all(&mut malformed, b"{").unwrap();
    let (diagnostic, exit) = collect_control_failure(malformed);
    assert!(String::from_utf8_lossy(&diagnostic).starts_with("runner error: runner read failed:"));
    assert_eq!(exit, -1);
}
