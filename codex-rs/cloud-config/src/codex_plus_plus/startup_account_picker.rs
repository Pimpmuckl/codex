//! Cloud configuration handoff before the Codex++ startup account picker.

use crate::bundle_loader::take_refresher_task;

/// Stops the bootstrap refresher so its account lease does not look like another session.
pub async fn stop_cloud_config_refresh_before_account_picker() {
    let Some(task) = take_refresher_task() else {
        return;
    };
    task.abort();
    let _ = task.await;
}
