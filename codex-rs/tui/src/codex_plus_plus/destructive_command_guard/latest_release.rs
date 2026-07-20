use anyhow::Context;
use anyhow::Result;
use codex_login::default_client::create_client;
use std::time::Duration;
use tokio::process::Command;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DcgTarget {
    pub(super) tag: String,
    pub(super) version: String,
    pub(super) precedence: [u64; 4],
    pub(super) commit: String,
}
impl DcgTarget {
    pub(super) fn from_tag(tag: &str) -> Option<Self> {
        let version = tag.strip_prefix('v')?;
        let (base, fork) = version.split_once("-codexpp.")?;
        let mut parts = base.split('.').chain(std::iter::once(fork));
        let mut next = || parts.next()?.parse::<u64>().ok();
        let precedence = [next()?, next()?, next()?, next()?];
        parts.next().is_none().then_some(Self {
            tag: tag.to_string(),
            version: version.to_string(),
            precedence,
            commit: String::new(),
        })
    }
    pub(super) fn from_release(release: &serde_json::Value) -> Option<Self> {
        (!release["draft"].as_bool()? && !release["prerelease"].as_bool()?)
            .then(|| release["tag_name"].as_str())
            .flatten()
            .and_then(Self::from_tag)
    }
}
pub(super) async fn resolve(source: &str) -> Result<DcgTarget> {
    let release = create_client()
        .get("https://api.github.com/repos/Pimpmuckl/destructive_command_guard/releases/latest")
        .timeout(Duration::from_secs(5))
        .send()
        .await?
        .error_for_status()?
        .json::<serde_json::Value>()
        .await?;
    let mut target = DcgTarget::from_release(&release)
        .context("latest DCG release is not an eligible vX.Y.Z-codexpp.N release")?;
    let mut command = Command::new("git");
    command
        .args(["ls-remote", source])
        .args([
            format!("refs/tags/{}", target.tag),
            format!("refs/tags/{}^{{}}", target.tag),
        ])
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(5), command.output())
        .await
        .context("timed out resolving the DCG release tag")??;
    target.commit = String::from_utf8(output.stdout)?
        .lines()
        .last()
        .and_then(|line| line.split_once('\t'))
        .map(|(sha, _)| sha.to_string())
        .filter(|sha| output.status.success() && sha.len() == 40)
        .context("resolved DCG release tag did not identify an immutable commit")?;
    Ok(target)
}
