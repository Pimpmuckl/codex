use std::fs;

use codex_config::types::AutomaticAccountSelection;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use tempfile::tempdir;
use tokio::sync::mpsc::unbounded_channel;

use super::*;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::ListSelectionView;
use crate::keymap::RuntimeKeymap;
use crate::legacy_core::config::ConfigBuilder;
use crate::test_backend::VT100Backend;

async fn test_config(contents: &str) -> (tempfile::TempDir, Config) {
    let codex_home = tempdir().expect("tempdir");
    fs::write(codex_home.path().join("config.toml"), contents).expect("write config");
    let config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .build()
        .await
        .expect("config");
    (codex_home, config)
}

fn render_settings(config: Config, current: AutomaticAccountSelection) -> String {
    let (tx, _rx) = unbounded_channel();
    let view = ListSelectionView::new(
        account_settings_params(current, config.codex_home.join("config.toml").to_path_buf()),
        AppEventSender::new(tx),
        RuntimeKeymap::defaults().list,
    );
    let mut terminal =
        Terminal::new(VT100Backend::new(/*width*/ 84, /*height*/ 12)).expect("terminal");
    terminal
        .draw(|frame| view.render(frame.area(), frame.buffer_mut()))
        .expect("render account settings");
    terminal.backend().to_string()
}

#[tokio::test]
async fn account_settings_enabled_snapshot() {
    let (_codex_home, config) = test_config("").await;
    insta::assert_snapshot!(
        "account_settings_enabled",
        render_settings(config, AutomaticAccountSelection::Enabled)
    );
}

#[tokio::test]
async fn account_settings_disabled_snapshot() {
    let (_codex_home, config) = test_config("automatic_account_selection = \"disabled\"\n").await;
    insta::assert_snapshot!(
        "account_settings_disabled",
        render_settings(config, AutomaticAccountSelection::Disabled)
    );
}

#[test]
fn account_settings_toggle_success_snapshot() {
    let success = persistence_success_message(AutomaticAccountSelection::Disabled);
    insta::assert_snapshot!("account_settings_toggle_success", success);
}

#[test]
fn account_settings_toggle_failure_snapshot() {
    let error = persistence_error_message("permission denied".to_string());
    insta::assert_snapshot!("account_settings_toggle_failure", error);
}

#[tokio::test]
async fn persistence_preserves_unrelated_config() {
    let codex_home = tempdir().expect("tempdir");
    fs::write(codex_home.path().join("config.toml"), "model = \"gpt-5\"\n").expect("write config");

    persist_automatic_account_selection(
        &codex_home.path().join("config.toml"),
        AutomaticAccountSelection::Disabled,
    )
    .await
    .expect("persist setting");

    assert_eq!(
        fs::read_to_string(codex_home.path().join("config.toml")).expect("read config"),
        "model = \"gpt-5\"\nautomatic_account_selection = \"disabled\"\n"
    );
}

#[tokio::test]
async fn persistence_failure_is_returned() {
    let codex_home = tempdir().expect("tempdir");
    fs::create_dir(codex_home.path().join("config.toml")).expect("replace config with directory");

    let result = persist_automatic_account_selection(
        &codex_home.path().join("config.toml"),
        AutomaticAccountSelection::Disabled,
    )
    .await;

    assert!(result.is_err());
}
