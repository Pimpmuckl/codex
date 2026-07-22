use super::collect_output_until_exit;
use super::collect_split_output;
use super::combine_spawned_output;
use super::find_python;
use super::wait_for_output_contains;
use crate::SpawnedProcess;
use crate::TerminalSize;
use crate::spawn_pipe_process;
use crate::spawn_pipe_process_no_stdin;
use crate::spawn_pty_process;
use std::collections::HashMap;
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Stdio;

use pretty_assertions::assert_eq;

const READY_MARKER: &str = "__CODEX_CHILD_READY__";
const VALUE_MARKER: &str = "__CODEX_CHILD_VALUE__";
const CONSOLE_TEST_ROLE_ENV: &str = "CODEX_PTY_CONSOLE_TEST_ROLE";
const CONSOLE_TEST_STDIN_ENV: &str = "CODEX_PTY_CONSOLE_TEST_STDIN";
const CONSOLE_TEST_NAME: &str =
    "tests::windows_tests::pipe_processes_do_not_inherit_parent_console";

struct WindowsShell {
    name: &'static str,
    program: String,
    args: Vec<String>,
    child_command: String,
}

fn find_powershell() -> Option<String> {
    ["pwsh.exe", "powershell.exe"]
        .into_iter()
        .find_map(|candidate| {
            std::process::Command::new(candidate)
                .args(["-NoLogo", "-NoProfile", "-Command", "exit 0"])
                .status()
                .ok()
                .filter(std::process::ExitStatus::success)
                .map(|_| candidate.to_string())
        })
}

