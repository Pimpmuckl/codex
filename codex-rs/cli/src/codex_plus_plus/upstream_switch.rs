use anyhow::Context;
use codex_core::config::Config;
use codex_install_context::InstallContext;
use codex_install_context::codex_plus_plus::LatestVersionSource;
use codex_install_context::codex_plus_plus::PackageManager;
use codex_install_context::codex_plus_plus::UpdateChannel;
use codex_install_context::codex_plus_plus::UpdatePlan;
use codex_install_context::codex_plus_plus::UpdateTarget;
use codex_login::AccountHandoffOutcome;
use codex_login::AccountStore;
use codex_login::default_client::create_client;
use codex_utils_cli::CliConfigOverrides;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

const MANAGED_PACKAGE_ROOT_ENV: &str = "CODEX_MANAGED_PACKAGE_ROOT";

pub(crate) async fn run(config_overrides: CliConfigOverrides) -> anyhow::Result<()> {
    #[cfg(windows)]
    if !std::env::args_os()
        .skip(1)
        .eq(["update", "upstream"].map(std::ffi::OsString::from))
    {
        anyhow::bail!("on Windows, rerun exactly as `codex update upstream`");
    }
    let overrides = config_overrides
        .parse_overrides()
        .map_err(|err| anyhow::anyhow!("error parsing -c overrides: {err}"))?;
    let config = Config::load_with_cli_overrides(overrides)
        .await
        .context("error loading configuration")?;
    let mut adapter = SystemAdapter {
        context: InstallContext::current().clone(),
        store: AccountStore::new(config.codex_home.to_path_buf()),
        store_mode: config.cli_auth_credentials_store_mode,
        keyring_backend: config.auth_keyring_backend_kind(),
    };
    run_with_adapter(
        &mut adapter,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    )
    .await
}

/// Owns every external read or mutation used by one upstream switch transaction.
trait SwitchAdapter {
    fn preflight(&mut self) -> impl Future<Output = anyhow::Result<Preflight>> + Send;
    fn export_selected_profile(&mut self) -> anyhow::Result<ProfileHandoff>;
    fn install_upstream(&mut self, preflight: &Preflight) -> anyhow::Result<()>;
    fn verify_upstream(&mut self, preflight: &Preflight) -> anyhow::Result<()>;
    fn rollback_fork(&mut self, preflight: &Preflight) -> anyhow::Result<()>;
    fn reconcile_root_auth(&mut self) -> anyhow::Result<()>;
}

