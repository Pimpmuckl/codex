use std::fs::OpenOptions;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;

use super::super::AuthDotJson;
use super::super::AuthStorageBackend;
use super::super::FileAuthStorage;
use crate::account_lease::AuthRefreshGuard;

#[derive(Clone, Debug)]
pub(in super::super) struct FileAuthorityMarker {
    path: PathBuf,
}

impl FileAuthorityMarker {
    pub(in super::super) fn new(codex_home: &Path) -> Self {
        Self {
            path: codex_home.join(".codex-plus-plus-auth-file-authority"),
        }
    }

    pub(in super::super) fn is_active(&self) -> io::Result<bool> {
        match self.path.metadata() {
            Ok(metadata) if metadata.is_file() => Ok(true),
            Ok(_) => Err(io::Error::other(
                "auth file-authority marker is not a regular file",
            )),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err),
        }
    }

    pub(in super::super) fn load_authoritative(
        &self,
        file_storage: &FileAuthStorage,
        guard: &AuthRefreshGuard,
    ) -> io::Result<Option<AuthDotJson>> {
        if !self.is_active()? {
            return Ok(None);
        }
        let auth = file_storage.load_with_guard(guard)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "authoritative auth file is missing",
            )
        })?;
        Ok(Some(auth))
    }

    pub(in super::super) fn save_if_authoritative(
        &self,
        file_storage: &FileAuthStorage,
        auth: &AuthDotJson,
        guard: &AuthRefreshGuard,
    ) -> io::Result<bool> {
        if !self.is_active()? {
            return Ok(false);
        }
        file_storage.save_with_guard(auth, guard)?;
        Ok(true)
    }

    pub(in super::super) fn save_fallback(
        &self,
        file_storage: &FileAuthStorage,
        auth: &AuthDotJson,
        guard: &AuthRefreshGuard,
    ) -> io::Result<()> {
        self.activate()?;
        file_storage.save_with_guard(auth, guard)
    }

    pub(in super::super) fn prepare_keyring_save(
        &self,
        auth: &AuthDotJson,
        guard: &AuthRefreshGuard,
    ) -> io::Result<()> {
        if self.is_active()? {
            let codex_home = self.path.parent().ok_or(io::ErrorKind::InvalidInput)?;
            FileAuthStorage::new(codex_home.to_path_buf()).save_with_guard(auth, guard)?;
        }
        Ok(())
    }

    pub(in super::super) fn activate(&self) -> io::Result<()> {
        let parent = self.path.parent().ok_or(io::ErrorKind::InvalidInput)?;
        std::fs::create_dir_all(parent)?;
        let mut options = OpenOptions::new();
        options.create(true).write(true).truncate(false);
        #[cfg(unix)]
        options.mode(0o600);
        options.open(&self.path)?.sync_all()?;
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    }

    pub(in super::super) fn clear(&self) -> io::Result<bool> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => {
                #[cfg(unix)]
                std::fs::File::open(self.path.parent().ok_or(io::ErrorKind::InvalidInput)?)?
                    .sync_all()?;
                Ok(true)
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err),
        }
    }
}
