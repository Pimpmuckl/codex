use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

fn test_store() -> (TempDir, AccountStore, AccountId) {
    let home = TempDir::new().expect("tempdir");
    let store = AccountStore::new(home.path().into());
    (home, store, AccountId("acct_test".into()))
}

fn redeeming(phase: &ResetAttemptPhase) -> (&str, &str) {
    let ResetAttemptPhase::Redeeming {
        credit_id,
        redeem_request_id,
    } = phase
    else {
        panic!("expected redeeming phase");
    };
    (credit_id, redeem_request_id)
}

#[test]
fn reset_mutation_lease_excludes_and_releases() {
    let (_home, store, account_id) = test_store();
    let lease = store
        .try_acquire_reset_mutation_lease(&account_id)
        .unwrap()
        .unwrap();
    assert!(
        store
            .try_acquire_reset_mutation_lease(&account_id)
            .unwrap()
            .is_none()
    );
    drop(lease);
    assert!(
        store
            .try_acquire_reset_mutation_lease(&account_id)
            .unwrap()
            .is_some()
    );
}

#[test]
fn ambiguous_attempt_replays_and_definite_noop_gets_a_fresh_uuid() {
    let (_home, store, account_id) = test_store();
    let lease = store.acquire_reset_mutation_lease(&account_id).unwrap();
    let first = lease.load_or_begin("credit-a").unwrap();
    let (first_credit, first_request) = redeeming(&first);
    assert_eq!(first_credit, "credit-a");
    assert_eq!(first_request.len(), 36);
    drop(lease);

    let lease = store.acquire_reset_mutation_lease(&account_id).unwrap();
    assert_eq!(lease.load_or_begin("credit-b").unwrap(), first);
    assert!(!lease.clear_redeeming("wrong-request").unwrap());
    assert!(lease.clear_redeeming(first_request).unwrap());
    let fresh = lease.load_or_begin("credit-a").unwrap();
    assert_ne!(redeeming(&fresh).1, first_request);
}

#[test]
fn confirmed_redemption_recovers_weekly_activation_and_completion() {
    let (_home, store, account_id) = test_store();
    let lease = store.acquire_reset_mutation_lease(&account_id).unwrap();
    let attempt = lease.load_or_begin("credit-a").unwrap();
    let request_id = redeeming(&attempt).1.to_string();
    assert!(!lease.confirm_redeemed("wrong-request", 100).unwrap());
    assert!(lease.confirm_redeemed(&request_id, 100).unwrap());
    let expected = ResetState {
        phase: Some(ResetAttemptPhase::ActivatingWeekly),
        completion: Some(ResetCompletion {
            id: request_id,
            completed_at: 100,
        }),
    };
    assert_eq!(lease.state().unwrap(), expected);
    drop(lease);

    let lease = store.acquire_reset_mutation_lease(&account_id).unwrap();
    assert_eq!(lease.state().unwrap(), expected);
    assert!(lease.finish_weekly_activation().unwrap());
    assert_eq!(
        lease.state().unwrap(),
        ResetState {
            phase: None,
            completion: expected.completion,
        }
    );
}

#[test]
fn bad_or_newer_state_fails_closed_without_replacement() {
    let (home, store, account_id) = test_store();
    let lease = store.acquire_reset_mutation_lease(&account_id).unwrap();
    let path = home.path().join("accounts/acct_test").join(STATE_FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();

    for invalid in [
        b"{broken".to_vec(),
        br#"{"version":2,"phase":null,"completion":null}"#.to_vec(),
        vec![b'x'; MAX_STATE_BYTES as usize + 1],
    ] {
        std::fs::write(&path, &invalid).unwrap();
        assert_eq!(
            lease.state().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            lease.load_or_begin("credit-a").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(std::fs::read(&path).unwrap(), invalid);
    }
}

#[tokio::test]
async fn auth_lease_uses_exact_identity_and_has_a_deadline() {
    let (home, store, _account_id) = test_store();
    let auth =
        CodexAuth::from_external_chatgpt_tokens("e30.e30.c2ln", "workspace-a", None).unwrap();
    let tokens = auth.get_token_data().unwrap();
    let account_id = super::super::account_id_for_token_data(&tokens).unwrap();
    let account_home = home.path().join("accounts").join(account_id.as_str());
    std::fs::create_dir_all(&account_home).unwrap();
    std::fs::write(account_home.join("auth.json"), b"{}").unwrap();
    std::fs::write(
        home.path().join("accounts/index.json"),
        serde_json::to_vec(&json!({
            "accounts": [{
                "id": account_id,
                "label": "a@example.com",
                "auth": { "scope": "file", "path": "unused" }
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let held = store.acquire_reset_mutation_lease(&account_id).unwrap();
    let other =
        CodexAuth::from_external_chatgpt_tokens("e30.e30.c2ln", "workspace-b", None).unwrap();
    assert!(
        store
            .acquire_reset_mutation_lease_for_auth(&other, Instant::now())
            .await
            .unwrap()
            .is_none()
    );
    let error = match store
        .acquire_reset_mutation_lease_for_auth(&auth, Instant::now() + Duration::from_millis(10))
        .await
    {
        Ok(_) => panic!("expected lease timeout"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    drop(held);
    let error = match store
        .acquire_reset_mutation_lease_for_auth(&auth, Instant::now())
        .await
    {
        Ok(_) => panic!("expected expired deadline"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(
        store
            .acquire_reset_mutation_lease_for_auth(&auth, Instant::now() + Duration::from_secs(1),)
            .await
            .unwrap()
            .is_some()
    );
}
