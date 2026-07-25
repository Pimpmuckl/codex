#[cfg(target_os = "windows")]
fn main() -> anyhow::Result<()> {
    codex_windows_sandbox::run_command_runner_main()
}

#[cfg(not(target_os = "windows"))]
fn main() {
    panic!("codex-command-runner is Windows-only");
}
