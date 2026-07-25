#![cfg(target_os = "windows")]

use anyhow::Result;
use codex_sandboxing::SandboxType;
use codex_sandboxing::SpawnRequest;
use codex_sandboxing::spawn_process;
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use std::fs;
use std::time::Duration;
use std::time::Instant;
use tempfile::TempDir;
use tokio::sync::mpsc;
use tokio::time::timeout;

const BURST_LINES: usize = 128;
const PROBE_ENV: &str = "CODEX_NON_TTY_RUNNER_PROBE";

#[link(name = "Kernel32")]
unsafe extern "system" {
    #[link_name = "GetConsoleProcessList"]
    fn get_console_process_list(process_list: *mut u32, process_count: u32) -> u32;
}

async fn collect_output(mut receiver: mpsc::Receiver<Vec<u8>>) -> Vec<u8> {
    let mut output = Vec::new();
    while let Some(chunk) = receiver.recv().await {
        output.extend(chunk);
    }
    output
}

#[tokio::test(flavor = "current_thread")]
#[allow(clippy::zombie_processes)]
async fn unrestricted_non_tty_uses_private_runner() -> Result<()> {
    if std::env::var_os(PROBE_ENV).is_some() {
        let mut console_pids = [0; 16];
        let count = unsafe {
            get_console_process_list(console_pids.as_mut_ptr(), console_pids.len() as u32)
        } as usize;
        let parent_pid: u32 = std::env::var("CODEX_PARENT_PID")?.parse()?;
        assert!(count <= console_pids.len() && !console_pids[..count].contains(&parent_pid));

        for index in 0..BURST_LINES {
            println!("native-stdout-{index:03}");
            eprintln!("native-stderr-{index:03}");
        }
        std::process::Command::new(std::env::var_os("ComSpec").unwrap())
            .args([
                "/d",
                "/c",
                r#""%SystemRoot%\System32\ping.exe" -n 3 127.0.0.1 >nul & echo survived > "%CODEX_DESCENDANT_MARKER%""#,
            ])
            .spawn()?;
        return Ok(());
    }

    assert!(
        std::env::var_os("CARGO_BIN_EXE_codex-command-runner").is_some()
            || std::env::var_os("CARGO_BIN_EXE_codex_command_runner").is_some(),
        "codex-command-runner must be provided as test data"
    );
    let temp = TempDir::new()?;
    let probe = temp.path().join("native-probe.exe");
    fs::copy(std::env::current_exe()?, &probe)?;
    let marker = temp.path().join("descendant-survived");
    let script = temp.path().join("runner-probe.cmd");
    fs::write(
        &script,
        "@echo shell-stdout\r\n@echo shell-stderr 1>&2\r\n@\"%CODEX_NATIVE_PROBE%\" unrestricted_non_tty_uses_private_runner --nocapture\r\n",
    )?;
    let mut env: HashMap<_, _> = std::env::vars().collect();
    env.insert(PROBE_ENV.into(), "1".into());
    env.insert("CODEX_NATIVE_PROBE".into(), probe.display().to_string());
    env.insert(
        "CODEX_DESCENDANT_MARKER".into(),
        marker.display().to_string(),
    );
    env.insert("CODEX_PARENT_PID".into(), std::process::id().to_string());
    let command = vec![script.display().to_string()];
    let arg0 = None;
    let started_at = Instant::now();
    let spawned = spawn_process(SpawnRequest {
        command: &command,
        cwd: temp.path(),
        env: &env,
        arg0: &arg0,
        sandbox: SandboxType::None,
        windows_sandbox: None,
        tty: false,
        stdin_open: false,
        inherited_fds: &[],
    })
    .await?;
    let codex_utils_pty::SpawnedProcess {
        session: _session,
        stdout_rx,
        stderr_rx,
        exit_rx,
    } = spawned;
    let stdout_task = tokio::spawn(collect_output(stdout_rx));
    let stderr_task = tokio::spawn(collect_output(stderr_rx));
    let exit_code = timeout(Duration::from_secs(10), exit_rx).await??;
    let stdout = timeout(Duration::from_secs(10), stdout_task).await??;
    let stderr = timeout(Duration::from_secs(10), stderr_task).await??;
    assert_eq!(exit_code, 0);
    assert!(
        started_at.elapsed() < Duration::from_secs(5),
        "root process did not exit promptly"
    );

    let stdout = String::from_utf8(stdout)?;
    let stderr = String::from_utf8(stderr)?;
    assert!(stdout.contains("shell-stdout"), "stdout={stdout:?}");
    assert!(stderr.contains("shell-stderr"), "stderr={stderr:?}");
    assert!(!stdout.contains("native-stderr-"), "stdout={stdout:?}");
    assert!(!stderr.contains("native-stdout-"), "stderr={stderr:?}");
    for index in 0..BURST_LINES {
        assert!(
            stdout.contains(&format!("native-stdout-{index:03}")),
            "stdout burst incomplete at {index}"
        );
        assert!(
            stderr.contains(&format!("native-stderr-{index:03}")),
            "stderr burst incomplete at {index}"
        );
    }

    std::thread::sleep(Duration::from_secs(3));
    assert!(!marker.exists(), "runner descendant escaped its job");
    Ok(())
}
