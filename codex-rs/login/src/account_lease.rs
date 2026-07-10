use fs2::FileExt as _;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::path::Path;

pub(crate) struct AccountLease {
    file: File,
}

impl AccountLease {
    pub(crate) fn acquire_auth_refresh(auth_home: &Path) -> io::Result<Self> {
        Self::acquire(&auth_home.join(".auth-refresh.lock"))
    }

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

fn is_contended(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::WouldBlock
        || cfg!(windows) && matches!(err.raw_os_error(), Some(32 | 33))
}

impl Drop for AccountLease {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}
