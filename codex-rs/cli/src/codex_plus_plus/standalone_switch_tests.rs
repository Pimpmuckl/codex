use super::*;
use crate::codex_plus_plus::upstream_switch::GithubAsset;
use codex_install_context::codex_plus_plus::UpdateChannel;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

pub(in crate::codex_plus_plus) fn fake_switch() -> StandaloneSwitch {
    let path = PathBuf::new();
    StandaloneSwitch {
        action: UpdateAction::StandaloneUnix,
        upstream_version: "0.145.0".to_string(),
        visible_exe: path.clone(),
        rollback: Rollback {
            generation: "generation".to_string(),
            binary: path.clone(),
            binary_sha256: String::new(),
            pointer: path.clone(),
            shim: FileSnapshot {
                path,
                contents: Vec::new(),
                permissions: fs::metadata(".").expect("workspace metadata").permissions(),
            },
            windows_user_path: None,
        },
    }
}

fn valid_release() -> GithubRelease {
    let digest = format!("sha256:{}", "a".repeat(64));
    GithubRelease {
        tag_name: "rust-v0.145.0".to_string(),
        draft: false,
        prerelease: false,
        assets: vec![
            GithubAsset {
                name: "codex-package-x86_64-unknown-linux-musl.tar.gz".to_string(),
                digest: Some(digest.clone()),
            },
            GithubAsset {
                name: "codex-package_SHA256SUMS".to_string(),
                digest: Some(digest),
            },
        ],
    }
}

#[test]
fn release_preflight_requires_target_and_checksum_digests() {
    let mut release = valid_release();

    assert!(asset_sha256(&release, "codex-package-x86_64-unknown-linux-musl.tar.gz").is_ok());
    release.assets[1].digest = None;
    assert_eq!(
        asset_sha256(&release, "codex-package_SHA256SUMS")
            .expect_err("missing checksum digest")
            .to_string(),
        "upstream release is missing SHA-256 metadata for codex-package_SHA256SUMS"
    );
}

#[test]
fn windows_user_path_prioritizes_upstream_and_removes_the_fork() {
    assert_eq!(
        normalized_windows_user_path(
            r"C:\Fork;C:\Tools;C:\Upstream",
            Path::new(r"c:\upstream"),
            Path::new(r"c:\fork"),
        ),
        r"c:\upstream;C:\Tools"
    );
}

#[tokio::test]
async fn valid_install_contexts_preflight_fork_and_upstream_provenance() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let generation = "20260715T1200000000000Z";
    let release_dir = temp.path().join(generation);
    let shim_dir = temp.path().join("shim");
    fs::create_dir_all(release_dir.join("bin"))?;
    fs::create_dir_all(&shim_dir)?;
    fs::write(release_dir.join("bin/codex"), b"preserved fork")?;
    fs::write(shim_dir.join("codex"), b"fork shim")?;
    fs::write(shim_dir.join(".codex-plus-plus-current"), generation)?;
    let context = InstallContext {
        method: InstallMethod::Standalone {
            release_dir: AbsolutePathBuf::from_absolute_path(&release_dir)?,
            resources_dir: None,
            platform: StandalonePlatform::Unix,
        },
        package_layout: None,
    };
    let plan = UpdatePlan::for_install_context(&context, UpdateChannel::Upstream);

    for (version, expected) in [("0.144.4-fork.1", Some("0.145.0")), ("0.145.0", None)] {
        fs::write(
            release_dir.join("codex-package.json"),
            format!(r#"{{"version":"{version}","target":"x86_64-unknown-linux-musl"}}"#),
        )?;
        let switch = preflight_with_sources(
            &context,
            plan,
            || Ok(shim_dir.clone()),
            |_| std::future::ready(Ok((valid_release(), "rust-v"))),
        )
        .await?;

        assert_eq!(
            switch.map(|switch| switch.upstream_version),
            expected.map(str::to_string)
        );
    }
    Ok(())
}

#[tokio::test]
async fn preflight_rejects_missing_package_metadata_before_mutation() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let context = InstallContext {
        method: InstallMethod::Standalone {
            release_dir: AbsolutePathBuf::from_absolute_path(temp.path())?,
            resources_dir: None,
            platform: StandalonePlatform::Unix,
        },
        package_layout: None,
    };
    let plan = UpdatePlan::for_install_context(&context, UpdateChannel::Upstream);

    let error = preflight(&context, plan)
        .await
        .err()
        .expect("missing package metadata");
    assert!(error.to_string().contains("failed to read"));
    Ok(())
}

#[test]
fn rollback_restores_the_shim_and_rejects_a_corrupt_generation() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let generation = "20260715T1200000000000Z";
    let release_dir = temp.path().join(generation);
    let shim_dir = temp.path().join("shim");
    let platform = if cfg!(windows) {
        StandalonePlatform::Windows
    } else {
        StandalonePlatform::Unix
    };
    let (entrypoint, target, shim_name) = match platform {
        StandalonePlatform::Unix => ("bin/codex", "x86_64-unknown-linux-musl", "codex"),
        StandalonePlatform::Windows => ("bin/codex.exe", "x86_64-pc-windows-msvc", "codex.cmd"),
    };
    fs::create_dir_all(release_dir.join("bin"))?;
    fs::create_dir_all(&shim_dir)?;
    let binary = release_dir.join(entrypoint);
    let shim = shim_dir.join(shim_name);
    fs::write(&binary, b"preserved fork")?;
    fs::write(&shim, b"fork shim")?;
    fs::write(shim_dir.join(".codex-plus-plus-current"), generation)?;
    let rollback = local_rollback(
        &release_dir,
        platform,
        &shim_dir,
        &PackageMetadata {
            version: "0.144.4-fork.1".to_string(),
            target: target.to_string(),
        },
    )?;
    let switch = StandaloneSwitch {
        action: match platform {
            StandalonePlatform::Unix => UpdateAction::StandaloneUnix,
            StandalonePlatform::Windows => UpdateAction::StandaloneWindows,
        },
        upstream_version: "0.145.0".to_string(),
        visible_exe: PathBuf::new(),
        rollback,
    };

    fs::write(&shim, b"upstream shim")?;
    rollback_fork(&switch)?;
    assert_eq!(fs::read(&shim)?, b"fork shim");

    fs::write(binary, b"corrupt fork")?;
    assert!(
        rollback_fork(&switch)
            .expect_err("corrupt generation")
            .to_string()
            .contains("generation digest changed")
    );
    Ok(())
}

#[test]
fn local_preflight_rejects_an_unsupported_target_before_mutation() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let metadata = PackageMetadata {
        version: "0.144.4-fork.1".to_string(),
        target: "aarch64-unknown-linux-musl".to_string(),
    };
    let error = local_rollback(
        temp.path(),
        StandalonePlatform::Unix,
        temp.path(),
        &metadata,
    )
    .err()
    .expect("unsupported target");
    assert_eq!(
        error.to_string(),
        "unsupported Codex++ standalone target: aarch64-unknown-linux-musl"
    );

    let metadata = PackageMetadata {
        version: metadata.version,
        target: "x86_64-unknown-linux-musl".to_string(),
    };
    let error = local_rollback(
        temp.path(),
        StandalonePlatform::Unix,
        temp.path(),
        &metadata,
    )
    .err()
    .expect("missing generation pointer");
    assert!(error.to_string().contains("generation pointer is missing"));
    Ok(())
}
