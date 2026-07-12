use super::*;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

const ACTIVE: bool = false;
const UNUSED: bool = true;
const RESETLESS: Option<i64> = None;

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

fn finish_completed(attempt: WeeklyWindowAttempt, _now: i64) {
    let refreshed_usage = usage(UNUSED, Some(i64::MAX));
    let outcome = WeeklyWindowAttemptOutcome::Completed { refreshed_usage };
    attempt.finish(outcome, _now).unwrap();
}

fn finish_rejected(attempt: WeeklyWindowAttempt, _now: i64) {
    let error = WeeklyWindowRetryableError::Rejected;
    let outcome = WeeklyWindowAttemptOutcome::Retryable { error };
    attempt.finish(outcome, _now).unwrap();
}

#[test]
fn reset_windows_baseline_dedupe_and_recover_dropped_dispatch() {
    let (_home, store, id) = test_store(/*automation_enabled*/ true);
    assert_not_due(&store, &id, usage(ACTIVE, Some(100)), 50);
    let attempt = ready(&store, &id, usage(UNUSED, Some(200)), 60);
    assert!(matches!(
        store.begin_weekly_window_attempt(&id, usage(UNUSED, Some(200)), /*now*/ 60),
        Ok(WeeklyWindowAttemptDecision::Locked)
    ));
    drop(attempt);
    let stale_at = 60 + SUPPRESSION_SECONDS;
    assert_not_due(&store, &id, usage(UNUSED, Some(300)), stale_at);
    let status = store.weekly_window_status(&id).unwrap();
    assert_eq!(status.last_error, Some(WeeklyWindowError::Ambiguous));
    let attempt = ready(&store, &id, usage(UNUSED, Some(300)), stale_at + 1);
    finish_completed(attempt, stale_at + 1);
    assert_not_due(&store, &id, usage(UNUSED, Some(300)), stale_at + 2);
    assert_not_due(&store, &id, usage(ACTIVE, RESETLESS), stale_at + 3);
    let attempt = ready(&store, &id, usage(UNUSED, RESETLESS), stale_at + 4);
    finish_completed(attempt, stale_at + 4);
    let retry_at = stale_at + 4 + SUPPRESSION_SECONDS;
    assert_not_due(&store, &id, usage(UNUSED, Some(i64::MAX)), retry_at - 1);
    drop(ready(&store, &id, usage(UNUSED, Some(i64::MAX)), i64::MAX));
    let (_home, disabled, id) = test_store(/*automation_enabled*/ false);
    assert_not_due(&disabled, &id, usage(UNUSED, Some(1)), 1);
}

#[test]
fn retry_backoff_reuses_identity_and_caps() {
    let (home, store, id) = test_store(/*automation_enabled*/ true);
    let mut now = 10;
    for failure in 0..=MAX_FAILURE_COUNT {
        finish_rejected(ready(&store, &id, usage(UNUSED, Some(10)), now), now);
        let retry_at = store
            .weekly_window_status(&id)
            .unwrap()
            .retry_not_before
            .unwrap();
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
fn resetless_windows_rearm_after_activity_and_seven_days() {
    let (_home, store, id) = test_store(/*automation_enabled*/ true);
    finish_rejected(ready(&store, &id, usage(UNUSED, RESETLESS), 0), 0);
    assert_not_due(&store, &id, usage(ACTIVE, RESETLESS), 1);
    finish_completed(ready(&store, &id, usage(UNUSED, RESETLESS), 2), 2);
    assert_not_due(&store, &id, usage(UNUSED, RESETLESS), 3);
    assert!(matches!(
        store.begin_weekly_window_attempt(&id, usage(UNUSED, RESETLESS), 2 + SUPPRESSION_SECONDS),
        Ok(WeeklyWindowAttemptDecision::Ready(_))
    ));
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
            retry_not_before: None,
            recovery_not_before: Some(100 + SUPPRESSION_SECONDS),
        }
    );
    drop(ready(&store, &id, usage(UNUSED, RESETLESS), 101));
    std::fs::write(&path, b"{broken").unwrap();
    assert_unavailable(&store, &id, usage(UNUSED, RESETLESS), 200);
    let recovery_at = 200 + SUPPRESSION_SECONDS;
    let reset_at = recovery_at + 100;
    assert_not_due(&store, &id, usage(UNUSED, Some(reset_at)), recovery_at);
    let rolled_usage = usage(UNUSED, Some(reset_at + 100));
    let attempt = ready(&store, &id, rolled_usage, recovery_at + 1);
    assert_eq!(store.weekly_window_status(&id).unwrap().last_error, None);
    drop(attempt);

    for incompatible in [br#"{"version":2}"#.to_vec(), vec![b'x'; 4097]] {
        std::fs::write(&path, &incompatible).unwrap();
        assert_unavailable(&store, &id, usage(UNUSED, RESETLESS), 101);
        assert_eq!(std::fs::read(&path).unwrap(), incompatible);
    }
}