async fn run_with_adapter(
    adapter: &mut impl SwitchAdapter,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> anyhow::Result<()> {
    line(stderr, "Resolving upstream and rollback artifacts...");
    let preflight = adapter.preflight().await?;
    if matches!(preflight, Preflight::AlreadyUpstream) {
        line(stdout, "Already using upstream Codex.");
        return Ok(());
    }

    line(
        stderr,
        "Preparing the selected account for upstream Codex...",
    );
    let handoff = adapter.export_selected_profile()?;
    line(stderr, "Installing and verifying upstream Codex...");
    let switch_result = adapter
        .install_upstream(&preflight)
        .and_then(|()| adapter.verify_upstream(&preflight));
    if let Err(switch_error) = switch_result {
        line(stderr, "Upstream switch failed; restoring Codex++...");
        let mut rollback_errors = Vec::new();
        if let Err(err) = adapter.rollback_fork(&preflight) {
            rollback_errors.push(err.to_string());
        }
        if handoff == ProfileHandoff::Selected
            && let Err(err) = adapter.reconcile_root_auth()
        {
            rollback_errors.push(format!("restore account handoff: {err}"));
        }
        if rollback_errors.is_empty() {
            return Err(switch_error.context("upstream switch failed; Codex++ was restored"));
        }
        anyhow::bail!(
            "upstream switch failed: {switch_error}; rollback failed: {}",
            rollback_errors.join("; ")
        );
    }

    line(
        stdout,
        &format!(
            "Switched to upstream Codex {}.",
            preflight.upstream_version()
        ),
    );
    if handoff == ProfileHandoff::NoUsableProfile {
        line(
            stdout,
            "No usable Codex++ profile was selected; run `codex login` to sign in.",
        );
    }
    Ok(())
}

fn line(writer: &mut impl Write, value: &str) {
    let _ = writeln!(writer, "{value}");
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProfileHandoff {
    Selected,
    NoUsableProfile,
}

enum Preflight {
    AlreadyUpstream,
    Package(PackageSwitch),
}

impl Preflight {
    fn upstream_version(&self) -> &str {
        match self {
            Self::Package(switch) => &switch.upstream.version,
            Self::AlreadyUpstream => unreachable!("no-op switches have no target version"),
        }
    }
}

struct PackageSwitch {
    manager: PackageManager,
    upstream: VerifiedPackageArtifact,
    rollback: VerifiedPackageArtifact,
}

struct VerifiedPackageArtifact {
    package: &'static str,
    version: String,
}

impl VerifiedPackageArtifact {
    fn exact_spec(&self) -> String {
        format!("{}@{}", self.package, self.version)
    }
}

struct SystemAdapter {
    context: InstallContext,
    store: AccountStore,
    store_mode: codex_login::AuthCredentialsStoreMode,
    keyring_backend: codex_login::AuthKeyringBackendKind,
}

impl SwitchAdapter for SystemAdapter {
    async fn preflight(&mut self) -> anyhow::Result<Preflight> {
        let upstream_plan = UpdatePlan::for_install_context(&self.context, UpdateChannel::Upstream);
        let rollback_plan =
            UpdatePlan::for_install_context(&self.context, UpdateChannel::CodexPlusPlus);
        match (upstream_plan.target(), rollback_plan.target()) {
            (
                UpdateTarget::PackageManager {
                    manager,
                    package: upstream_package,
                },
                UpdateTarget::PackageManager {
                    manager: rollback_manager,
                    package: rollback_package,
                },
            ) if manager == rollback_manager => {
                self.preflight_package(
                    manager,
                    upstream_package,
                    rollback_package,
                    upstream_plan,
                    rollback_plan,
                )
                .await
            }
            _ => anyhow::bail!(
                "this Codex++ installation cannot be switched automatically; reinstall upstream Codex manually"
            ),
        }
    }

    fn export_selected_profile(&mut self) -> anyhow::Result<ProfileHandoff> {
        match self
            .store
            .export_selected_account_to_root_auth(self.store_mode, self.keyring_backend)?
        {
            AccountHandoffOutcome::NoHandoff
            | AccountHandoffOutcome::UnavailableForEphemeralStore => {
                Ok(ProfileHandoff::NoUsableProfile)
            }
            AccountHandoffOutcome::Completed(_)
            | AccountHandoffOutcome::PreservedNewerProfile(_)
            | AccountHandoffOutcome::RootRetained(_) => Ok(ProfileHandoff::Selected),
        }
    }

    fn install_upstream(&mut self, preflight: &Preflight) -> anyhow::Result<()> {
        match preflight {
            Preflight::Package(switch) => {
                uninstall_package(switch.manager, switch.rollback.package)?;
                install_package(switch.manager, &switch.upstream.exact_spec())
            }
            Preflight::AlreadyUpstream => Ok(()),
        }
    }

    fn verify_upstream(&mut self, preflight: &Preflight) -> anyhow::Result<()> {
        match preflight {
            Preflight::Package(switch) => verify_visible_version(&switch.upstream.version),
            Preflight::AlreadyUpstream => Ok(()),
        }
    }

    fn rollback_fork(&mut self, preflight: &Preflight) -> anyhow::Result<()> {
        match preflight {
            Preflight::Package(switch) => {
                let _ = uninstall_package(switch.manager, switch.upstream.package);
                install_package(switch.manager, &switch.rollback.exact_spec())?;
                verify_visible_version(&switch.rollback.version)
            }
            Preflight::AlreadyUpstream => Ok(()),
        }
    }

    fn reconcile_root_auth(&mut self) -> anyhow::Result<()> {
        self.store
            .reconcile_root_auth_to_matching_account(self.store_mode, self.keyring_backend)?;
        Ok(())
    }
}

impl SystemAdapter {
    async fn preflight_package(
        &self,
        manager: PackageManager,
        upstream_package: &'static str,
        rollback_package: &'static str,
        upstream_plan: UpdatePlan,
        rollback_plan: UpdatePlan,
    ) -> anyhow::Result<Preflight> {
        let root = absolute_env(MANAGED_PACKAGE_ROOT_ENV)?;
        let metadata: PackageMetadata = read_json(&root.join("package.json"))?;
        if metadata.name == upstream_package {
            return Ok(Preflight::AlreadyUpstream);
        }
        if metadata.name != rollback_package || metadata.version.trim().is_empty() {
            anyhow::bail!("running package is not a usable {rollback_package} rollback source");
        }
        ensure_manager_targets_root(manager, rollback_package, &root)?;
        let upstream =
            verified_package(upstream_plan, upstream_package, PackageSelector::Latest).await?;
        let rollback = verified_package(
            rollback_plan,
            rollback_package,
            PackageSelector::Exact(&metadata.version),
        )
        .await?;
        Ok(Preflight::Package(PackageSwitch {
            manager,
            upstream,
            rollback,
        }))
    }
}

#[derive(Deserialize)]
struct PackageMetadata {
    #[serde(default)]
    name: String,
    version: String,
}

#[derive(Deserialize)]
struct NpmPackageInfo {
    #[serde(rename = "dist-tags")]
    dist_tags: HashMap<String, String>,
    versions: HashMap<String, NpmVersion>,
}

#[derive(Deserialize)]
struct NpmVersion {
    dist: NpmDist,
}

#[derive(Deserialize)]
struct NpmDist {
    tarball: String,
    integrity: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
}

enum PackageSelector<'a> {
    Latest,
    Exact(&'a str),
}

async fn verified_package(
    plan: UpdatePlan,
    package: &'static str,
    selector: PackageSelector<'_>,
) -> anyhow::Result<VerifiedPackageArtifact> {
    let registry_url = match plan.latest_version_source() {
        LatestVersionSource::GitHub {
            npm_registry_url: Some(url),
            ..
        } => url,
        _ => anyhow::bail!("package update plan is missing an npm registry source"),
    };
    let info: NpmPackageInfo = fetch_json(registry_url).await?;
    let version = match selector {
        PackageSelector::Exact(version) => version.to_string(),
        PackageSelector::Latest => {
            let (release, prefix) = latest_release(plan).await?;
            let version = stable_release_version(&release, prefix)?;
            if info.dist_tags.get("latest") != Some(&version) {
                anyhow::bail!("npm latest does not match upstream release {version}");
            }
            version
        }
    };
    let dist = &info
        .versions
        .get(&version)
        .ok_or_else(|| anyhow::anyhow!("{package} {version} is not published"))?
        .dist;
    if dist.integrity.trim().is_empty() || !dist.tarball.starts_with("https://") {
        anyhow::bail!("{package} {version} is missing usable integrity metadata");
    }
    Ok(VerifiedPackageArtifact { package, version })
}

async fn latest_release(plan: UpdatePlan) -> anyhow::Result<(GithubRelease, &'static str)> {
    let LatestVersionSource::GitHub {
        api_url,
        tag_prefix,
        ..
    } = plan.latest_version_source()
    else {
        anyhow::bail!("update plan is missing a GitHub release source");
    };
    Ok((fetch_json(api_url).await?, tag_prefix))
}

async fn fetch_json<T: DeserializeOwned>(url: &str) -> anyhow::Result<T> {
    Ok(create_client()
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json::<T>()
        .await?)
}

fn stable_release_version(release: &GithubRelease, prefix: &str) -> anyhow::Result<String> {
    let version = release.tag_name.strip_prefix(prefix).unwrap_or_default();
    if release.draft
        || release.prerelease
        || version.split('.').count() != 3
        || !version.split('.').all(|part| part.parse::<u64>().is_ok())
    {
        anyhow::bail!("latest upstream release is not a stable Codex release");
    }
    Ok(version.to_string())
}

fn absolute_env(name: &str) -> anyhow::Result<PathBuf> {
    let path =
        PathBuf::from(std::env::var_os(name).ok_or_else(|| anyhow::anyhow!("{name} is not set"))?);
    if !path.is_absolute() {
        anyhow::bail!("{name} is not an absolute path");
    }
    Ok(path)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    serde_json::from_reader(
        std::fs::File::open(path).with_context(|| format!("failed to read {}", path.display()))?,
    )
    .with_context(|| format!("failed to parse {}", path.display()))
}

fn uninstall_package(manager: PackageManager, package: &str) -> anyhow::Result<()> {
    let mut command = package_command(manager);
    match manager {
        PackageManager::Npm => command.args(["uninstall", "-g", package]),
        PackageManager::Bun => command.args(["remove", "-g", package]),
        PackageManager::Pnpm => command.args(["remove", "-g", package]),
    };
    run_quiet(command, "uninstall current package")
}

fn install_package(manager: PackageManager, package: &str) -> anyhow::Result<()> {
    let mut command = package_command(manager);
    match manager {
        PackageManager::Npm | PackageManager::Bun => command.args(["install", "-g", package]),
        PackageManager::Pnpm => command.args(["add", "-g", package]),
    };
    run_quiet(command, "install package")
}

fn package_command(manager: PackageManager) -> Command {
    let executable = match manager {
        PackageManager::Npm => "npm",
        PackageManager::Bun => "bun",
        PackageManager::Pnpm => "pnpm",
    };
    if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args(["/C", executable]);
        command
    } else {
        Command::new(executable)
    }
}

fn ensure_manager_targets_root(
    manager: PackageManager,
    package: &str,
    running_root: &Path,
) -> anyhow::Result<()> {
    let mut command = package_command(manager);
    match manager {
        PackageManager::Npm | PackageManager::Pnpm => command.args(["root", "-g"]),
        PackageManager::Bun => command.args(["pm", "bin", "-g"]),
    };
    let output = command
        .output()
        .context("failed to inspect package-manager root")?;
    anyhow::ensure!(output.status.success(), "package-manager root check failed");
    let stdout = String::from_utf8(output.stdout).context("package-manager root was not UTF-8")?;
    let bun_global_dir = std::env::var_os("BUN_INSTALL_GLOBAL_DIR").map(PathBuf::from);
    let target = manager_global_root(manager, &stdout, bun_global_dir.as_deref())?.join(package);
    let running = running_root.canonicalize()?;
    let target = target.canonicalize()?;
    anyhow::ensure!(running == target, "wrong package-manager target");
    Ok(())
}

fn manager_global_root(
    manager: PackageManager,
    stdout: &str,
    bun_global_dir: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    let reported = stdout.trim();
    anyhow::ensure!(!reported.is_empty(), "empty package-manager root");
    match (manager, bun_global_dir) {
        (PackageManager::Bun, Some(path)) => Ok(path.join("node_modules")),
        (PackageManager::Bun, None) => Ok(Path::new(reported)
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Bun returned an invalid global bin directory"))?
            .join("install/global/node_modules")),
        (PackageManager::Npm | PackageManager::Pnpm, _) => Ok(PathBuf::from(reported)),
    }
}

fn run_quiet(mut command: Command, label: &str) -> anyhow::Result<()> {
    let output = command
        .output()
        .with_context(|| format!("failed to run {label}"))?;
    if !output.status.success() {
        anyhow::bail!("{label} failed with status {}", output.status);
    }
    Ok(())
}

fn verify_visible_version(expected: &str) -> anyhow::Result<()> {
    let mut command = if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args(["/C", "codex", "--version"]);
        command
    } else {
        let mut command = Command::new("codex");
        command.arg("--version");
        command
    };
    let output = command.output().context("failed to run installed Codex")?;
    let stdout = String::from_utf8(output.stdout).context("Codex version was not UTF-8")?;
    let actual = stdout.split_whitespace().last();
    if !output.status.success() || actual != Some(expected) {
        anyhow::bail!("installed Codex did not verify as upstream version {expected}");
    }
    Ok(())
}

#[cfg(test)]
#[path = "upstream_switch_tests.rs"]
mod tests;
