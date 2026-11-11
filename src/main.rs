mod app;
mod catalog;
mod document;
mod editor;
mod fields;
mod profile;
mod storage;
#[cfg(test)]
mod tests;
mod ui;

use std::{
    io::{self, IsTerminal},
    path::PathBuf,
};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use profile::{Profiles, Provider, expand_path, home_dir, registry_path};

#[derive(Parser)]
#[command(version, about = "Claude Code / Codex の設定を編集するTUI")]
struct Cli {
    #[arg(long, global = true, help = "プロファイル一覧の保存先")]
    registry: Option<PathBuf>,
    #[arg(long, help = "起動時に選択する登録済みプロファイル名")]
    profile: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "プロファイルを登録・一覧表示・解除")]
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
}

#[derive(Subcommand)]
enum ProfileCommand {
    List,
    Add {
        #[arg(long)]
        name: String,
        #[arg(long, value_enum)]
        provider: Option<Provider>,
        #[arg(long)]
        directory: PathBuf,
    },
    Remove {
        name: String,
    },
    #[command(about = "登録済みプロファイルの種類を変更")]
    SetProvider {
        name: String,
        #[arg(long, value_enum)]
        provider: Provider,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let home = home_dir()?;
    let path = expand_path(&cli.registry.unwrap_or_else(|| registry_path(&home)), &home)?;
    let mut profiles = Profiles::load(path, &home)?;
    if let Some(Command::Profile { command }) = cli.command {
        match command {
            ProfileCommand::List => {
                for index in profiles.display_order() {
                    let profile = &profiles.items[index];
                    println!(
                        "{}\t{}\t{}",
                        profile.name,
                        profile.provider.label(),
                        profile.directory.display()
                    );
                    if let Err(error) = profiles.validate(index) {
                        eprintln!("{}: {error:#}", profile.name);
                    }
                }
            }
            ProfileCommand::Add {
                name,
                provider,
                directory,
            } => {
                let directory = expand_path(&directory, &home)?;
                profiles.add(name, Provider::resolve(provider, &directory)?, directory)?;
                println!("プロファイルを登録しました");
            }
            ProfileCommand::Remove { name } => {
                let index = profiles
                    .items
                    .iter()
                    .position(|item| item.name == name)
                    .ok_or_else(|| anyhow::anyhow!("プロファイルが見つかりません"))?;
                profiles.remove(index)?;
                println!("登録を解除しました（設定ファイルは保持されます）");
            }
            ProfileCommand::SetProvider { name, provider } => {
                let index = profiles
                    .items
                    .iter()
                    .position(|item| item.name == name)
                    .ok_or_else(|| anyhow::anyhow!("プロファイルが見つかりません"))?;
                profiles.set_provider(index, provider)?;
                println!("プロファイルの種類を{}に変更しました", provider.label());
            }
        }
        return Ok(());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!(
            "TUIは対話端末から起動してください。登録一覧は `ai-config-tui profile list` で確認できます"
        );
    }
    let mut app = app::App::new(profiles, home, cli.profile.as_deref())?;
    let mut terminal = ratatui::init();
    let result = app.run(&mut terminal);
    ratatui::restore();
    result
}
