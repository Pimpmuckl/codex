use super::*;
use pretty_assertions::assert_eq;
use std::time::UNIX_EPOCH;

#[test]
fn status_distinguishes_fork_revision_updates_from_stable_minor_lag() {
    let checked_at = UNIX_EPOCH + Duration::from_secs(1_000);

    assert_eq!(
        ForkReleaseStatus::new(
            "0.144.4-fork.1".to_string(),
            Some("0.144.4-fork.2".to_string()),
            Some("0.147.1".to_string()),
            checked_at,
        ),
        ForkReleaseStatus {
            installed_fork_version: "0.144.4-fork.1".to_string(),
            latest_fork_version: Some("0.144.4-fork.2".to_string()),
            latest_stable_upstream_version: Some("0.147.1".to_string()),
            fork_upstream_base: Some("0.144.4".to_string()),
            stable_minor_lag: Some(3),
            checked_at,
            warning_state_key: Some("fork-update/0.144.4-fork.2".to_string()),
            dismissed_version: None,
        }
    );
}

#[test]
fn status_ignores_patch_churn_and_prereleases_for_lag() {
    let current = ForkReleaseStatus::new(
        "0.144.4-fork.2".to_string(),
        Some("0.144.4-fork.2".to_string()),
        Some("0.147.9".to_string()),
        UNIX_EPOCH,
    );
    let prerelease = ForkReleaseStatus::new(
        "0.144.4-fork.2".to_string(),
        Some("0.144.9-fork.7".to_string()),
        Some("0.148.0-beta.1".to_string()),
        UNIX_EPOCH,
    );

    assert_eq!(current.stable_minor_lag, Some(3));
    assert_eq!(
        current.warning_state_key.as_deref(),
        Some("upstream-lag/0.144/0.147")
    );
    assert_eq!(
        (prerelease.fork_upstream_base, prerelease.stable_minor_lag),
        (Some("0.144.9".to_string()), None)
    );
}
