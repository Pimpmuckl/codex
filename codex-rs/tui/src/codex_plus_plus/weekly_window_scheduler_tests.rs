use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use super::*;

#[tokio::test(start_paused = true)]
async fn schedule_scans_on_time_and_observes_disable_until_dropped() {
    let scans = Arc::new(AtomicUsize::new(0));
    let task_scans = Arc::clone(&scans);
    let (tx, receiver) = watch::channel(true);
    let scheduler = WeeklyWindowScheduler {
        tx,
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

    let active_scan = scheduler.tx.subscribe();
    scheduler.set_enabled(false);
    scheduler.set_enabled(true);
    assert!(scan_stopped(&active_scan));

    drop(scheduler);
    tokio::time::advance(SCAN_INTERVAL).await;
    tokio::task::yield_now().await;
    assert_eq!(scans.load(Ordering::Relaxed), 2);
}

#[test]
fn unsupported_routing_closes_without_retry() {
    for outcome in [
        WeeklyWindowPingOutcome::UnsupportedConfiguration,
        WeeklyWindowPingOutcome::UnsupportedRouting,
    ] {
        assert_eq!(
            attempt_outcome(outcome),
            WeeklyWindowAttemptOutcome::Completed {
                refreshed_usage: WeeklyWindowUsage::Missing,
            }
        );
    }
}
