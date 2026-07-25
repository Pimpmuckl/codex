use super::DirectProcessDriver;
use super::wait_for_exit_and_output;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn exit_waits_for_both_output_readers() {
    let (exit_tx, exit_rx) = tokio::sync::oneshot::channel();
    let (stdout_done_tx, stdout_done_rx) = tokio::sync::oneshot::channel();
    let (stderr_done_tx, stderr_done_rx) = tokio::sync::oneshot::channel();
    let stdout_reader_handle = tokio::spawn(async move {
        let _ = stdout_done_rx.await;
    });
    let stderr_reader_handle = tokio::spawn(async move {
        let _ = stderr_done_rx.await;
    });
    exit_tx.send(7).expect("send exit");
    let mut wait = Box::pin(wait_for_exit_and_output(
        exit_rx,
        stdout_reader_handle,
        stderr_reader_handle,
    ));

    tokio::select! {
        biased;
        _ = &mut wait => panic!("exit published before stdout completed"),
        _ = std::future::ready(()) => {}
    }
    stdout_done_tx.send(()).expect("complete stdout");
    tokio::task::yield_now().await;
    tokio::select! {
        biased;
        _ = &mut wait => panic!("exit published before stderr completed"),
        _ = std::future::ready(()) => {}
    }
    stderr_done_tx.send(()).expect("complete stderr");

    assert_eq!(wait.await, 7);
}

#[tokio::test]
async fn stream_drop_is_independent() {
    let (stdout_tx, stdout_rx) = tokio::sync::mpsc::channel(1);
    let mut spawned = super::spawn_from_direct_driver(DirectProcessDriver {
        writer_tx: tokio::sync::mpsc::channel(1).0,
        stdout_rx,
        stderr_rx: tokio::sync::mpsc::channel(1).1,
        exit_rx: tokio::sync::oneshot::channel().1,
        stdout_reader_handle: tokio::spawn(async {}),
        stderr_reader_handle: tokio::spawn(async {}),
        terminator: None,
        writer_handle: None,
    });
    drop(spawned.stderr_rx);
    stdout_tx.send(b"stdout".to_vec()).await.unwrap();
    assert_eq!(spawned.stdout_rx.recv().await, Some(b"stdout".to_vec()));
}
