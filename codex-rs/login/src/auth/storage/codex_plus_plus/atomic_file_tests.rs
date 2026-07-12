use super::*;
use pretty_assertions::assert_eq;
use tempfile::tempdir;
fn auth(key: &str) -> AuthDotJson {
    serde_json::from_value(serde_json::json!({"OPENAI_API_KEY": key})).expect("valid auth")
}
#[test]
fn save_recovers_at_every_durable_boundary() -> anyhow::Result<()> {
    for (point, expected) in [
        (FaultPoint::BeforePendingSync, auth("new")),
        (FaultPoint::AfterPendingSync, auth("new")),
        (FaultPoint::BeforeReplace, auth("new")),
        (FaultPoint::AfterReplace, auth("new")),
        (FaultPoint::AfterReplaceDirectorySync, auth("new")),
    ] {
        let home = tempdir()?;
        let storage = AtomicFileStorage::new(home.path().to_path_buf());
        std::fs::write(home.path().join(format!("{PENDING_PREFIX}noise")), b"noise")?;
        storage.save(&auth("old"))?;
        let oversized = auth(&"x".repeat(MAX_AUTH_BYTES as usize));
        assert!(storage.save(&oversized).is_err());
        storage.fail_once(point);
        assert!(storage.save(&auth("new")).is_err(), "{point:?}");
        assert_eq!(storage.load()?, Some(expected), "{point:?}");
        assert!(artifacts(home.path())?.is_empty());
    }
    let home = tempdir()?;
    let oversized = vec![b'x'; MAX_AUTH_BYTES as usize + 1];
    std::fs::write(home.path().join("auth.json"), oversized)?;
    let storage = AtomicFileStorage::new(home.path().to_path_buf());
    storage.save(&auth("new"))?;
    assert_eq!(storage.load()?, Some(auth("new")));
    let home = tempdir()?;
    let storage = AtomicFileStorage::new(home.path().to_path_buf());
    storage.save(&auth("old"))?;
    storage.fail_once(FaultPoint::BeforeReplace);
    assert!(storage.save(&auth("new")).is_err());
    let drift = serde_json::to_vec_pretty(&auth("drift"))?;
    std::fs::write(home.path().join("auth.json"), drift)?;
    let error = storage.load().unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    for bytes in [
        b"not json".to_vec(),
        vec![b'x'; MAX_AUTH_BYTES as usize + 1],
    ] {
        let home = tempdir()?;
        let storage = AtomicFileStorage::new(home.path().to_path_buf());
        let new = fingerprint(&auth("new"))?;
        let new = short_fingerprint(&new);
        let pending = home
            .path()
            .join(format!("{PENDING_PREFIX}{NONE_FINGERPRINT}-{new}"));
        std::fs::write(pending, bytes)?;
        assert_eq!(storage.load()?, None);
        assert!(artifacts(home.path())?.is_empty());
    }
    Ok(())
}
