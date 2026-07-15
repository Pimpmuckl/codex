use super::*;
use codex_install_context::codex_plus_plus::FORK_RELEASE_STATUS_MAX_AGE;
use pretty_assertions::assert_eq;
use std::time::Duration;
use std::time::UNIX_EPOCH;
use tempfile::tempdir;

#[tokio::test]
async fn cache_round_trips_fresh_and_stale_status() {
    let codex_home = tempdir().expect("temp codex home");
    let path = release_status_filepath(codex_home.path());
    let checked_at = UNIX_EPOCH + Duration::from_secs(10_000);
    let mut expected = ForkReleaseStatus::new(
        "0.144.4-fork.2".to_string(),
        Some("0.144.4-fork.2".to_string()),
        Some("0.147.1".to_string()),
        checked_at,
    );
    expected.dismissed_version = Some("0.144.4-fork.1".to_string());

    write_release_status(&path, &expected)
        .await
        .expect("write release status");
    let actual = read_release_status(&path).expect("read release status");

    assert_eq!(actual, expected);
    assert!(!actual.is_stale(
        checked_at + FORK_RELEASE_STATUS_MAX_AGE,
        FORK_RELEASE_STATUS_MAX_AGE
    ));
    assert!(actual.is_stale(
        checked_at + FORK_RELEASE_STATUS_MAX_AGE + Duration::from_secs(1),
        FORK_RELEASE_STATUS_MAX_AGE
    ));
}

#[tokio::test]
async fn refresh_recovers_corrupt_cache_and_offline_preserves_it() {
    let codex_home = tempdir().expect("temp codex home");
    let path = release_status_filepath(codex_home.path());
    tokio::fs::create_dir_all(path.parent().expect("cache parent"))
        .await
        .expect("create cache parent");
    tokio::fs::write(&path, "not json")
        .await
        .expect("write corrupt cache");

    probe::refresh_with_probes(
        &path,
        "0.144.4-fork.1",
        async { Ok("0.144.4-fork.2".to_string()) },
        async { Ok("0.147.0".to_string()) },
    )
    .await
    .expect("repair cache");
    let repaired = read_release_status(&path).expect("read repaired cache");

    let error = probe::refresh_with_probes(
        &path,
        "0.144.4-fork.1",
        async { Err(anyhow::anyhow!("offline")) },
        async { Ok("0.148.0".to_string()) },
    )
    .await
    .expect_err("offline refresh should fail");

    assert_eq!(error.to_string(), "offline");
    assert_eq!(
        read_release_status(&path).expect("read stale cache"),
        repaired
    );
}
