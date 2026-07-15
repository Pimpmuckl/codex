use anyhow::Context;
use codex_install_context::InstallContext;
use codex_install_context::InstallMethod;
use codex_install_context::StandalonePlatform;
use codex_install_context::codex_plus_plus::UpdatePlan;
use codex_install_context::codex_plus_plus::UpdateTarget;
use codex_tui::UpdateAction;
use serde::Deserialize;
use sha2::Digest;
use sha2::Sha256;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use super::upstream_switch::GithubRelease;
use super::upstream_switch::absolute_env;
use super::upstream_switch::latest_release;
use super::upstream_switch::read_json;
use super::upstream_switch::run_quiet;
use super::upstream_switch::stable_release_version;

pub(super) struct StandaloneSwitch {
    action: UpdateAction,
    pub(super) upstream_version: String,
    visible_exe: PathBuf,
    rollback: Rollback,
}

struct Rollback {
    generation: String,
    binary: PathBuf,
    binary_sha256: String,
    pointer: PathBuf,
    shim: FileSnapshot,
    windows_user_path: Option<String>,
}

struct FileSnapshot {
    path: PathBuf,
    contents: Vec<u8>,
    permissions: fs::Permissions,
}

#[derive(Deserialize)]
struct PackageMetadata {
    version: String,
    target: String,
}

pub(super) async fn preflight(
    context: &InstallContext,
    upstream_plan: UpdatePlan,
) -> anyhow::Result<Option<StandaloneSwitch>> {
    let InstallMethod::Standalone { release_dir, .. } = &context.method else {
        unreachable!("standalone update plan requires a standalone install context")
    };
    let UpdateTarget::Standalone { platform, .. } = upstream_plan.target() else {
        unreachable!("standalone preflight requires a standalone update plan")
    };
    let metadata: PackageMetadata = read_json(&release_dir.join("codex-package.json"))?;
    match metadata.version.split_once("-fork.") {
        None if version_base(&metadata.version) => return Ok(None),
        Some((base, revision)) if version_base(base) && revision.parse::<u64>().is_ok() => {}
        _ => anyhow::bail!("running standalone package has invalid version provenance"),
    }
    let shim_dir = absolute_env("CODEX_PLUS_PLUS_SHIM_DIR")
        .context("Codex++ shim provenance is missing; reinstall Codex++ before switching")?;
    let mut rollback = local_rollback(release_dir, platform, &shim_dir, &metadata)?;
    if platform == StandalonePlatform::Windows {
        rollback.windows_user_path = Some(read_windows_user_path()?);
    }
    let (release, prefix) = latest_release(upstream_plan).await?;
    let upstream_version = stable_release_version(&release, prefix)?;
    asset_sha256(
        &release,
        &format!("codex-package-{}.tar.gz", metadata.target),
    )?;
    asset_sha256(&release, "codex-package_SHA256SUMS")?;
    let visible_exe = match platform {
        StandalonePlatform::Unix => shim_dir.join("codex"),
        StandalonePlatform::Windows => {
            absolute_env("LOCALAPPDATA")?.join("Programs/OpenAI/Codex/bin/codex.exe")
        }
    };
    Ok(Some(StandaloneSwitch {
        action: UpdateAction::from_update_plan(upstream_plan)
            .context("standalone update plan has no action")?,
        upstream_version,
        visible_exe,
        rollback,
    }))
}

pub(super) fn install_upstream(switch: &StandaloneSwitch) -> anyhow::Result<()> {
    let (program, args) = switch.action.command_args();
    let install_dir = switch
        .visible_exe
        .parent()
        .context("installed executable has no parent directory")?;
    let mut command = Command::new(program);
    command.args(args);
    command
        .env("CODEX_RELEASE", &switch.upstream_version)
        .env("CODEX_NON_INTERACTIVE", "1")
        .env("CODEX_INSTALL_DIR", install_dir);
    run_quiet(command, "official Codex installer")?;
    if switch.action == UpdateAction::StandaloneWindows {
        let fork_dir = switch
            .rollback
            .shim
            .path
            .parent()
            .context("Codex++ shim has no parent directory")?;
        let path = normalized_windows_user_path(&read_windows_user_path()?, install_dir, fork_dir);
        restore_windows_user_path(&path)?;
    }
    Ok(())
}

pub(super) fn verify_upstream(switch: &StandaloneSwitch) -> anyhow::Result<()> {
    let mut command = Command::new(&switch.visible_exe);
    if switch.action == UpdateAction::StandaloneWindows {
        command = Command::new("powershell");
        command.args(["-NoProfile", "-NonInteractive", "-c", "$env:Path=[Environment]::GetEnvironmentVariable('Path','Machine')+';'+[Environment]::GetEnvironmentVariable('Path','User'); codex --version"]);
    } else {
        command.arg("--version");
    }
    let output = command
        .output()
        .context("failed to run shell-visible upstream Codex")?;
    let stdout = String::from_utf8(output.stdout)?;
    if !output.status.success()
        || stdout.split_whitespace().last() != Some(switch.upstream_version.as_str())
    {
        anyhow::bail!(
            "installed Codex did not verify as upstream version {}",
            switch.upstream_version
        );
    }
    Ok(())
}

