use clap::Parser;
use color_eyre::eyre::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
};
use skunkwork::{app::App, github::GitHubClient, repo, tui};
use std::io::stdout;

#[derive(Debug, Parser)]
#[command(name = "skunkwork")]
#[command(version)]
#[command(about = "A Ratatui issue tracker for one GitHub repository")]
struct Args {
    /// Repository to open, formatted as owner/repo. If omitted, gh repo view is used.
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
    let client = GitHubClient::from_gh_cli()?;
    let mut app = App::new(repository);
    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture)?;
    let result = tui::run(&mut terminal, &mut app, &client).await;
    let _ = execute!(stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}
