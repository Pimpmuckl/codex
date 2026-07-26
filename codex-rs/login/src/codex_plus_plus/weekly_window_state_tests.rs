use super::*;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

const ACTIVE: bool = false;
const UNUSED: bool = true;

fn test_store(automation_enabled: bool) -> (TempDir, AccountStore, AccountId) {
    let home = TempDir::new().expect("tempdir");
    let id = AccountId("acct_test".into());
    let accounts = home.path().join("accounts");
    std::fs::create_dir(&accounts).expect("accounts dir");
    let index = format!(
        r#"{{"accounts":[{{"id":"acct_test","label":"test@example.com","enabled":true,"automation_enabled":{automation_enabled},"auth":{{"scope":"file","path":"accounts/acct_test/auth.json"}}}}]}}"#
    );
    std::fs::write(accounts.join("index.json"), index).expect("index");
    let store = AccountStore::new(home.path().into());
    (home, store, id)
}

fn usage(unused: bool, resets_at: Option<i64>) -> WeeklyWindowUsage {
    WeeklyWindowUsage::Present { unused, resets_at }
}

fn ready(s: &AccountStore, id: &AccountId, u: WeeklyWindowUsage, _at: i64) -> WeeklyWindowAttempt {
    let WeeklyWindowAttemptDecision::Ready(attempt) =
        s.begin_weekly_window_attempt(id, u, _at).expect("attempt")
    else {
        panic!("attempt should be ready");
    };
    attempt
}

fn assert_not_due(store: &AccountStore, id: &AccountId, usage: WeeklyWindowUsage, _now: i64) {
    assert!(matches!(
        store.begin_weekly_window_attempt(id, usage, _now),
        Ok(WeeklyWindowAttemptDecision::NotDue)
    ));
}

fn assert_unavailable(store: &AccountStore, id: &AccountId, usage: WeeklyWindowUsage, _now: i64) {
    assert!(matches!(
        store.begin_weekly_window_attempt(id, usage, _now),
        Ok(WeeklyWindowAttemptDecision::StateUnavailable)
    ));
}

fn finish_rejected(attempt: WeeklyWindowAttempt, _now: i64) {
    let error = WeeklyWindowRetryableError::Rejected { status: Some(400) };
    let outcome = WeeklyWindowAttemptOutcome::Retryable { error };
    attempt.finish(outcome, _now).unwrap();
}

#[test]
fn dated_unused_window_requires_outward_movement_and_recovers_dropped_dispatch() {
    let (_home, store, id) = test_store(/*automation_enabled*/ true);
    assert_not_due(&store, &id, usage(ACTIVE, Some(100)), 50);
    assert_not_due(&store, &id, usage(UNUSED, Some(200)), 60);
    assert_not_due(&store, &id, usage(UNUSED, Some(200)), 61);
    let attempt = ready(&store, &id, usage(UNUSED, Some(201)), 62);
    assert!(matches!(
        store.begin_weekly_window_attempt(&id, usage(UNUSED, Some(201)), /*now*/ 62),
        Ok(WeeklyWindowAttemptDecision::Locked)
    ));
    drop(attempt);
    assert_not_due(&store, &id, usage(UNUSED, Some(202)), 63);
    let status = store.weekly_window_status(&id).unwrap();
    assert_eq!(status.last_error, Some(WeeklyWindowError::Ambiguous));
    assert_not_due(&store, &id, usage(UNUSED, Some(202)), 64);
    let attempt = ready(&store, &id, usage(UNUSED, Some(203)), 65);
    attempt
        .finish(
            WeeklyWindowAttemptOutcome::Completed {
                refreshed_usage: usage(ACTIVE, Some(203)),
            },
            /*now*/ 66,
        )
        .unwrap();
    assert_not_due(&store, &id, usage(UNUSED, Some(300)), 67);
    assert_not_due(&store, &id, usage(UNUSED, Some(300)), 68);
    drop(ready(&store, &id, usage(UNUSED, Some(301)), 69));

    let (_home, store, id) = test_store(/*automation_enabled*/ true);
    assert_not_due(&store, &id, usage(UNUSED, Some(100)), 100);
    drop(ready(&store, &id, usage(UNUSED, Some(101)), 101));
    assert_not_due(&store, &id, usage(ACTIVE, None), 102);
    assert_not_due(&store, &id, usage(UNUSED, Some(200)), 103);
    assert_not_due(&store, &id, usage(UNUSED, Some(200)), 104);
    drop(ready(&store, &id, usage(UNUSED, Some(201)), 105));
}

#[test]
fn automatic_selection_flag_does_not_gate_weekly_window_attempts() {
    let (_home, store, id) = test_store(/*automation_enabled*/ false);
    assert_not_due(&store, &id, usage(UNUSED, Some(1)), 1);
    drop(ready(&store, &id, usage(UNUSED, Some(2)), 2));
}

#[test]
fn weekly_scan_lease_is_nonblocking_and_released_on_drop() {
    let (_home, store, _id) = test_store(/*automation_enabled*/ true);
    let lease = store.try_acquire_weekly_window_scan().unwrap().unwrap();
    assert!(store.try_acquire_weekly_window_scan().unwrap().is_none());
    drop(lease);
    assert!(store.try_acquire_weekly_window_scan().unwrap().is_some());
}

