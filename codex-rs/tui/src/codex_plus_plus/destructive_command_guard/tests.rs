use super::*;
use crate::legacy_core::config::ConfigBuilder;
use color_eyre::Result;
use pretty_assertions::assert_eq;
use std::fs;
use tempfile::TempDir;

#[tokio::test]
async fn managed_install_lifecycle_is_transactional_and_next_session_only() -> Result<()> {
    if !PLATFORM_SUPPORTED {
        return Ok(());
    }
    let home = TempDir::new()?;
    let source = TempDir::new()?;
    fs::write(
        home.path().join("config.toml"),
        "[features]\nplugins = true\n",
    )?;
    write_marketplace(source.path())?;
    compile_fake_dcg(source.path())?;
    write_installer(source.path(), /*fail*/ true)?;

    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .build()
        .await?;
    let app_server = crate::start_embedded_app_server_for_picker(&config).await?;
    let mut manager = DcgManager::new(&app_server, &config).unwrap();
    manager.marketplace_source = fs::canonicalize(source.path())?.display().to_string();
    manager.marketplace_ref = None;
    assert!(manager.install_and_enable().await.is_err());
    assert!(fs::read_to_string(home.path().join("config.toml"))?.contains("enabled = false"));
    write_installer(source.path(), /*fail*/ false)?;
    let installed = manager.install_and_enable().await.unwrap();
    let enabled_status = DcgStatus::Enabled(PINNED_VERSION.to_string());
    assert_eq!(installed.status, enabled_status);
    assert!(!installed.takes_effect_in_current_session);
    let expected_log = format!(
        "Pimpmuckl|destructive_command_guard|{PINNED_TAG}|{}|True|True",
        manager.binary_path().parent().unwrap().display()
    );
    assert_eq!(
        fs::read_to_string(source.path().join("installer-args.txt"))?,
        expected_log
    );
    let disabled = manager.disable().await.unwrap();
    let disabled_status = DcgStatus::Disabled(PINNED_VERSION.to_string());
    assert_eq!(disabled.status, disabled_status);
    assert_eq!(manager.enable().await.unwrap().status, installed.status);
    write_installer(source.path(), /*fail*/ true)?;
    assert!(manager.update().await.is_err());
    assert_eq!(manager.detect_status().await, installed.status);

    let config_before_remote_attempt = fs::read_to_string(home.path().join("config.toml"))?;
    manager.remote_hook_host = true;
    let status = manager.detect_status().await;
    let unsupported = DcgStatus::Unsupported(DcgUnsupportedReason::RemoteHookHost);
    assert_eq!(status, unsupported);
    assert!(manager.disable().await.is_err());
    let config_after_remote_attempt = fs::read_to_string(home.path().join("config.toml"))?;
    assert_eq!(config_after_remote_attempt, config_before_remote_attempt);
    app_server.shutdown().await?;
    Ok(())
}

fn write_marketplace(root: &Path) -> Result<()> {
    let plugin = root.join("plugins").join(PLUGIN_NAME);
    fs::create_dir_all(root.join(".agents/plugins"))?;
    fs::create_dir_all(plugin.join(".codex-plugin"))?;
    fs::create_dir_all(plugin.join("hooks"))?;
    fs::write(
        root.join(".agents/plugins/marketplace.json"),
        format!(
            r#"{{"name":"{MARKETPLACE_NAME}","plugins":[{{"name":"{PLUGIN_NAME}","source":{{"source":"local","path":"./plugins/{PLUGIN_NAME}"}}}}]}}"#
        ),
    )?;
    fs::write(
        plugin.join(".codex-plugin/plugin.json"),
        format!(r#"{{"name":"{PLUGIN_NAME}","version":"{PINNED_VERSION}","description":"test"}}"#),
    )?;
    fs::write(
        plugin.join("hooks/hooks.json"),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"\"${PLUGIN_DATA}/dcg\"","commandWindows":"\"${PLUGIN_DATA}\\dcg.exe\""}]}]}}"#,
    )?;
    Ok(())
}

fn compile_fake_dcg(root: &Path) -> Result<()> {
    let source = root.join("fake-dcg.rs");
    fs::write(
        &source,
        format!(r#"fn main() {{ println!("dcg {PINNED_VERSION}"); }}"#),
    )?;
    let status = std::process::Command::new("rustc")
        .arg(source)
        .args(["-o"])
        .arg(root.join(format!("fake-dcg{}", std::env::consts::EXE_SUFFIX)))
        .status()?;
    assert!(status.success());
    Ok(())
}

#[cfg(target_os = "windows")]
fn write_installer(root: &Path, fail: bool) -> Result<()> {
    let body = if fail {
        r#"Param([string]$Owner,[string]$Repo,[string]$Version,[string]$Dest,[switch]$NoConfigure,[switch]$Verify); [IO.File]::WriteAllText((Join-Path $Dest 'dcg.exe'), 'broken'); exit 7"#
    } else {
        r#"Param([string]$Owner,[string]$Repo,[string]$Version,[string]$Dest,[switch]$NoConfigure,[switch]$Verify); Copy-Item (Join-Path $PSScriptRoot 'fake-dcg.exe') (Join-Path $Dest 'dcg.exe') -Force; [IO.File]::WriteAllText((Join-Path $PSScriptRoot 'installer-args.txt'), "$Owner|$Repo|$Version|$Dest|$NoConfigure|$Verify")"#
    };
    fs::write(root.join("install.ps1"), body)?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn write_installer(root: &Path, fail: bool) -> Result<()> {
    let body = if fail {
        r#"#!/usr/bin/env bash
set -eu; while (($#)); do if [[ $1 == --dest ]]; then dest=$2; shift; fi; shift; done; printf broken > "$dest/dcg"; exit 7"#
    } else {
        r#"#!/usr/bin/env bash
set -eu; while (($#)); do case $1 in --version) version=$2; shift;; --dest) dest=$2; shift;; esac; shift; done; cp "$(dirname "$0")/fake-dcg" "$dest/dcg"; chmod +x "$dest/dcg"; printf '%s|%s|%s|%s|True|True' "$OWNER" "$REPO" "$version" "$dest" > "$(dirname "$0")/installer-args.txt""#
    };
    fs::write(root.join("install.sh"), body)?;
    Ok(())
}
