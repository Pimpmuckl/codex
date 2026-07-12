use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use super::*;

#[tokio::test(start_paused = true)]
async fn schedule_scans_on_time_and_observes_disable_until_dropped() {
    let scans = Arc::new(AtomicUsize::new(0));
    let task_scans = Arc::clone(&scans);
    let (state, receiver) = watch::channel(true);
    let scheduler = WeeklyWindowScheduler {
        state,
        task: tokio::spawn(run_schedule(
            move |_control| {
                task_scans.fetch_add(1, Ordering::Relaxed);
                std::future::ready(())
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

    drop(scheduler);
    tokio::time::advance(SCAN_INTERVAL).await;
    tokio::task::yield_now().await;
    assert_eq!(scans.load(Ordering::Relaxed), 2);
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
    assert_completed(Completed, usage(true), usage(false));
    assert_completed(UnsupportedConfiguration, Missing, Missing);
    assert_completed(UnsupportedRouting, Missing, Missing);
}