#[test]
fn retry_backoff_reuses_identity_and_caps() {
    let (home, store, id) = test_store(/*automation_enabled*/ true);
    assert_not_due(&store, &id, usage(UNUSED, Some(9)), 9);
    let mut now = 10;
    for failure in 0..=MAX_FAILURE_COUNT {
        finish_rejected(ready(&store, &id, usage(UNUSED, Some(10)), now), now);
        let retry_at = store
            .weekly_window_status(&id)
            .unwrap()
            .retry_not_before
            .unwrap();
        assert_eq!(
            store.weekly_window_status(&id).unwrap().last_http_status,
            Some(400)
        );
        assert_eq!(
            retry_at - now,
            (300_i64 * (1_i64 << failure.min(MAX_FAILURE_COUNT))).min(6 * 60 * 60)
        );
        assert_not_due(&store, &id, usage(UNUSED, Some(10)), retry_at - 1);
        now = retry_at;
    }
    let path = home.path().join("accounts/acct_test").join(STATE_FILE);
    let StateRead::Ready(state) = read_state(&path).unwrap() else {
        panic!("valid state");
    };
    assert_eq!(state.failure_count, MAX_FAILURE_COUNT);
    assert!(std::fs::metadata(path).unwrap().len() <= MAX_STATE_BYTES);
}

#[test]
fn retry_tracks_reset_drift_and_resetless_activity() {
    let (_home, store, id) = test_store(/*automation_enabled*/ true);
    assert_not_due(&store, &id, usage(UNUSED, Some(9)), 9);
    finish_rejected(ready(&store, &id, usage(UNUSED, Some(10)), 10), 10);
    assert_not_due(&store, &id, usage(UNUSED, Some(12)), 309);
    assert_not_due(&store, &id, usage(UNUSED, Some(11)), 310);
    finish_rejected(ready(&store, &id, usage(UNUSED, Some(12)), 311), 311);
    assert_eq!(
        store.weekly_window_status(&id).unwrap().retry_not_before,
        Some(911)
    );
    ready(&store, &id, usage(UNUSED, Some(12)), 911)
        .finish(
            WeeklyWindowAttemptOutcome::Completed {
                refreshed_usage: WeeklyWindowUsage::Missing,
            },
            /*now*/ 911,
        )
        .unwrap();
    assert_not_due(&store, &id, usage(UNUSED, Some(12)), 912);
    drop(ready(&store, &id, usage(UNUSED, Some(13)), 913));

    let (_home, store, id) = test_store(/*automation_enabled*/ true);
    assert_not_due(&store, &id, usage(UNUSED, Some(9)), 9);
    finish_rejected(ready(&store, &id, usage(UNUSED, Some(10)), 10), 10);
    assert_not_due(&store, &id, usage(ACTIVE, None), 11);
    assert_not_due(&store, &id, usage(UNUSED, Some(11)), 12);
    drop(ready(&store, &id, usage(UNUSED, Some(12)), 13));

    let (_home, store, id) = test_store(/*automation_enabled*/ true);
    assert_not_due(&store, &id, usage(UNUSED, Some(9)), 9);
    finish_rejected(ready(&store, &id, usage(UNUSED, Some(10)), 10), 10);
    assert_not_due(&store, &id, usage(UNUSED, Some(8)), 310);
    drop(ready(&store, &id, usage(UNUSED, Some(10)), 311));
}

#[test]
fn reset_regression_does_not_reopen_a_closed_identity() {
    let (_home, store, id) = test_store(/*automation_enabled*/ true);
    assert_not_due(&store, &id, usage(UNUSED, Some(10)), 10);
    ready(&store, &id, usage(UNUSED, Some(11)), 11)
        .finish(
            WeeklyWindowAttemptOutcome::Completed {
                refreshed_usage: WeeklyWindowUsage::Missing,
            },
            /*now*/ 11,
        )
        .unwrap();
    assert_not_due(&store, &id, usage(UNUSED, Some(9)), 12);
    assert_not_due(&store, &id, usage(UNUSED, Some(11)), 13);
    drop(ready(&store, &id, usage(UNUSED, Some(12)), 14));
}

#[test]
fn legacy_retry_state_is_rebaselined() {
    let (home, store, id) = test_store(/*automation_enabled*/ true);
    let path = home.path().join("accounts/acct_test").join(STATE_FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let state = State {
        version: 1,
        attempt_identity: Some(AttemptIdentity::ResetAt(10)),
        attempt_status: Some(AttemptStatus::Retryable),
        retry_not_before: Some(0),
        ..State::default()
    };
    std::fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();

    assert_not_due(&store, &id, usage(UNUSED, Some(10)), 10);
    assert_not_due(&store, &id, usage(UNUSED, Some(10)), 11);
    drop(ready(&store, &id, usage(UNUSED, Some(11)), 12));
}

#[test]
fn bad_state_is_quarantined_or_preserved_without_credentials() {
    let (home, store, id) = test_store(/*automation_enabled*/ true);
    let path = home.path().join("accounts/acct_test").join(STATE_FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"{broken").unwrap();
    assert_unavailable(&store, &id, usage(ACTIVE, Some(100)), 100);
    assert_eq!(
        store.weekly_window_status(&id).unwrap(),
        WeeklyWindowStatus {
            last_error: Some(WeeklyWindowError::StateQuarantined),
            last_http_status: None,
            retry_not_before: None,
            recovery_not_before: None,
        }
    );
    assert_not_due(&store, &id, usage(UNUSED, Some(200)), 101);
    assert_not_due(&store, &id, usage(UNUSED, Some(200)), 102);
    let attempt = ready(&store, &id, usage(UNUSED, Some(201)), 103);
    assert_eq!(store.weekly_window_status(&id).unwrap().last_error, None);
    drop(attempt);

    for incompatible in [br#"{"version":3}"#.to_vec(), vec![b'x'; 4097]] {
        std::fs::write(&path, &incompatible).unwrap();
        assert_unavailable(&store, &id, usage(UNUSED, None), 104);
        assert_eq!(std::fs::read(&path).unwrap(), incompatible);
    }
}
