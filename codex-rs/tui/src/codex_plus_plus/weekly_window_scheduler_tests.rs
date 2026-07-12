use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use super::*;

#[tokio::test(start_paused = true)]
async fn schedule_scans_immediately_and_each_interval_until_dropped() {
    let scans = Arc::new(AtomicUsize::new(0));
    let task_scans = Arc::clone(&scans);
    let model = Arc::new(RwLock::new("old-model".to_string()));
    let scheduler = WeeklyWindowScheduler {
        model: Arc::clone(&model),
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
    scheduler.set_model("new-model");
    assert_eq!(
        *model
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        "new-model"
    );

    drop(scheduler);
    tokio::time::advance(SCAN_INTERVAL).await;
    tokio::task::yield_now().await;
    assert_eq!(scans.load(Ordering::Relaxed), 2);
}
