use super::*;
use pretty_assertions::assert_eq;

#[derive(Clone, Copy)]
enum Outcome {
    Ok,
    Err(&'static str),
}

impl Outcome {
    fn result(self) -> anyhow::Result<()> {
        match self {
            Self::Ok => Ok(()),
            Self::Err(message) => anyhow::bail!("{message}"),
        }
    }
}

#[derive(Clone, Copy)]
enum Start {
    Package,
    AlreadyUpstream,
}

struct FakeAdapter {
    preflight: Option<Preflight>,
    handoff: ProfileHandoff,
    install: Outcome,
    verify: Outcome,
    rollback: Outcome,
    events: Vec<&'static str>,
}

impl SwitchAdapter for FakeAdapter {
    async fn preflight(&mut self) -> anyhow::Result<Preflight> {
        self.events.push("preflight");
        Ok(self.preflight.take().expect("one preflight"))
    }

    fn export_selected_profile(&mut self) -> anyhow::Result<ProfileHandoff> {
        self.events.push("handoff");
        Ok(self.handoff)
    }

    fn install_upstream(&mut self, _preflight: &Preflight) -> anyhow::Result<()> {
        self.events.push("install");
        self.install.result()
    }

    fn verify_upstream(&mut self, _preflight: &Preflight) -> anyhow::Result<()> {
        self.events.push("verify");
        self.verify.result()
    }

    fn rollback_fork(&mut self, _preflight: &Preflight) -> anyhow::Result<()> {
        self.events.push("rollback");
        self.rollback.result()
    }

    fn reconcile_root_auth(&mut self) -> anyhow::Result<()> {
        self.events.push("reconcile");
        Ok(())
    }
}

struct Case {
    name: &'static str,
    start: Start,
    handoff: ProfileHandoff,
    install: Outcome,
    verify: Outcome,
    rollback: Outcome,
    expected_ok: bool,
    expected_events: &'static [&'static str],
    stdout: &'static str,
    error: Option<&'static str>,
}

fn package_preflight() -> Preflight {
    Preflight::Package(PackageSwitch {
        manager: PackageManager::Npm,
        upstream: VerifiedPackageArtifact {
            package: "@openai/codex",
            version: "0.145.0".to_string(),
        },
        rollback: VerifiedPackageArtifact {
            package: "@jjliebig/codex-plus-plus",
            version: "0.144.4-fork.1".to_string(),
        },
    })
}

#[tokio::test]
async fn switch_transaction_handles_success_noop_failures_and_auth_absence() {
    let cases = [
        Case {
            name: "success",
            start: Start::Package,
            handoff: ProfileHandoff::Selected,
            install: Outcome::Ok,
            verify: Outcome::Ok,
            rollback: Outcome::Ok,
            expected_ok: true,
            expected_events: &["preflight", "handoff", "install", "verify"],
            stdout: "Switched to upstream Codex 0.145.0.\n",
            error: None,
        },
        Case {
            name: "already upstream",
            start: Start::AlreadyUpstream,
            handoff: ProfileHandoff::Selected,
            install: Outcome::Ok,
            verify: Outcome::Ok,
            rollback: Outcome::Ok,
            expected_ok: true,
            expected_events: &["preflight"],
            stdout: "Already using upstream Codex.\n",
            error: None,
        },
        Case {
            name: "install failure rolls back",
            start: Start::Package,
            handoff: ProfileHandoff::Selected,
            install: Outcome::Err("install failed"),
            verify: Outcome::Ok,
            rollback: Outcome::Ok,
            expected_ok: false,
            expected_events: &["preflight", "handoff", "install", "rollback", "reconcile"],
            stdout: "",
            error: Some("Codex++ was restored"),
        },
        Case {
            name: "verification failure rolls back",
            start: Start::Package,
            handoff: ProfileHandoff::Selected,
            install: Outcome::Ok,
            verify: Outcome::Err("verification failed"),
            rollback: Outcome::Ok,
            expected_ok: false,
            expected_events: &[
                "preflight",
                "handoff",
                "install",
                "verify",
                "rollback",
                "reconcile",
            ],
            stdout: "",
            error: Some("Codex++ was restored"),
        },
        Case {
            name: "rollback failure is reported",
            start: Start::Package,
            handoff: ProfileHandoff::Selected,
            install: Outcome::Err("install failed"),
            verify: Outcome::Ok,
            rollback: Outcome::Err("rollback failed"),
            expected_ok: false,
            expected_events: &["preflight", "handoff", "install", "rollback", "reconcile"],
            stdout: "",
            error: Some("rollback failed"),
        },
        Case {
            name: "no profile still switches",
            start: Start::Package,
            handoff: ProfileHandoff::NoUsableProfile,
            install: Outcome::Ok,
            verify: Outcome::Ok,
            rollback: Outcome::Ok,
            expected_ok: true,
            expected_events: &["preflight", "handoff", "install", "verify"],
            stdout: concat!(
                "Switched to upstream Codex 0.145.0.\n",
                "No usable Codex++ profile was selected; run `codex login` to sign in.\n"
            ),
            error: None,
        },
    ];

    for case in cases {
        let mut adapter = FakeAdapter {
            preflight: Some(match case.start {
                Start::Package => package_preflight(),
                Start::AlreadyUpstream => Preflight::AlreadyUpstream,
            }),
            handoff: case.handoff,
            install: case.install,
            verify: case.verify,
            rollback: case.rollback,
            events: Vec::new(),
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let result = run_with_adapter(&mut adapter, &mut stdout, &mut stderr).await;

        assert_eq!(result.is_ok(), case.expected_ok, "{}", case.name);
        assert_eq!(adapter.events, case.expected_events, "{}", case.name);
        assert_eq!(
            String::from_utf8(stdout).expect("stdout"),
            case.stdout,
            "{}",
            case.name
        );
        if let Some(expected) = case.error {
            assert!(
                result.expect_err(case.name).to_string().contains(expected),
                "{}",
                case.name
            );
        }
        let stderr = String::from_utf8(stderr).expect("stderr");
        assert!(stderr.starts_with("Resolving upstream and rollback artifacts...\n"));
    }
}

#[test]
fn package_manager_roots_cover_npm_pnpm_and_bun() {
    for (manager, output, expected) in [
        (PackageManager::Npm, "npm", "npm"),
        (PackageManager::Pnpm, "pnpm", "pnpm"),
        (
            PackageManager::Bun,
            "bun/bin",
            "bun/install/global/node_modules",
        ),
    ] {
        assert_eq!(
            manager_global_root(manager, output, None).expect("manager root"),
            PathBuf::from(expected)
        );
    }
}
