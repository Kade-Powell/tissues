use clap::Parser;
use color_eyre::eyre::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
};
use std::io::stdout;
use tissues::{app::App, config::AppConfig, github::GitHubClient, repo, tui};

#[derive(Debug, Parser)]
#[command(name = "tissues")]
#[command(version)]
#[command(about = "A Ratatui issue tracker for one GitHub repository")]
struct Args {
    /// Repository to open, formatted as owner/repo or a GitHub remote URL.
    /// If omitted, gh repo view or git remote origin is used.
    repo: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let repository = match args.repo {
        Some(repo) => repo.parse()?,
        None => repo::infer_current_repo()?,
    };
    let config = AppConfig::load();
    let client = GitHubClient::from_gh_cli_user(config.auth.gh_user.as_deref())?;
    let mut app = App::new(repository);
    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture)?;
    let result = tui::run(&mut terminal, &mut app, &client).await;
    let _ = execute!(stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}
