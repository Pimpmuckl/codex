use crate::app_server_session::AppServerSession;
use crate::config_update::replace_config_value;
use crate::config_update::write_config_batch;
use crate::hooks_rpc::fetch_hooks_list;
use crate::hooks_rpc::hooks_list_entry_for_cwd;
use crate::legacy_core::config::Config;
use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::ConfigWriteResponse;
use codex_app_server_protocol::HookMetadata;
use codex_app_server_protocol::HookTrustStatus;
use codex_app_server_protocol::MarketplaceAddParams;
use codex_app_server_protocol::MarketplaceAddResponse;
use codex_app_server_protocol::MergeStrategy;
use codex_app_server_protocol::PluginInstallParams;
use codex_app_server_protocol::PluginInstallResponse;
use codex_app_server_protocol::PluginReadParams;
use codex_app_server_protocol::PluginReadResponse;
use codex_app_server_protocol::RequestId;
use codex_core_plugins::installed_marketplaces::marketplace_install_root;
use codex_core_plugins::store::PluginStore;
use codex_plugin::PluginId;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::process::Command;
use uuid::Uuid;

const MARKETPLACE_NAME: &str = "pimpmuckl-dcg";
const PLUGIN_NAME: &str = "destructive-command-guard";
const PLUGIN_ID: &str = "destructive-command-guard@pimpmuckl-dcg";
const PINNED_SOURCE: &str = "https://github.com/Pimpmuckl/destructive_command_guard.git";
const PINNED_TAG: &str = "v0.6.8-codexpp.1";
const PINNED_VERSION: &str = "0.6.8-codexpp.1";
const PINNED_COMMIT: &str = "fd4e90a41fa493aa9a61c715bd5114b7fcf4348e";
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
static OPERATION_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static STATUS_DETECTION_ID: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "windows")]
const BINARY_NAME: &str = "dcg.exe";
#[cfg(not(target_os = "windows"))]
const BINARY_NAME: &str = "dcg";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DcgUnsupportedReason {
    Platform,
    RemoteHookHost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RepairReason {
    MarketplaceConfigMalformed,
    MarketplacePinMismatch,
    MarketplaceUnavailable,
    PluginMissing,
    BinaryMissing,
    BinaryVersionUnreadable,
    HookMissing,
    HookUntrusted,
    HookModified,
    HookDisabled,
    StatusUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DcgStatus {
    Unsupported(DcgUnsupportedReason),
    NotInstalled,
    ExternalInstallation(String),
    Enabled(String),
    Disabled(String),
    UpdateAvailable {
        installed_version: Option<String>,
        target_version: String,
    },
    NeedsRepair(RepairReason),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DcgChange {
    pub(crate) status: DcgStatus,
    pub(crate) takes_effect_in_current_session: bool,
}

pub(crate) struct DcgManager {
    request_handle: AppServerRequestHandle,
    local_codex_home: PathBuf,
    remote_hook_host: bool,
    cwd: PathBuf,
    marketplace_source: String,
    marketplace_ref: Option<String>,
    plugin_id: PluginId,
}

impl DcgManager {
    pub(crate) fn new(app_server: &AppServerSession, config: &Config) -> Result<Self> {
        let app_server_home = app_server
            .codex_home_path(&config.codex_home)
            .context("app-server did not report its Codex home")?;
        Ok(Self {
            request_handle: app_server.request_handle(),
            local_codex_home: app_server_home.as_str().into(),
            remote_hook_host: app_server.uses_remote_workspace(),
            cwd: config.cwd.to_path_buf(),
            marketplace_source: PINNED_SOURCE.to_string(),
            marketplace_ref: Some(PINNED_TAG.to_string()),
            plugin_id: PluginId::parse(PLUGIN_ID)?,
        })
    }

    pub(crate) async fn detect_status(&self) -> DcgStatus {
        if let Some(reason) = self.unsupported_reason() {
            return DcgStatus::Unsupported(reason);
        }
        let marketplace_root = match self.marketplace_root() {
            Ok(Some(root)) => root,
            Ok(None) if self.binary_path().is_file() => {
                return DcgStatus::NeedsRepair(RepairReason::PluginMissing);
            }
            Ok(None) => {
                let Some(binary) = external_binary() else {
                    return DcgStatus::NotInstalled;
                };
                return self
                    .reported_version(&binary)
                    .await
                    .map_or(DcgStatus::NotInstalled, DcgStatus::ExternalInstallation);
            }
            Err(reason) => return DcgStatus::NeedsRepair(reason),
        };
        if !marketplace_root.is_dir() {
            return DcgStatus::NeedsRepair(RepairReason::MarketplaceUnavailable);
        }
        let plugin = match self.read_plugin(&marketplace_root).await {
            Ok(response) => response.plugin.summary,
            Err(_) => return DcgStatus::NeedsRepair(RepairReason::PluginMissing),
        };
        if !plugin.installed {
            return DcgStatus::NeedsRepair(RepairReason::PluginMissing);
        }
        if plugin.local_version.as_deref() != Some(PINNED_VERSION) {
            return DcgStatus::UpdateAvailable {
                installed_version: plugin.local_version,
                target_version: PINNED_VERSION.to_string(),
            };
        }
        let binary = self.binary_path();
        if !binary.is_file() {
            return DcgStatus::NeedsRepair(RepairReason::BinaryMissing);
        }
        let Some(version) = self.reported_version(&binary).await else {
            return DcgStatus::NeedsRepair(RepairReason::BinaryVersionUnreadable);
        };
        if version != PINNED_VERSION {
            return DcgStatus::UpdateAvailable {
                installed_version: Some(version),
                target_version: PINNED_VERSION.to_string(),
            };
        }
        if !plugin.enabled {
            return DcgStatus::Disabled(version);
        }
        let hook = match self.managed_hook().await {
            Ok(Some(hook)) => hook,
            Ok(None) => return DcgStatus::NeedsRepair(RepairReason::HookMissing),
            Err(_) => return DcgStatus::NeedsRepair(RepairReason::StatusUnavailable),
        };
        if !hook.enabled {
            return DcgStatus::NeedsRepair(RepairReason::HookDisabled);
        }
        match hook.trust_status {
            HookTrustStatus::Managed | HookTrustStatus::Trusted => DcgStatus::Enabled(version),
            HookTrustStatus::Untrusted => DcgStatus::NeedsRepair(RepairReason::HookUntrusted),
            HookTrustStatus::Modified => DcgStatus::NeedsRepair(RepairReason::HookModified),
        }
    }

    pub(crate) async fn install_and_enable(&self) -> Result<DcgChange> {
        self.install_managed().await
    }

    pub(crate) fn try_begin_operation() -> bool {
        !OPERATION_IN_FLIGHT.swap(true, Ordering::AcqRel)
    }

    pub(crate) fn finish_operation() {
        OPERATION_IN_FLIGHT.store(false, Ordering::Release);
    }

    pub(crate) fn management_supported(app_server: &AppServerSession) -> bool {
        PLATFORM_SUPPORTED && !app_server.uses_remote_workspace()
    }

    pub(crate) fn begin_status_detection() -> u64 {
        STATUS_DETECTION_ID.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub(crate) fn is_current_status_detection(id: u64) -> bool {
        STATUS_DETECTION_ID.load(Ordering::Acquire) == id
    }

    pub(crate) async fn enable(&self) -> Result<DcgChange> {
        self.set_enabled(/*enabled*/ true).await
    }

    pub(crate) async fn disable(&self) -> Result<DcgChange> {
        self.set_enabled(/*enabled*/ false).await
    }

    pub(crate) async fn update(&self) -> Result<DcgChange> {
        self.install_managed().await
    }

    pub(crate) async fn repair(&self, reason: RepairReason) -> Result<DcgChange> {
        self.ensure_supported()?;
        match reason {
            RepairReason::MarketplaceUnavailable
            | RepairReason::PluginMissing
            | RepairReason::BinaryMissing
            | RepairReason::BinaryVersionUnreadable
            | RepairReason::HookMissing
            | RepairReason::HookUntrusted
            | RepairReason::HookModified => self.install_managed().await,
            RepairReason::MarketplaceConfigMalformed
            | RepairReason::MarketplacePinMismatch
            | RepairReason::HookDisabled
            | RepairReason::StatusUnavailable => {
                bail!("repair is not safe for {reason:?} without external correction")
            }
        }
    }

    async fn install_managed(&self) -> Result<DcgChange> {
        self.ensure_supported()?;
        if let Err(reason) = self.marketplace_root() {
            bail!("cannot install while marketplace state is {reason:?}");
        }
        let marketplace: MarketplaceAddResponse = self
            .request_handle
            .request_typed(ClientRequest::MarketplaceAdd {
                request_id: request_id("dcg-marketplace-add"),
                params: MarketplaceAddParams {
                    source: self.marketplace_source.clone(),
                    ref_name: self.marketplace_ref.clone(),
                    sparse_paths: None,
                },
            })
            .await
            .context("failed to add the pinned DCG marketplace")?;
        if marketplace.marketplace_name != MARKETPLACE_NAME {
            bail!("pinned checkout exposed an unexpected marketplace name");
        }
        self.verify_marketplace_checkout(marketplace.installed_root.as_path())
            .await?;
        let plugin_was_enabled = self
            .read_plugin(marketplace.installed_root.as_path())
            .await?
            .plugin
            .summary
            .enabled;
        let binary = self.binary_path();
        let data_root = binary.parent().context("DCG binary path has no parent")?;
        tokio::fs::create_dir_all(data_root).await?;
        let backup = if binary.is_file() {
            let dir = tempfile::tempdir_in(data_root)?;
            let path = dir.path().join(BINARY_NAME);
            tokio::fs::copy(&binary, &path).await?;
            Some((dir, path))
        } else {
            None
        };
        let manifest = marketplace
            .installed_root
            .join(".agents/plugins/marketplace.json");
        let _: PluginInstallResponse = self
            .request_handle
            .request_typed(ClientRequest::PluginInstall {
                request_id: request_id("dcg-plugin-install"),
                params: PluginInstallParams {
                    marketplace_path: Some(manifest),
                    remote_marketplace_name: None,
                    plugin_name: PLUGIN_NAME.to_string(),
                },
            })
            .await
            .context("failed to install the pinned DCG plugin")?;
        let result = async {
            self.run_installer(marketplace.installed_root.as_path(), data_root)
                .await?;
            self.verify_binary().await?;
            self.trust_managed_hook().await
        }
        .await;
        if let Err(err) = result {
            let rollback = match &backup {
                Some((_, path)) => tokio::fs::copy(path, &binary).await.map(|_| ()),
                None => match tokio::fs::remove_file(&binary).await {
                    Ok(()) => Ok(()),
                    Err(remove_err) if remove_err.kind() == ErrorKind::NotFound => Ok(()),
                    Err(remove_err) => Err(remove_err),
                },
            };
            let enablement_rollback = self.set_enabled(plugin_was_enabled).await;
            if let Err(rollback_err) = rollback {
                bail!("{err:#}; binary rollback also failed: {rollback_err}");
            }
            if let Err(rollback_err) = enablement_rollback {
                bail!("{err:#}; plugin enablement rollback also failed: {rollback_err:#}");
            }
            return Err(err);
        }
        Ok(DcgChange {
            status: DcgStatus::Enabled(PINNED_VERSION.to_string()),
            takes_effect_in_current_session: false,
        })
    }

    async fn run_installer(&self, root: &Path, data_root: &Path) -> Result<()> {
        #[cfg(target_os = "windows")]
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive"])
            .args(["-ExecutionPolicy", "Bypass", "-File"])
            .arg(root.join("install.ps1"))
            .args(["-Owner", "Pimpmuckl", "-Repo", "destructive_command_guard"])
            .args(["-Version", PINNED_TAG, "-Dest"])
            .arg(data_root)
            .args(["-NoConfigure", "-Verify"])
            .output()
            .await?;
        #[cfg(not(target_os = "windows"))]
        let output = Command::new("bash")
            .arg(root.join("install.sh"))
            .args(["--version", PINNED_TAG, "--dest"])
            .arg(data_root)
            .args(["--no-configure", "--verify"])
            .env("OWNER", "Pimpmuckl")
            .env("REPO", "destructive_command_guard")
            .output()
            .await?;
        if !output.status.success() {
            bail!(
                "pinned DCG installer failed with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    async fn verify_marketplace_checkout(&self, root: &Path) -> Result<()> {
        if self.marketplace_ref.is_none() {
            return Ok(());
        }
        let output = Command::new("git")
            .args(["-C", &root.to_string_lossy(), "status"])
            .args(["--porcelain=v2", "--branch", "--untracked-files=all"])
            .output()
            .await?;
        let status = String::from_utf8_lossy(&output.stdout);
        let expected_oid = format!("# branch.oid {PINNED_COMMIT}");
        if !output.status.success()
            || !status.lines().any(|line| line == expected_oid)
            || status.lines().any(|line| !line.starts_with("# "))
        {
            bail!("pinned DCG marketplace checkout is not the clean {PINNED_COMMIT} tree");
        }
        Ok(())
    }

    async fn verify_binary(&self) -> Result<()> {
        let version = self
            .reported_version(&self.binary_path())
            .await
            .context("managed DCG binary did not report a version")?;
        if version != PINNED_VERSION {
            bail!("managed DCG binary reported {version}, expected {PINNED_VERSION}");
        }
        Ok(())
    }

    async fn trust_managed_hook(&self) -> Result<()> {
        let mut hook = None;
        for _ in 0..100 {
            hook = self.managed_hook().await?;
            if hook.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let hook = hook.context("installed DCG plugin hook was not discovered")?;
        if !hook.enabled {
            bail!("installed DCG plugin hook is disabled");
        }
        let key = hook.key;
        let key_path = format!("hooks.state.{}", serde_json::to_string(&key)?);
        let prior_state = std::fs::read_to_string(self.local_codex_home.join("config.toml"))
            .ok()
            .and_then(|contents| toml::from_str::<toml::Value>(&contents).ok())
            .and_then(|config| config.get("hooks")?.get("state")?.get(&key).cloned());
        let response = self
            .write_config_without_reload(replace_config_value(
                key_path.clone(),
                serde_json::json!({ "trusted_hash": hook.current_hash }),
            ))
            .await?;
        if response.status != codex_app_server_protocol::WriteStatus::Ok {
            let prior_state =
                prior_state.map_or(Ok(serde_json::Value::Null), serde_json::to_value)?;
            write_config_batch(
                self.request_handle.clone(),
                vec![replace_config_value(key_path, prior_state)],
            )
            .await
            .map_err(|err| anyhow::anyhow!("{err:#}"))?;
            bail!("installed DCG hook trust is overridden by managed configuration");
        }
        Ok(())
    }

    async fn managed_hook(&self) -> Result<Option<HookMetadata>> {
        let response = fetch_hooks_list(self.request_handle.clone(), self.cwd.clone())
            .await
            .map_err(|err| anyhow::anyhow!("{err:#}"))?;
        let entry = hooks_list_entry_for_cwd(response, &self.cwd);
        if !entry.errors.is_empty() {
            bail!("hook discovery reported errors");
        }
        Ok(entry.hooks.into_iter().find(|hook| {
            hook.plugin_id.as_deref() == Some(PLUGIN_ID)
                && hook
                    .source_path
                    .as_path()
                    .components()
                    .any(|part| part.as_os_str() == PINNED_VERSION)
        }))
    }

    async fn read_plugin(&self, root: &Path) -> Result<PluginReadResponse> {
        let marketplace_path =
            AbsolutePathBuf::try_from(root.join(".agents/plugins/marketplace.json"))?;
        self.request_handle
            .request_typed(ClientRequest::PluginRead {
                request_id: request_id("dcg-plugin-read"),
                params: PluginReadParams {
                    marketplace_path: Some(marketplace_path),
                    remote_marketplace_name: None,
                    plugin_name: PLUGIN_NAME.to_string(),
                },
            })
            .await
            .context("failed to inspect the managed DCG plugin")
    }

    async fn set_enabled(&self, enabled: bool) -> Result<DcgChange> {
        self.ensure_supported()?;
        self.write_config_without_reload(ConfigEdit {
            key_path: format!("plugins.{PLUGIN_ID}"),
            value: serde_json::json!({ "enabled": enabled }),
            merge_strategy: MergeStrategy::Upsert,
        })
        .await?;
        Ok(DcgChange {
            status: self.detect_status().await,
            takes_effect_in_current_session: false,
        })
    }

    async fn write_config_without_reload(&self, edit: ConfigEdit) -> Result<ConfigWriteResponse> {
        Ok(self
            .request_handle
            .request_typed(ClientRequest::ConfigBatchWrite {
                request_id: request_id("dcg-config-write"),
                params: codex_app_server_protocol::ConfigBatchWriteParams {
                    edits: vec![edit],
                    file_path: None,
                    expected_version: None,
                    reload_user_config: false,
                },
            })
            .await?)
    }

    fn marketplace_root(&self) -> std::result::Result<Option<PathBuf>, RepairReason> {
        let home = &self.local_codex_home;
        let contents = match std::fs::read_to_string(home.join("config.toml")) {
            Ok(contents) => contents,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(RepairReason::MarketplaceConfigMalformed),
        };
        let config = toml::from_str::<toml::Value>(&contents)
            .map_err(|_| RepairReason::MarketplaceConfigMalformed)?;
        let Some(marketplace) = config
            .get("marketplaces")
            .and_then(toml::Value::as_table)
            .and_then(|marketplaces| marketplaces.get(MARKETPLACE_NAME))
        else {
            return Ok(None);
        };
        let expected_type = self.marketplace_ref.as_ref().map_or("local", |_| "git");
        if marketplace.get("source_type").and_then(toml::Value::as_str) != Some(expected_type)
            || marketplace.get("source").and_then(toml::Value::as_str)
                != Some(self.marketplace_source.as_str())
            || marketplace.get("ref").and_then(toml::Value::as_str)
                != self.marketplace_ref.as_deref()
        {
            return Err(RepairReason::MarketplacePinMismatch);
        }
        Ok(Some(if expected_type == "local" {
            PathBuf::from(&self.marketplace_source)
        } else {
            marketplace_install_root(home).join(MARKETPLACE_NAME)
        }))
    }

    async fn reported_version(&self, binary: &Path) -> Option<String> {
        let mut command = Command::new(binary);
        command.arg("--version").kill_on_drop(true);
        let output = tokio::time::timeout(VERSION_PROBE_TIMEOUT, command.output())
            .await
            .ok()?
            .ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8(output.stdout)
            .ok()?
            .split_whitespace()
            .find(|token| token.contains('.'))
            .map(str::to_string)
    }

    fn binary_path(&self) -> PathBuf {
        PluginStore::new(self.local_codex_home.clone())
            .plugin_data_root(&self.plugin_id)
            .join(BINARY_NAME)
            .as_path()
            .to_path_buf()
    }

    fn unsupported_reason(&self) -> Option<DcgUnsupportedReason> {
        if !PLATFORM_SUPPORTED {
            return Some(DcgUnsupportedReason::Platform);
        }
        self.remote_hook_host
            .then_some(DcgUnsupportedReason::RemoteHookHost)
    }

    fn ensure_supported(&self) -> Result<()> {
        if let Some(reason) = self.unsupported_reason() {
            bail!("DCG management is unsupported for {reason:?}");
        }
        Ok(())
    }
}

fn external_binary() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let mut candidates = std::env::split_paths(&path).map(|directory| directory.join(BINARY_NAME));
    #[cfg(unix)]
    return candidates.find(|candidate| {
        candidate
            .metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    });
    #[cfg(not(unix))]
    candidates.find(|candidate| candidate.is_file())
}

fn request_id(prefix: &str) -> RequestId {
    RequestId::String(format!("{prefix}-{}", Uuid::new_v4()))
}

const PLATFORM_SUPPORTED: bool = cfg!(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "windows", target_arch = "x86_64")
));

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
