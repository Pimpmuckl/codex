use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use super::*;

#[tokio::test(start_paused = true)]
async fn schedule_scans_on_time_and_observes_disable_until_dropped() {
    let scans = Arc::new(AtomicUsize::new(0));
    let task_scans = Arc::clone(&scans);
    let drained = Arc::new(AtomicBool::new(false));
    let task_drained = Arc::clone(&drained);
    let release = Arc::new(tokio::sync::Notify::new());
    let task_release = Arc::clone(&release);
    let (state, receiver) = watch::channel(true);
    let scheduler = WeeklyWindowScheduler {
        state,
        statuses: Arc::new(Mutex::new(HashMap::new())),
        _task: tokio::spawn(run_schedule(
            move |_control| {
                let scan = task_scans.fetch_add(1, Ordering::Relaxed);
                let release = Arc::clone(&task_release);
                let drained = Arc::clone(&task_drained);
                async move {
                    if scan == 2 {
                        release.notified().await;
                        drained.store(true, Ordering::Relaxed);
                    }
                }
            },
            receiver,
        )),
    };

    tokio::task::yield_now().await;
    assert_eq!(scans.load(Ordering::Relaxed), 1);
    tokio::time::advance(SCAN_INTERVAL).await;
    tokio::task::yield_now().await;
    assert_eq!(scans.load(Ordering::Relaxed), 2);

    let active_scan = scheduler.state.subscribe();
    scheduler.set_enabled(false);
    scheduler.set_enabled(true);
    assert!(active_scan.has_changed().unwrap());
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    assert_eq!(scans.load(Ordering::Relaxed), 3);

    drop(scheduler);
    release.notify_one();
    tokio::task::yield_now().await;
    assert!(drained.load(Ordering::Relaxed));
    tokio::time::advance(SCAN_INTERVAL).await;
    tokio::task::yield_now().await;
    assert_eq!(scans.load(Ordering::Relaxed), 3);
}

#[test]
fn scheduler_status_is_bounded() {
    let mut statuses = HashMap::new();
    for index in 0..=MAX_STATUS_ACCOUNTS {
        record_status(
            &mut statuses,
            &serde_json::from_str(&format!("\"acct_{index}\"")).expect("account id"),
            WeeklyWindowStatus::Waiting(Some(100)),
        );
    }
    assert_eq!(statuses.len(), MAX_STATUS_ACCOUNTS);
}

#[test]
fn completed_ping_records_active_usage_and_unsupported_routing_closes() {
    use WeeklyWindowPingOutcome::*;
    use WeeklyWindowUsage::*;
    let assert_completed = |outcome, usage, refreshed_usage| {
        assert_eq!(
            attempt_outcome(outcome, usage),
            WeeklyWindowAttemptOutcome::Completed { refreshed_usage }
        )
    };
    let usage = |unused| Present {
        unused,
        resets_at: Some(42),
    };
    assert_completed(Completed, usage(true), usage(true));
    assert_completed(Completed, Missing, Missing);
    assert_completed(UnsupportedConfiguration, Missing, Missing);
    assert_completed(UnsupportedRouting, Missing, Missing);
}