fn utf8_hex(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join("")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conpty_delivers_input_to_foreground_children() -> anyhow::Result<()> {
    let Some(python) = find_python() else {
        eprintln!("python not found; skipping ConPTY input test");
        return Ok(());
    };
    let code = format!(
        "print('__CODEX_CHILD_'+'READY__', flush=True); value=input(); print('{VALUE_MARKER}'+value.encode('utf-8').hex(), flush=True)"
    );
    let expected = "cafeé 漢字";
    let expected_marker = format!("{VALUE_MARKER}{}", utf8_hex(expected));
    let mut shells = vec![WindowsShell {
        name: "cmd",
        program: std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string()),
        args: vec!["/D".to_string(), "/Q".to_string()],
        child_command: format!("\"{}\" -u -c \"{code}\"", python.replace('"', "\"\"")),
    }];
    if let Some(program) = find_powershell() {
        shells.push(WindowsShell {
            name: "PowerShell",
            program,
            args: vec!["-NoLogo".to_string(), "-NoProfile".to_string()],
            child_command: format!("& '{}' -u -c \"{code}\"", python.replace('\'', "''")),
        });
    }
    let env: HashMap<String, String> = std::env::vars().collect();

    for shell in shells {
        let spawned = spawn_pty_process(
            &shell.program,
            &shell.args,
            Path::new("."),
            &env,
            /*arg0*/ &None,
            TerminalSize::default(),
            &[],
        )
        .await?;
        let (session, mut output_rx, exit_rx) = combine_spawned_output(spawned);
        let writer = session.writer_sender();
        writer
            .send(format!("{}\n", shell.child_command).into_bytes())
            .await?;
        wait_for_output_contains(&mut output_rx, READY_MARKER, /*timeout_ms*/ 10_000)
            .await
            .map_err(|err| anyhow::anyhow!("{} child did not become ready: {err}", shell.name))?;

        writer
            .send(format!("{expected}X\u{8}\n").into_bytes())
            .await?;
        let mut output =
            wait_for_output_contains(&mut output_rx, &expected_marker, /*timeout_ms*/ 10_000)
                .await
                .map_err(|err| {
                    anyhow::anyhow!("{} child received incorrect input: {err}", shell.name)
                })?;

        writer.send(b"exit 0\n".to_vec()).await?;
        let (remaining, exit_code) =
            collect_output_until_exit(output_rx, exit_rx, /*timeout_ms*/ 10_000).await;
        output.extend_from_slice(&remaining);

        assert_eq!(
            exit_code,
            0,
            "{} did not exit cleanly: {:?}",
            shell.name,
            String::from_utf8_lossy(&output)
        );
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conpty_ctrl_c_interrupts_powershell_foreground_child() -> anyhow::Result<()> {
    let Some(program) = find_powershell() else {
        return Ok(());
    };
    let args = vec!["-NoLogo".to_string(), "-NoProfile".to_string()];
    let env: HashMap<String, String> = std::env::vars().collect();
    let spawned = spawn_pty_process(
        &program,
        &args,
        Path::new("."),
        &env,
        /*arg0*/ &None,
        TerminalSize::default(),
        &[],
    )
    .await?;
    let (session, mut output_rx, exit_rx) = combine_spawned_output(spawned);
    let writer = session.writer_sender();
    writer.send(b"ping.exe -4 -t localhost\n".to_vec()).await?;
    wait_for_output_contains(&mut output_rx, "127.0.0.1", /*timeout_ms*/ 10_000).await?;

    writer.send(vec![0x03]).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    writer.send(b"cmd.exe /D /C ver\n".to_vec()).await?;
    let mut output = wait_for_output_contains(
        &mut output_rx,
        "Microsoft Windows",
        /*timeout_ms*/ 10_000,
    )
    .await?;

    writer.send(b"exit 0\n".to_vec()).await?;
    let (remaining, exit_code) =
        collect_output_until_exit(output_rx, exit_rx, /*timeout_ms*/ 10_000).await;
    output.extend_from_slice(&remaining);
    assert_eq!(
        exit_code,
        0,
        "PowerShell did not resume after Ctrl-C: {:?}",
        String::from_utf8_lossy(&output)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pipe_processes_do_not_inherit_parent_console() -> anyhow::Result<()> {
    match std::env::var(CONSOLE_TEST_ROLE_ENV).as_deref() {
        Ok("probe") => {
            let console_access = (
                std::fs::File::open("CONIN$").is_ok(),
                std::fs::OpenOptions::new()
                    .write(true)
                    .open("CONOUT$")
                    .is_ok(),
            );
            assert_eq!(
                console_access,
                (false, false),
                "pipe child retained access to the parent console"
            );

            let mut stdin = String::new();
            std::io::stdin().read_to_string(&mut stdin)?;
            assert_eq!(stdin, std::env::var(CONSOLE_TEST_STDIN_ENV)?);
            println!("console-probe-stdout");
            eprintln!("console-probe-stderr");
        }
        Ok("parent") => {
            assert!(
                std::fs::File::open("CONIN$").is_ok(),
                "test parent did not receive its requested console"
            );
            for expected_stdin in ["pipe-stdin\n", ""] {
                let program = std::env::current_exe()?.to_string_lossy().into_owned();
                let args = vec![
                    "--exact".to_string(),
                    CONSOLE_TEST_NAME.to_string(),
                    "--nocapture".to_string(),
                ];
                let mut env: HashMap<String, String> = std::env::vars().collect();
                env.insert(CONSOLE_TEST_ROLE_ENV.to_string(), "probe".to_string());
                env.insert(
                    CONSOLE_TEST_STDIN_ENV.to_string(),
                    expected_stdin.to_string(),
                );
                let spawned = if expected_stdin.is_empty() {
                    spawn_pipe_process_no_stdin(
                        &program,
                        &args,
                        Path::new("."),
                        &env,
                        /*arg0*/ &None,
                        &[],
                    )
                    .await?
                } else {
                    spawn_pipe_process(
                        &program,
                        &args,
                        Path::new("."),
                        &env,
                        /*arg0*/ &None,
                        &[],
                    )
                    .await?
                };
                let SpawnedProcess {
                    session,
                    stdout_rx,
                    stderr_rx,
                    exit_rx,
                } = spawned;
                if !expected_stdin.is_empty() {
                    let writer = session.writer_sender();
                    writer.send(expected_stdin.as_bytes().to_vec()).await?;
                    drop(writer);
                    session.close_stdin();
                }

                let stdout_task = tokio::spawn(collect_split_output(stdout_rx));
                let stderr_task = tokio::spawn(collect_split_output(stderr_rx));
                let timeout = tokio::time::Duration::from_secs(10);
                let code = tokio::time::timeout(timeout, exit_rx).await??;
                let stdout = tokio::time::timeout(timeout, stdout_task).await??;
                let stderr = tokio::time::timeout(timeout, stderr_task).await??;
                assert_eq!(
                    code,
                    0,
                    "console probe failed:\nstdout: {}\nstderr: {}",
                    String::from_utf8_lossy(&stdout),
                    String::from_utf8_lossy(&stderr)
                );
                assert!(
                    String::from_utf8_lossy(&stdout).contains("console-probe-stdout"),
                    "missing captured stdout: {}",
                    String::from_utf8_lossy(&stdout)
                );
                assert!(
                    String::from_utf8_lossy(&stderr).contains("console-probe-stderr"),
                    "missing captured stderr: {}",
                    String::from_utf8_lossy(&stderr)
                );
            }
        }
        _ => {
            let output = std::process::Command::new(std::env::current_exe()?)
                .args(["--exact", CONSOLE_TEST_NAME, "--nocapture"])
                .env(CONSOLE_TEST_ROLE_ENV, "parent")
                .creation_flags(winapi::um::winbase::CREATE_NEW_CONSOLE)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()?;
            assert!(
                output.status.success(),
                "console parent failed:\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    Ok(())
}
