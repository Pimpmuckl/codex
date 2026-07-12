use super::super::AuthDotJson;
use sha2::Digest;
use sha2::Sha256;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Write;
use std::io::{self};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::Mutex;
const MAX_AUTH_BYTES: u64 = 1024 * 1024;
const NONE_FINGERPRINT: &str = "0000000000000000";
const PENDING_PREFIX: &str = ".codex-auth-";
#[derive(Debug)]
pub(in crate::auth::storage) struct AtomicFileStorage {
    codex_home: PathBuf,
    #[cfg(test)]
    fault: Mutex<Option<FaultPoint>>,
}
impl AtomicFileStorage {
    pub(in crate::auth::storage) fn new(codex_home: PathBuf) -> Self {
        Self {
            codex_home,
            #[cfg(test)]
            fault: Mutex::new(None),
        }
    }
    pub(in crate::auth::storage) fn load(&self) -> io::Result<Option<AuthDotJson>> {
        self.recover_save()?;
        read_auth(&self.auth_path())
    }
    pub(in crate::auth::storage) fn load_without_recovery(
        &self,
    ) -> io::Result<Option<AuthDotJson>> {
        if single_artifact(&self.codex_home)?.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "pending auth save requires recovery",
            ));
        }
        read_auth(&self.auth_path())
    }
    pub(in crate::auth::storage) fn save(&self, auth: &AuthDotJson) -> io::Result<()> {
        std::fs::create_dir_all(&self.codex_home)?;
        if let Some(parent) = self.codex_home.parent() {
            sync_directory(parent)?;
        }
        self.recover_save()?;
        let prior_fingerprint =
            file_fingerprint(&self.auth_path())?.unwrap_or_else(|| NONE_FINGERPRINT.to_string());
        let new_fingerprint = fingerprint(auth)?;
        let prior_short = short_fingerprint(&prior_fingerprint);
        let new_short = short_fingerprint(&new_fingerprint);
        let pending = format!("{PENDING_PREFIX}{prior_short}-{new_short}");
        let pending = self.codex_home.join(pending);
        let bytes = serde_json::to_vec_pretty(auth).map_err(io::Error::other)?;
        if bytes.len() as u64 > MAX_AUTH_BYTES {
            return Err(invalid_data("auth payload exceeds 1 MiB"));
        }
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&pending)?;
        file.write_all(&bytes)?;
        self.hit(FaultPoint::BeforePendingSync)?;
        file.sync_all()?;
        self.hit(FaultPoint::AfterPendingSync)?;
        sync_directory(&self.codex_home)?;
        self.hit(FaultPoint::BeforeReplace)?;
        self.promote_pending(&pending, prior_short, new_short)?;
        self.hit(FaultPoint::AfterReplace)?;
        sync_directory(&self.codex_home)?;
        self.hit(FaultPoint::AfterReplaceDirectorySync)
    }
    pub(super) fn recover_save(&self) -> io::Result<()> {
        let Some(pending) = single_artifact(&self.codex_home)? else {
            return Ok(());
        };
        let (prior, new) = pending
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(pending_hashes)
            .ok_or_else(|| invalid_data("invalid pending auth filename"))?;
        let pending_valid = read_auth(&pending).and_then(|auth| {
            auth.ok_or_else(|| invalid_data("pending auth payload is missing"))
                .and_then(|auth| fingerprint(&auth).map(|actual| short_fingerprint(&actual) == new))
        });
        if !matches!(&pending_valid, Ok(true)) {
            let current = file_fingerprint(&self.auth_path())?;
            let current = current.as_deref().map(short_fingerprint);
            let expected_prior = (prior != NONE_FINGERPRINT).then_some(prior);
            if current == expected_prior || current == Some(new) {
                std::fs::remove_file(pending)?;
                return sync_directory(&self.codex_home);
            }
        }
        match pending_valid {
            Ok(true) => {}
            Ok(false) => return Err(invalid_data("pending auth fingerprint changed")),
            Err(err) => return Err(err),
        }
        OpenOptions::new().write(true).open(&pending)?.sync_all()?;
        self.promote_pending(&pending, prior, new)?;
        sync_directory(&self.codex_home)
    }
    fn promote_pending(&self, pending: &Path, prior: &str, new: &str) -> io::Result<()> {
        let pending_auth =
            read_auth(pending)?.ok_or_else(|| invalid_data("pending auth payload is missing"))?;
        if short_fingerprint(&fingerprint(&pending_auth)?) != new {
            return Err(invalid_data("pending auth fingerprint changed"));
        }
        let current_fingerprint = file_fingerprint(&self.auth_path())?;
        let current_fingerprint = current_fingerprint.as_deref().map(short_fingerprint);
        if current_fingerprint == Some(new) {
            std::fs::remove_file(pending)?;
            return Ok(());
        }
        let expected_prior = (prior != NONE_FINGERPRINT).then_some(prior);
        if current_fingerprint != expected_prior {
            return Err(invalid_data("current auth fingerprint changed"));
        }
        crate::account::replace_file(pending, &self.auth_path())
    }
    fn auth_path(&self) -> PathBuf {
        self.codex_home.join("auth.json")
    }
    #[cfg(not(test))]
    fn hit(&self, _point: FaultPoint) -> io::Result<()> {
        Ok(())
    }
    #[cfg(test)]
    fn hit(&self, point: FaultPoint) -> io::Result<()> {
        let mut fault = self
            .fault
            .lock()
            .map_err(|_| io::Error::other("fault lock is poisoned"))?;
        if *fault == Some(point) {
            *fault = None;
            Err(io::Error::other(format!("injected crash at {point:?}")))
        } else {
            Ok(())
        }
    }
    #[cfg(test)]
    pub(in crate::auth::storage) fn fail_once(&self, point: FaultPoint) {
        if let Ok(mut fault) = self.fault.lock() {
            *fault = Some(point);
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::auth::storage) enum FaultPoint {
    BeforePendingSync,
    AfterPendingSync,
    BeforeReplace,
    AfterReplace,
    AfterReplaceDirectorySync,
}
fn read_auth(path: &Path) -> io::Result<Option<AuthDotJson>> {
    read_file(path)?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(io::Error::other))
        .transpose()
}
fn read_file(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid_data("auth path is not a regular file"));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_AUTH_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_AUTH_BYTES {
        return Err(invalid_data("auth payload exceeds 1 MiB"));
    }
    Ok(Some(bytes))
}
fn file_fingerprint(path: &Path) -> io::Result<Option<String>> {
    if let Ok(metadata) = std::fs::symlink_metadata(path)
        && metadata.is_file()
        && metadata.len() > MAX_AUTH_BYTES
    {
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_AUTH_BYTES)
            .read_to_end(&mut bytes)?;
        let mut hasher = Sha256::new();
        hasher.update(metadata.len().to_le_bytes());
        hasher.update(bytes);
        return Ok(Some(format!("{:x}", hasher.finalize())));
    }
    let Some(bytes) = read_file(path)? else {
        return Ok(None);
    };
    let fingerprint = match serde_json::from_slice::<AuthDotJson>(&bytes) {
        Ok(auth) => fingerprint(&auth)?,
        Err(_) => format!("{:x}", Sha256::digest(bytes)),
    };
    Ok(Some(fingerprint))
}
fn fingerprint(auth: &AuthDotJson) -> io::Result<String> {
    let bytes = serde_json::to_vec(auth).map_err(io::Error::other)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
fn artifacts(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut matches = entries.collect::<io::Result<Vec<_>>>()?;
    matches.retain(|entry| pending_hashes(&entry.file_name().to_string_lossy()).is_some());
    Ok(matches.iter().map(std::fs::DirEntry::path).collect())
}
fn single_artifact(directory: &Path) -> io::Result<Option<PathBuf>> {
    let matches = artifacts(directory)?;
    if matches.len() > 1 {
        return Err(invalid_data("multiple auth recovery artifacts found"));
    }
    Ok(matches.into_iter().next())
}
fn pending_hashes(name: &str) -> Option<(&str, &str)> {
    let encoded = name.strip_prefix(PENDING_PREFIX)?;
    encoded
        .split_once('-')
        .filter(|(prior, new)| valid_hash(prior) && valid_hash(new))
}
fn valid_hash(value: &str) -> bool {
    value.len() == NONE_FINGERPRINT.len()
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}
fn short_fingerprint(value: &str) -> &str {
    &value[..NONE_FINGERPRINT.len()]
}
fn invalid_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
#[cfg(unix)]
fn sync_directory(directory: &Path) -> io::Result<()> {
    File::open(directory)?.sync_all()
}
#[cfg(windows)]
fn sync_directory(_directory: &Path) -> io::Result<()> {
    Ok(())
}
#[cfg(test)]
#[path = "atomic_file_tests.rs"]
mod tests;
