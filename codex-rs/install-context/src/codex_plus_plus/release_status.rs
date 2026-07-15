use super::is_newer;
use super::update_plan::parse_version;
use std::time::Duration;
use std::time::SystemTime;

const LAG_WARNING_THRESHOLD: u64 = 3;
pub const FORK_RELEASE_STATUS_MAX_AGE: Duration = Duration::from_secs(20 * 60 * 60);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForkReleaseStatus {
    pub installed_fork_version: String,
    pub latest_fork_version: Option<String>,
    pub latest_stable_upstream_version: Option<String>,
    pub fork_upstream_base: Option<String>,
    pub stable_minor_lag: Option<u64>,
    pub checked_at: SystemTime,
    pub warning_state_key: Option<String>,
    pub dismissed_version: Option<String>,
}

impl ForkReleaseStatus {
    pub fn new(
        installed_fork_version: String,
        latest_fork_version: Option<String>,
        latest_stable_upstream_version: Option<String>,
        checked_at: SystemTime,
    ) -> Self {
        let latest_stable_upstream_version =
            latest_stable_upstream_version.filter(|version| stable_version(version).is_some());
        let fork_base = latest_fork_version.as_deref().and_then(fork_upstream_base);
        let upstream = latest_stable_upstream_version
            .as_deref()
            .and_then(stable_version);
        let fork_upstream_base =
            fork_base.map(|(major, minor, patch)| format!("{major}.{minor}.{patch}"));
        let stable_minor_lag = fork_base.zip(upstream).and_then(|(fork, upstream)| {
            (fork.0 == upstream.0).then_some(upstream.1.saturating_sub(fork.1))
        });
        let warning_state_key = latest_fork_version
            .as_deref()
            .filter(|latest| is_newer(latest, &installed_fork_version) == Some(true))
            .map(|latest| format!("fork-update/{latest}"))
            .or_else(|| match (stable_minor_lag, fork_base, upstream) {
                (Some(lag), Some(fork), Some(upstream)) if lag >= LAG_WARNING_THRESHOLD => {
                    Some(format!(
                        "upstream-lag/{}.{}/{}.{}",
                        fork.0, fork.1, upstream.0, upstream.1
                    ))
                }
                _ => None,
            });

        Self {
            installed_fork_version,
            latest_fork_version,
            latest_stable_upstream_version,
            fork_upstream_base,
            stable_minor_lag,
            checked_at,
            warning_state_key,
            dismissed_version: None,
        }
    }

    pub fn with_installed_fork_version(self, installed_fork_version: String) -> Self {
        let mut status = Self::new(
            installed_fork_version,
            self.latest_fork_version,
            self.latest_stable_upstream_version,
            self.checked_at,
        );
        status.dismissed_version = self.dismissed_version;
        status
    }

    pub fn unavailable(installed_fork_version: String) -> Self {
        Self::new(installed_fork_version, None, None, std::time::UNIX_EPOCH)
    }

    pub fn is_stale(&self, now: SystemTime, max_age: Duration) -> bool {
        now.duration_since(self.checked_at)
            .is_ok_and(|age| age > max_age)
    }
}

fn fork_upstream_base(version: &str) -> Option<(u64, u64, u64)> {
    let (base, revision) = parse_version(version)?;
    revision.map(|_| base)
}

fn stable_version(version: &str) -> Option<(u64, u64, u64)> {
    let (base, revision) = parse_version(version)?;
    revision.is_none().then_some(base)
}

#[cfg(test)]
#[path = "release_status_tests.rs"]
mod tests;