pub(super) fn rollback_fork(switch: &StandaloneSwitch) -> anyhow::Result<()> {
    let snapshot = &switch.rollback.shim;
    if fs::read(&snapshot.path).ok().as_deref() != Some(snapshot.contents.as_slice()) {
        if fs::symlink_metadata(&snapshot.path).is_ok() {
            fs::remove_file(&snapshot.path)
                .with_context(|| format!("failed to replace {}", snapshot.path.display()))?;
        }
        fs::write(&snapshot.path, &snapshot.contents)
            .with_context(|| format!("failed to restore {}", snapshot.path.display()))?;
        fs::set_permissions(&snapshot.path, snapshot.permissions.clone())?;
    }
    if fs::read_to_string(&switch.rollback.pointer)?.trim() != switch.rollback.generation {
        fs::write(&switch.rollback.pointer, &switch.rollback.generation)?;
    }
    if let Some(path) = &switch.rollback.windows_user_path {
        restore_windows_user_path(path)?;
    }
    if file_sha256(&switch.rollback.binary)? != switch.rollback.binary_sha256 {
        anyhow::bail!("preserved Codex++ generation digest changed; reinstall Codex++");
    }
    Ok(())
}

fn local_rollback(
    release_dir: &Path,
    platform: StandalonePlatform,
    shim_dir: &Path,
    metadata: &PackageMetadata,
) -> anyhow::Result<Rollback> {
    let entrypoint = match platform {
        StandalonePlatform::Unix => "bin/codex",
        StandalonePlatform::Windows => "bin/codex.exe",
    };
    let supported = matches!(
        (platform, metadata.target.as_str()),
        (StandalonePlatform::Windows, "x86_64-pc-windows-msvc")
            | (StandalonePlatform::Unix, "x86_64-unknown-linux-musl")
            | (StandalonePlatform::Unix, "aarch64-apple-darwin")
    );
    if !supported {
        anyhow::bail!("unsupported Codex++ standalone target: {}", metadata.target);
    }
    let pointer = shim_dir.join(".codex-plus-plus-current");
    let generation = fs::read_to_string(&pointer)
        .context("Codex++ shim generation pointer is missing")?
        .trim()
        .to_string();
    if generation.is_empty()
        || !release_dir.ends_with(&generation)
        || Path::new(&generation).components().count() != 1
    {
        anyhow::bail!("Codex++ shim does not identify the running generation");
    }
    let binary = release_dir.join(entrypoint);
    let binary_sha256 =
        file_sha256(&binary).context("failed to digest the preserved Codex++ generation")?;
    let shim_path = shim_dir.join(match platform {
        StandalonePlatform::Unix => "codex",
        StandalonePlatform::Windows => "codex.cmd",
    });
    let shim_metadata = fs::symlink_metadata(&shim_path)
        .with_context(|| format!("Codex++ shim is missing: {}", shim_path.display()))?;
    if !shim_metadata.file_type().is_file() {
        anyhow::bail!(
            "Codex++ shim is not a regular file: {}",
            shim_path.display()
        );
    }
    let shim = FileSnapshot {
        contents: fs::read(&shim_path)?,
        permissions: shim_metadata.permissions(),
        path: shim_path,
    };
    Ok(Rollback {
        generation,
        binary,
        binary_sha256,
        pointer,
        shim,
        windows_user_path: None,
    })
}

fn asset_sha256(release: &GithubRelease, name: &str) -> anyhow::Result<()> {
    release
        .assets
        .iter()
        .find(|asset| asset.name == name)
        .and_then(|asset| asset.digest.as_deref())
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .filter(|digest| digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .with_context(|| format!("upstream release is missing SHA-256 metadata for {name}"))?;
    Ok(())
}

fn file_sha256(path: &Path) -> anyhow::Result<String> {
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}

fn read_windows_user_path() -> anyhow::Result<String> {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-c", "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); [Console]::Out.Write([Environment]::GetEnvironmentVariable('Path','User'))"])
        .output()
        .context("failed to read Windows user PATH")?;
    if !output.status.success() {
        anyhow::bail!("failed to read Windows user PATH");
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn restore_windows_user_path(path: &str) -> anyhow::Result<()> {
    let mut command = Command::new("powershell");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-c",
            "[Environment]::SetEnvironmentVariable('Path',$env:CODEX_SWITCH_USER_PATH,'User')",
        ])
        .env("CODEX_SWITCH_USER_PATH", path);
    run_quiet(command, "restore Windows user PATH")
}

fn normalized_windows_user_path(current: &str, upstream: &Path, fork: &Path) -> String {
    let upstream = upstream.to_string_lossy();
    let fork = fork.to_string_lossy();
    std::iter::once(upstream.as_ref())
        .chain(current.split(';').filter(|entry| {
            !entry.is_empty()
                && !entry.eq_ignore_ascii_case(upstream.as_ref())
                && !entry.eq_ignore_ascii_case(fork.as_ref())
        }))
        .collect::<Vec<_>>()
        .join(";")
}

fn version_base(version: &str) -> bool {
    version.split('.').count() == 3
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

#[cfg(test)]
#[path = "standalone_switch_tests.rs"]
pub(super) mod tests;
