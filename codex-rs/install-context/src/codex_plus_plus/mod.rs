use std::path::Path;

mod release_status;
mod update_plan;

pub use release_status::FORK_RELEASE_STATUS_MAX_AGE;
pub use release_status::ForkReleaseStatus;
pub use update_plan::LatestVersionSource;
pub use update_plan::PackageManager;
pub use update_plan::UpdateChannel;
pub use update_plan::UpdatePlan;
pub use update_plan::UpdateTarget;
pub use update_plan::is_newer;

pub(crate) fn is_managed_standalone_release_dir(release_dir: &Path, codex_home: &Path) -> bool {
    ["standalone", "codex-plus-plus"].into_iter().any(|name| {
        release_dir.starts_with(codex_home.join("packages").join(name).join("releases"))
    })
}
