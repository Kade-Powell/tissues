use clap::Parser;
use color_eyre::eyre::Result;
use worktrack::{app::App, github::GitHubClient, repo, tui};

#[derive(Debug, Parser)]
#[command(name = "worktrack")]
#[command(about = "A Ratatui work tracker for one GitHub repository")]
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
    let result = tui::run(&mut terminal, &mut app, &client).await;
    ratatui::restore();
    result
}
