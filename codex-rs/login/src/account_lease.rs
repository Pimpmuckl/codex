use fs2::FileExt as _;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) struct AccountLease {
    file: File,
}

impl AccountLease {
    pub(crate) fn acquire(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.lock_exclusive()?;
        Ok(Self { file })
    }

    pub(crate) fn try_acquire(path: &Path) -> io::Result<Option<Self>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
        {
            Ok(file) => file,
            Err(err) if is_contended(&err) => return Ok(None),
            Err(err) => return Err(err),
        };
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { file })),
            Err(err) if is_contended(&err) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

#[derive(Clone)]
pub(crate) struct AuthRefreshGuard {
    auth_home: PathBuf,
    _lease: Arc<AccountLease>,
}

impl AuthRefreshGuard {
    pub(crate) fn acquire(auth_home: &Path) -> io::Result<Self> {
        let lease = AccountLease::acquire(&auth_home.join(".auth-refresh.lock"))?;
        Ok(Self::new(auth_home, lease))
    }

    pub(crate) fn ensure_matches(&self, auth_home: &Path) -> io::Result<()> {
        if self.auth_home == normalized(auth_home) {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "auth refresh guard does not match auth home",
            ))
        }
    }

    fn new(auth_home: &Path, lease: AccountLease) -> Self {
        Self {
            auth_home: normalized(auth_home),
            _lease: Arc::new(lease),
        }
    }
}

fn normalized(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn is_contended(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::WouldBlock
        || cfg!(windows) && matches!(err.raw_os_error(), Some(32 | 33))
}

impl Drop for AccountLease {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}
