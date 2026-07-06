use anyhow::Context;
use clap::Args;
use clap::Parser;
use codex_core::config::Config;
use codex_login::AccountProfile;
use codex_login::AccountStore;
use codex_utils_cli::CliConfigOverrides;

#[derive(Debug, Parser)]
pub(crate) struct AccountCli {
    #[clap(skip)]
    pub config_overrides: CliConfigOverrides,

    #[command(subcommand)]
    pub subcommand: AccountSubcommand,
}

#[derive(Debug, clap::Subcommand)]
pub(crate) enum AccountSubcommand {
    /// Import the current ChatGPT login as an account.
    ImportCurrent(ImportCurrentArgs),

    /// List imported accounts.
    List,
}

#[derive(Debug, Args)]
pub(crate) struct ImportCurrentArgs {
    pub label: String,
}

pub(crate) async fn run_account_command(account_cli: AccountCli) -> anyhow::Result<()> {
    let config = load_config(account_cli.config_overrides).await?;
    let store = AccountStore::new(config.codex_home.clone());

    match account_cli.subcommand {
        AccountSubcommand::ImportCurrent(args) => {
            let profile = store
                .import_current(args.label, config.cli_auth_credentials_store_mode)
                .context("failed to import current account")?;
            println!("Imported account {} ({})", profile.id, profile.label);
        }
        AccountSubcommand::List => {
            print_accounts(store.list().context("failed to list accounts")?);
        }
    }
    Ok(())
}

async fn load_config(config_overrides: CliConfigOverrides) -> anyhow::Result<Config> {
    let overrides = config_overrides
        .parse_overrides()
        .map_err(|err| anyhow::anyhow!("error parsing -c overrides: {err}"))?;
    Config::load_with_cli_overrides(overrides)
        .await
        .context("error loading configuration")
}

fn print_accounts(accounts: Vec<AccountProfile>) {
    if accounts.is_empty() {
        println!("No accounts imported.");
        return;
    }

    println!("ID  Label  Enabled  Priority");
    for account in accounts {
        println!(
            "{}  {}  {}  {}",
            account.id, account.label, account.enabled, account.priority
        );
    }
}
