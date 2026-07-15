//! Diagnoses whether Codex update paths target the running installation.
//!
//! Update diagnostics combine cached release status and install-channel hints.
//! For npm-managed launches, this module also
//! verifies that npm install -g would update the package root that launched the
//! current process, which catches PATH and prefix mismatches before the user runs
//! an update command.

use std::path::Path;
use std::time::SystemTime;

use codex_core::config::Config;
use codex_install_context::InstallContext;
use codex_install_context::codex_plus_plus::FORK_RELEASE_STATUS_MAX_AGE;
use codex_install_context::codex_plus_plus::UpdateChannel;
use codex_install_context::codex_plus_plus::UpdatePlan;
use codex_install_context::codex_plus_plus::is_newer;
use codex_tui::UpdateAction;

use super::CheckStatus;
use super::DoctorCheck;
use super::NpmRootCheck;
use super::doctor_install_context;
use super::doctor_managed_by_npm;
use super::npm_global_root_check;
/// Builds the update-health row for the current installation.
///
/// Missing or stale release status degrades the row to a warning instead of
/// masking more direct install or configuration failures.
pub(super) fn updates_check(config: &Config) -> DoctorCheck {
    let current_exe = std::env::current_exe().ok();
    let install_context = doctor_install_context(current_exe.as_deref());
    let update_plan =
        UpdatePlan::for_install_context(&install_context, UpdateChannel::CodexPlusPlus);
    let mut details = vec![
        format!(
            "check for update on startup: {}",
            config.check_for_update_on_startup
        ),
        format!("update action: {}", update_action_label(&install_context)),
    ];
    let mut status = CheckStatus::Ok;
    let mut summary = "update configuration is locally consistent".to_string();
    let mut remediation = None;
    if push_cached_version_details(&mut details, config.codex_home.as_path()) {
        status = CheckStatus::Warning;
        summary = "fork release status cache is stale or unavailable".to_string();
    }

    if doctor_managed_by_npm(current_exe.as_deref())
        && let Some(package) = update_plan.package_manager_package()
    {
        match npm_global_root_check(package) {
            NpmRootCheck::Match { package_root } => {
                details.push(format!("npm update target: {}", package_root.display()));
            }
            NpmRootCheck::Mismatch {
                running_package_root,
                npm_package_root,
            } => {
                status = CheckStatus::Fail;
                summary = "update would target a different npm install".to_string();
                details.push(format!(
                    "running package root: {}",
                    running_package_root.display()
                ));
                details.push(format!("npm package root: {}", npm_package_root.display()));
                remediation = Some(format!(
                    "Fix PATH or npm prefix so the running package root ({}) matches the npm global package root ({}).",
                    running_package_root.display(),
                    npm_package_root.display()
                ));
            }
            NpmRootCheck::MissingPackageRoot => {
                status = status.max(CheckStatus::Warning);
                summary = "npm update target could not be proven".to_string();
                remediation = Some(
                    "Reinstall or update Codex so the JS shim provides CODEX_MANAGED_PACKAGE_ROOT."
                        .to_string(),
                );
            }
            NpmRootCheck::NpmUnavailable(error) => {
                status = status.max(CheckStatus::Warning);
                summary = "npm update target could not be inspected".to_string();
                details.push(format!("npm root -g failed: {error}"));
            }
        }
    }

    let mut check = DoctorCheck::new("updates.status", "updates", status, summary).details(details);
    if let Some(remediation) = remediation {
        check = check.remediation(remediation);
    }
    check
}

fn push_cached_version_details(details: &mut Vec<String>, codex_home: &Path) -> bool {
    let info = UpdateAction::read_cached_fork_release_status(codex_home, env!("CARGO_PKG_VERSION"));
    if let Some(latest) = info.latest_fork_version.as_deref() {
        details.push(format!("cached latest version: {latest}"));
        let availability = if is_newer(latest, env!("CARGO_PKG_VERSION")) == Some(true) {
            "newer version is available"
        } else {
            "current version is not older"
        };
        details.push(format!("latest version status: {availability}"));
    }
    if let Some(dismissed) = info.dismissed_version.as_deref() {
        details.push(format!("dismissed version: {dismissed}"));
    }
    info.is_stale(SystemTime::now(), FORK_RELEASE_STATUS_MAX_AGE)
}

fn update_action_label(context: &InstallContext) -> String {
    let plan = UpdatePlan::for_install_context(context, UpdateChannel::CodexPlusPlus);
    UpdateAction::from_update_plan(plan)
        .map(UpdateAction::command_str)
        .unwrap_or_else(|| format!("manual: {}", plan.channel().install_url()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_install_context::InstallMethod;
    use pretty_assertions::assert_eq;

    #[test]
    fn update_action_labels_use_the_planned_distribution() {
        assert_eq!(
            update_action_label(&InstallContext {
                method: InstallMethod::Npm,
                package_layout: None,
            }),
            "npm install -g @jjliebig/codex-plus-plus"
        );
        assert_eq!(
            update_action_label(&InstallContext {
                method: InstallMethod::Pnpm,
                package_layout: None,
            }),
            "pnpm add -g @jjliebig/codex-plus-plus"
        );
        assert_eq!(
            update_action_label(&InstallContext {
                method: InstallMethod::Other,
                package_layout: None,
            }),
            "manual: https://github.com/Pimpmuckl/codex#install-a-release"
        );
    }

    #[test]
    fn update_details_use_the_shared_fork_release_cache() {
        let codex_home = tempfile::tempdir().expect("temp codex home");
        let cache_dir = codex_home.path().join("codex-plus-plus");
        std::fs::create_dir_all(&cache_dir).expect("create cache directory");
        let checked_at = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_secs();
        std::fs::write(
            cache_dir.join("release-status.json"),
            serde_json::json!({
                "installed_fork_version": "0.144.4-fork.1",
                "latest_fork_version": "0.144.4-fork.2",
                "latest_stable_upstream_version": "0.147.0",
                "checked_at": {
                    "secs_since_epoch": checked_at,
                    "nanos_since_epoch": 0,
                },
                "dismissed_version": null,
            })
            .to_string(),
        )
        .expect("write release cache");
        let mut details = Vec::new();

        assert!(!push_cached_version_details(
            &mut details,
            codex_home.path()
        ));
        assert_eq!(
            details,
            vec![
                "cached latest version: 0.144.4-fork.2".to_string(),
                "latest version status: current version is not older".to_string(),
            ]
        );
    }
}
