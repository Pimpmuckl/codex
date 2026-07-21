use super::*;
use crate::AuthKeyringBackendKind;
use crate::account::tests::import_test_account;
use crate::account::tests::test_auth_manager;
use codex_config::types::AuthCredentialsStoreMode;
use pretty_assertions::assert_eq;
use std::sync::mpsc;
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn legacy_defaults_and_reimport_preserves_automation_policy() {
    let codex_home = tempdir().expect("tempdir");
    let store = AccountStore::new(codex_home.path().to_path_buf());
    let profile = import_test_account(&store, codex_home.path(), "first", "account-a");
    let index_path = codex_home.path().join("accounts/index.json");
    let mut index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&index_path).expect("read index"))
            .expect("parse index");
    index["accounts"][0]
        .as_object_mut()
        .expect("profile object")
        .remove("automation_enabled");
    std::fs::write(
        &index_path,
        serde_json::to_vec_pretty(&index).expect("serialize index"),
    )
    .expect("write legacy index");

    assert!(store.list().expect("list")[0].automation_enabled);
    store
        .set_automation_enabled(&profile.id, /*automation_enabled*/ false)
        .expect("disable automation");
    let reimported = store
        .import_current(
            Some("first again".to_string()),
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
        )
        .expect("reimport account");
    assert!(!reimported.automation_enabled);
}

#[test]
fn disabling_waits_for_an_active_reset_mutation() {
    let codex_home = tempdir().expect("tempdir");
    let store = AccountStore::new(codex_home.path().to_path_buf());
    let first = import_test_account(&store, codex_home.path(), "first", "account-a");
    let second = import_test_account(&store, codex_home.path(), "second", "account-b");
    let lease = store.acquire_reset_mutation_lease(&first.id).unwrap();
    let (tx, rx) = mpsc::channel();
    let worker_store = store.clone();
    let worker_ids = [first.id, second.id];
    let worker =
        std::thread::spawn(move || {
            tx.send(worker_store.set_automation_enabled_batch(
                worker_ids.iter().map(|account_id| (account_id, false)),
            ))
            .unwrap();
        });
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while store.list().unwrap()[0].automation_enabled {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        store
            .list()
            .unwrap()
            .iter()
            .all(|profile| !profile.automation_enabled)
    );
    assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
    drop(lease);
    assert!(rx.recv_timeout(Duration::from_secs(1)).unwrap().unwrap());
    worker.join().unwrap();
}

#[tokio::test]
async fn startup_resumes_disabled_current_but_skips_it_for_alternatives() {
    let codex_home = tempdir().expect("tempdir");
    let store = AccountStore::new(codex_home.path().to_path_buf());
    let first = import_test_account(&store, codex_home.path(), "first", "account-a");
    let second = import_test_account(&store, codex_home.path(), "second", "account-b");
    store
        .set_automation_enabled(&first.id, /*automation_enabled*/ false)
        .expect("disable first automation");
    store
        .apply_imported_account_to_root_auth(
            &first.id,
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
        )
        .expect("select first account");

    let manager = test_auth_manager(codex_home.path()).await;
    assert_eq!(manager.active_account_id(), Some(first.id.clone()));
    drop(manager);
    crate::logout(
        codex_home.path(),
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )
    .expect("clear current marker");
    let restarted = test_auth_manager(codex_home.path()).await;
    assert_eq!(restarted.active_account_id(), Some(second.id));
    restarted
        .activate_imported_account(&first.id)
        .await
        .expect("activate first manually");
    assert_eq!(restarted.active_account_id(), Some(first.id));
}
