use super::super::AuthDotJson;
use super::super::AuthStorageBackend;
use super::super::AutoAuthStorage;
use crate::account_lease::AuthRefreshGuard;
use tracing::warn;

pub(in crate::auth::storage) fn load(
    storage: &AutoAuthStorage,
) -> std::io::Result<Option<AuthDotJson>> {
    if let Some(auth) = storage
        .file_authority
        .load_authoritative(&storage.file_storage)?
    {
        return Ok(Some(auth));
    }

    match storage.keyring_storage.load() {
        Ok(Some(auth)) => Ok(Some(auth)),
        Ok(None) => storage.file_storage.load(),
        Err(err) => {
            warn!("failed to load CLI auth from keyring, falling back to file storage: {err}");
            storage.file_storage.load()
        }
    }
}

pub(in crate::auth::storage) fn save(
    storage: &AutoAuthStorage,
    auth: &AuthDotJson,
    guard: &AuthRefreshGuard,
) -> std::io::Result<()> {
    if storage
        .file_authority
        .save_if_authoritative(&storage.file_storage, auth)?
    {
        return Ok(());
    }

    match storage.keyring_storage.save_with_guard(auth, guard) {
        Ok(()) => Ok(()),
        Err(err) => {
            warn!("failed to save auth to keyring, falling back to file storage: {err}");
            storage
                .file_authority
                .save_fallback(&storage.file_storage, auth)
        }
    }
}

pub(in crate::auth::storage) fn delete(
    storage: &AutoAuthStorage,
    guard: &AuthRefreshGuard,
) -> std::io::Result<bool> {
    storage.keyring_storage.delete_with_guard(guard)
}
