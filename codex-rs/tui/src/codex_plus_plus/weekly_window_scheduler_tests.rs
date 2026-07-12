use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use super::*;

#[tokio::test(start_paused = true)]
async fn schedule_scans_immediately_and_each_interval_until_dropped() {
    let scans = Arc::new(AtomicUsize::new(0));
    let task_scans = Arc::clone(&scans);
    let scheduler = WeeklyWindowScheduler {
        task: tokio::spawn(run_schedule(move || {
            task_scans.fetch_add(1, Ordering::Relaxed);
            std::future::ready(())
        })),
    };

    tokio::task::yield_now().await;
    assert_eq!(scans.load(Ordering::Relaxed), 1);
    tokio::time::advance(SCAN_INTERVAL).await;
    tokio::task::yield_now().await;
    assert_eq!(scans.load(Ordering::Relaxed), 2);

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
