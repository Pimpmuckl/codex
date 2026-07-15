use super::*;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
fn plan(method: InstallMethod, channel: UpdateChannel) -> UpdatePlan {
    let context = InstallContext {
        method,
        package_layout: None,
    };
    UpdatePlan::for_install_context(&context, channel)
}
#[test]
fn package_manager_plans_preserve_the_manager() {
    for (method, manager) in [
        (InstallMethod::Npm, PackageManager::Npm),
        (InstallMethod::Bun, PackageManager::Bun),
        (InstallMethod::Pnpm, PackageManager::Pnpm),
    ] {
        assert_eq!(
            plan(method, UpdateChannel::CodexPlusPlus).target(),
            UpdateTarget::PackageManager {
                manager,
                package: "@jjliebig/codex-plus-plus",
            }
        );
    }
    assert_eq!(
        plan(InstallMethod::Npm, UpdateChannel::Upstream).target(),
        UpdateTarget::PackageManager {
            manager: PackageManager::Npm,
            package: "@openai/codex",
        }
    );
}
#[test]
fn standalone_and_manual_plans_stay_with_the_requested_channel() {
    let release_dir = AbsolutePathBuf::from_absolute_path(std::env::temp_dir())
        .expect("temp directory should be absolute");
    let windows = || InstallMethod::Standalone {
        release_dir: release_dir.clone(),
        resources_dir: None,
        platform: StandalonePlatform::Windows,
    };
    assert_eq!(
        [
            plan(windows(), UpdateChannel::CodexPlusPlus).target(),
            plan(windows(), UpdateChannel::Upstream).target(),
            plan(InstallMethod::Brew, UpdateChannel::CodexPlusPlus).target(),
            plan(InstallMethod::Brew, UpdateChannel::Upstream).target(),
        ],
        [
            UpdateTarget::Standalone {
                platform: StandalonePlatform::Windows,
                installer_url: CODEX_PLUS_PLUS_WINDOWS_INSTALLER_URL,
            },
            UpdateTarget::Standalone {
                platform: StandalonePlatform::Windows,
                installer_url: UPSTREAM_WINDOWS_INSTALLER_URL,
            },
            UpdateTarget::Manual,
            UpdateTarget::Homebrew,
        ]
    );
}
#[test]
fn version_comparison_distinguishes_plain_and_fork_versions() {
    for (latest, current, expected) in [
        ("0.144.5", "0.144.4", Some(true)),
        ("0.144.4-fork.2", "0.144.4-fork.1", Some(true)),
        ("0.144.4-fork.1", "0.144.4", None),
        ("0.145.0-beta.1", "0.144.4", None),
        ("not-a-version", "0.144.4", None),
    ] {
        assert_eq!(is_newer(latest, current), expected, "{current} -> {latest}");
    }
}
