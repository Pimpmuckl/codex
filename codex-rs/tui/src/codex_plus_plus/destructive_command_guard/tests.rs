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
    let (home, source) = (TempDir::new()?, TempDir::new()?);
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
    assert!(!installed.takes_effect_in_current_session);
    manager.enable().await.unwrap();
    write_installer(source.path(), /*fail*/ true)?;
    assert!(manager.update().await.is_err());
    assert_eq!(manager.detect_status().await, installed.status);
    let config_before_remote_attempt = fs::read_to_string(home.path().join("config.toml"))?;
    manager.remote_hook_host = true;
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
        r#"Param([string]$Owner,[string]$Repo,[string]$Version,[string]$Dest,[switch]$NoConfigure,[switch]$Verify); if($Owner -ne 'Pimpmuckl' -or $Repo -ne 'destructive_command_guard' -or $Version -ne 'v0.6.8-codexpp.1' -or -not $NoConfigure -or -not $Verify){exit 8}; Copy-Item (Join-Path $PSScriptRoot 'fake-dcg.exe') (Join-Path $Dest 'dcg.exe') -Force"#
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
set -eu; no_configure=0; verify=0; while (($#)); do case $1 in --version) version=$2; shift;; --dest) dest=$2; shift;; --no-configure) no_configure=1;; --verify) verify=1;; esac; shift; done; [[ $OWNER == Pimpmuckl && $REPO == destructive_command_guard && $version == v0.6.8-codexpp.1 && $no_configure == 1 && $verify == 1 ]]; cp "$(dirname "$0")/fake-dcg" "$dest/dcg"; chmod +x "$dest/dcg""#
    };
    fs::write(root.join("install.sh"), body)?;
    Ok(())
}
