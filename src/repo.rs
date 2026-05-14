use std::{fmt, str::FromStr};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::Deserialize;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Repository {
    pub owner: String,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepoParseError {
    MissingOwner,
    MissingName,
    ExtraSeparator,
}

impl fmt::Display for RepoParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingOwner => write!(f, "repository owner is missing"),
            Self::MissingName => write!(f, "repository name is missing"),
            Self::ExtraSeparator => write!(f, "repository must be formatted as owner/repo"),
        }
    }
}

impl std::error::Error for RepoParseError {}

impl FromStr for Repository {
    type Err = RepoParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = normalize_repository_input(value);
        let mut parts = value.split('/');
        let owner = parts.next().unwrap_or_default().trim();
        let name = parts
            .next()
            .unwrap_or_default()
            .trim()
            .trim_end_matches(".git");

        if parts.next().is_some() {
            return Err(RepoParseError::ExtraSeparator);
        }

        if owner.is_empty() {
            return Err(RepoParseError::MissingOwner);
        }

        if name.is_empty() {
            return Err(RepoParseError::MissingName);
        }

        Ok(Self {
            owner: owner.to_string(),
            name: name.to_string(),
        })
    }
}

fn normalize_repository_input(value: &str) -> String {
    let value = value.trim().trim_end_matches('/');
    for prefix in [
        "git@github.com:",
        "ssh://git@github.com/",
        "https://github.com/",
        "http://github.com/",
    ] {
        if let Some(repo) = value.strip_prefix(prefix) {
            return repo.to_string();
        }
    }
    value.to_string()
}

impl fmt::Display for Repository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

pub fn infer_current_repo() -> Result<Repository> {
    if let Ok(repo) = infer_current_repo_from_gh() {
        return Ok(repo);
    }

    if let Ok(repo) = infer_current_repo_from_git_remote() {
        return Ok(repo);
    }

    Err(eyre!(
        "could not infer repository; pass owner/repo or a GitHub remote URL, or run inside a GitHub checkout"
    ))
}

fn infer_current_repo_from_gh() -> Result<Repository> {
    let output = std::process::Command::new("gh")
        .args(["repo", "view", "--json", "nameWithOwner"])
        .output()
        .wrap_err("failed to run `gh repo view --json nameWithOwner`")?;

    if !output.status.success() {
        return Err(eyre!("`gh repo view --json nameWithOwner` failed"));
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RepoView {
        name_with_owner: String,
    }

    let view: RepoView =
        serde_json::from_slice(&output.stdout).wrap_err("failed to parse `gh repo view` output")?;
    view.name_with_owner.parse().map_err(Into::into)
}

fn infer_current_repo_from_git_remote() -> Result<Repository> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
        .wrap_err("failed to run `git remote get-url origin`")?;

    if !output.status.success() {
        return Err(eyre!("`git remote get-url origin` failed"));
    }

    let remote = String::from_utf8_lossy(&output.stdout);
    remote.trim().parse().map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_owner_and_repo() {
        let repo: Repository = "owner/tissue".parse().unwrap();

        assert_eq!(repo.owner, "owner");
        assert_eq!(repo.name, "tissue");
        assert_eq!(repo.to_string(), "owner/tissue");
    }

    #[test]
    fn parses_github_ssh_remote_url() {
        let repo: Repository = "git@github.com:Kade-Powell/tissues.git".parse().unwrap();

        assert_eq!(repo.owner, "Kade-Powell");
        assert_eq!(repo.name, "tissues");
        assert_eq!(repo.to_string(), "Kade-Powell/tissues");
    }

    #[test]
    fn parses_github_https_remote_url() {
        let repo: Repository = "https://github.com/Kade-Powell/tissues.git"
            .parse()
            .unwrap();

        assert_eq!(repo.owner, "Kade-Powell");
        assert_eq!(repo.name, "tissues");
        assert_eq!(repo.to_string(), "Kade-Powell/tissues");
    }

    #[test]
    fn rejects_missing_owner() {
        assert_eq!(
            "/tissue".parse::<Repository>().unwrap_err(),
            RepoParseError::MissingOwner
        );
    }

    #[test]
    fn rejects_missing_name() {
        assert_eq!(
            "owner/".parse::<Repository>().unwrap_err(),
            RepoParseError::MissingName
        );
    }

    #[test]
    fn rejects_extra_separator() {
        assert_eq!(
            "owner/tissue/extra".parse::<Repository>().unwrap_err(),
            RepoParseError::ExtraSeparator
        );
    }
}
