use std::process::Command;

use async_trait::async_trait;
use color_eyre::eyre::{Result, WrapErr, eyre};
use octocrab::{Octocrab, models, params};

use crate::{
    app::{AssigneeFilter, IssueFilters, IssueSort, IssueStateFilter},
    domain::{IssueComment, IssueDetail, IssueState, IssueSummary, IssueTemplate, Label, User},
    repo::Repository,
};

#[async_trait]
pub trait IssueBackend {
    async fn current_login(&self) -> Result<String>;
    async fn list_issues(
        &self,
        repo: &Repository,
        filters: &IssueFilters,
    ) -> Result<Vec<IssueSummary>>;
    async fn get_issue(&self, repo: &Repository, number: u64) -> Result<IssueDetail>;
    async fn list_comments(&self, repo: &Repository, number: u64) -> Result<Vec<IssueComment>>;
    async fn list_labels(&self, repo: &Repository) -> Result<Vec<Label>>;
    async fn list_issue_templates(&self, repo: &Repository) -> Result<Vec<IssueTemplate>>;
    async fn list_collaborators(&self, repo: &Repository) -> Result<Vec<User>>;
    async fn create_issue(
        &self,
        repo: &Repository,
        title: &str,
        body: &str,
        labels: &[String],
    ) -> Result<IssueSummary>;
    async fn add_comment(&self, repo: &Repository, number: u64, body: &str)
    -> Result<IssueComment>;
    async fn set_issue_state(
        &self,
        repo: &Repository,
        number: u64,
        state: IssueState,
    ) -> Result<IssueSummary>;
    async fn update_issue(
        &self,
        repo: &Repository,
        number: u64,
        title: &str,
        body: &str,
    ) -> Result<IssueSummary>;
    async fn set_issue_assignees(
        &self,
        repo: &Repository,
        number: u64,
        assignees: &[String],
    ) -> Result<IssueSummary>;
    async fn set_issue_labels(
        &self,
        repo: &Repository,
        number: u64,
        labels: &[String],
    ) -> Result<IssueSummary>;
}

pub struct GitHubClient {
    crab: Octocrab,
}

impl GitHubClient {
    pub fn from_gh_cli() -> Result<Self> {
        Self::from_token(load_gh_token()?)
    }

    pub fn from_token(token: String) -> Result<Self> {
        let crab = Octocrab::builder()
            .personal_token(token)
            .build()
            .wrap_err("failed to build GitHub client")?;

        Ok(Self { crab })
    }

    pub async fn load_current_login(&self) -> Result<String> {
        let user = self
            .crab
            .current()
            .user()
            .await
            .wrap_err("failed to load authenticated GitHub user")?;
        Ok(user.login)
    }

    async fn load_template_body(
        &self,
        repo: &Repository,
        path: impl Into<String>,
    ) -> Option<String> {
        self.crab
            .repos(&repo.owner, &repo.name)
            .get_content()
            .path(path)
            .send()
            .await
            .ok()
            .and_then(|mut content| {
                content
                    .take_items()
                    .into_iter()
                    .next()
                    .and_then(|item| item.decoded_content())
            })
    }
}

#[async_trait]
impl IssueBackend for GitHubClient {
    async fn current_login(&self) -> Result<String> {
        self.load_current_login().await
    }

    async fn list_issues(
        &self,
        repo: &Repository,
        filters: &IssueFilters,
    ) -> Result<Vec<IssueSummary>> {
        let labels = filters.labels.clone();
        let assignee = match &filters.assignee {
            AssigneeFilter::Any => None,
            AssigneeFilter::Me => Some(self.load_current_login().await?),
            AssigneeFilter::None => Some("none".to_string()),
            AssigneeFilter::User(user) => Some(user.clone()),
        };

        let handler = self.crab.issues(&repo.owner, &repo.name);
        let mut request = handler
            .list()
            .state(to_param_state(&filters.state))
            .sort(to_param_sort(&filters.sort))
            .direction(params::Direction::Descending)
            .per_page(100);

        if let Some(assignee) = assignee.as_deref() {
            request = request.assignee(assignee);
        }

        if !labels.is_empty() {
            request = request.labels(&labels);
        }

        let page = request
            .send()
            .await
            .wrap_err("failed to list GitHub issues")?;
        let issues = self
            .crab
            .all_pages(page)
            .await
            .wrap_err("failed to load all GitHub issue pages")?;
        let query = filters.query.trim().to_lowercase();

        Ok(issues
            .into_iter()
            .filter(|issue| issue.pull_request.is_none())
            .filter(|issue| query.is_empty() || issue.title.to_lowercase().contains(&query))
            .map(issue_summary_from_octocrab)
            .collect())
    }

    async fn get_issue(&self, repo: &Repository, number: u64) -> Result<IssueDetail> {
        let issue = self
            .crab
            .issues(&repo.owner, &repo.name)
            .get(number)
            .await
            .wrap_err_with(|| format!("failed to load issue #{number}"))?;
        let comments = self.list_comments(repo, number).await?;
        let body = issue.body.clone().unwrap_or_default();

        Ok(IssueDetail {
            summary: issue_summary_from_octocrab(issue),
            body,
            comments,
        })
    }

    async fn list_comments(&self, repo: &Repository, number: u64) -> Result<Vec<IssueComment>> {
        let page = self
            .crab
            .issues(&repo.owner, &repo.name)
            .list_comments(number)
            .per_page(100)
            .send()
            .await
            .wrap_err_with(|| format!("failed to list comments for issue #{number}"))?;
        let comments = self
            .crab
            .all_pages(page)
            .await
            .wrap_err("failed to load all comment pages")?;

        Ok(comments.into_iter().map(comment_from_octocrab).collect())
    }

    async fn list_labels(&self, repo: &Repository) -> Result<Vec<Label>> {
        let page = self
            .crab
            .issues(&repo.owner, &repo.name)
            .list_labels_for_repo()
            .per_page(100)
            .send()
            .await
            .wrap_err("failed to list repository labels")?;
        let mut labels = self
            .crab
            .all_pages(page)
            .await
            .wrap_err("failed to load all repository labels")?
            .into_iter()
            .map(|label| Label { name: label.name })
            .collect::<Vec<_>>();
        labels.sort_by(|left, right| left.name.cmp(&right.name));

        Ok(labels)
    }

    async fn list_issue_templates(&self, repo: &Repository) -> Result<Vec<IssueTemplate>> {
        let mut templates = Vec::new();

        if let Some(content) = self
            .load_template_body(repo, ".github/ISSUE_TEMPLATE.md")
            .await
        {
            templates.push(IssueTemplate {
                name: "Issue template".to_string(),
                body: content,
            });
        }

        if let Ok(mut directory) = self
            .crab
            .repos(&repo.owner, &repo.name)
            .get_content()
            .path(".github/ISSUE_TEMPLATE")
            .send()
            .await
        {
            let items = directory.take_items();
            for item in items
                .into_iter()
                .filter(|item| item.r#type == "file")
                .filter(|item| item.name.ends_with(".md") || item.name.ends_with(".markdown"))
            {
                let name = item.name.clone();
                if let Some(body) = self.load_template_body(repo, item.path).await {
                    templates.push(IssueTemplate {
                        name: template_name(&name),
                        body,
                    });
                }
            }
        }

        templates.sort_by(|left, right| left.name.cmp(&right.name));
        templates.dedup_by(|left, right| left.name == right.name);
        Ok(templates)
    }

    async fn list_collaborators(&self, repo: &Repository) -> Result<Vec<User>> {
        let page = self
            .crab
            .repos(&repo.owner, &repo.name)
            .list_collaborators()
            .per_page(100)
            .send()
            .await
            .wrap_err("failed to list repository collaborators")?;
        let mut collaborators = self
            .crab
            .all_pages(page)
            .await
            .wrap_err("failed to load all repository collaborators")?
            .into_iter()
            .map(|collaborator| User {
                login: collaborator.author.login,
            })
            .collect::<Vec<_>>();
        collaborators.sort_by(|left, right| left.login.cmp(&right.login));

        Ok(collaborators)
    }

    async fn create_issue(
        &self,
        repo: &Repository,
        title: &str,
        body: &str,
        labels: &[String],
    ) -> Result<IssueSummary> {
        let issue = self
            .crab
            .issues(&repo.owner, &repo.name)
            .create(title)
            .body(body.to_string())
            .labels(labels.to_vec())
            .send()
            .await
            .wrap_err("failed to create issue")?;

        Ok(issue_summary_from_octocrab(issue))
    }

    async fn add_comment(
        &self,
        repo: &Repository,
        number: u64,
        body: &str,
    ) -> Result<IssueComment> {
        let comment = self
            .crab
            .issues(&repo.owner, &repo.name)
            .create_comment(number, body)
            .await
            .wrap_err_with(|| format!("failed to add comment to issue #{number}"))?;

        Ok(comment_from_octocrab(comment))
    }

    async fn set_issue_state(
        &self,
        repo: &Repository,
        number: u64,
        state: IssueState,
    ) -> Result<IssueSummary> {
        let issue = self
            .crab
            .issues(&repo.owner, &repo.name)
            .update(number)
            .state(match state {
                IssueState::Open => models::IssueState::Open,
                IssueState::Closed => models::IssueState::Closed,
            })
            .send()
            .await
            .wrap_err_with(|| format!("failed to update issue #{number}"))?;

        Ok(issue_summary_from_octocrab(issue))
    }

    async fn update_issue(
        &self,
        repo: &Repository,
        number: u64,
        title: &str,
        body: &str,
    ) -> Result<IssueSummary> {
        let issue = self
            .crab
            .issues(&repo.owner, &repo.name)
            .update(number)
            .title(title)
            .body(body)
            .send()
            .await
            .wrap_err_with(|| format!("failed to update issue #{number}"))?;

        Ok(issue_summary_from_octocrab(issue))
    }

    async fn set_issue_assignees(
        &self,
        repo: &Repository,
        number: u64,
        assignees: &[String],
    ) -> Result<IssueSummary> {
        let issue = self
            .crab
            .issues(&repo.owner, &repo.name)
            .update(number)
            .assignees(assignees)
            .send()
            .await
            .wrap_err_with(|| format!("failed to update assignees for issue #{number}"))?;

        Ok(issue_summary_from_octocrab(issue))
    }

    async fn set_issue_labels(
        &self,
        repo: &Repository,
        number: u64,
        labels: &[String],
    ) -> Result<IssueSummary> {
        let issue = self
            .crab
            .issues(&repo.owner, &repo.name)
            .update(number)
            .labels(labels)
            .send()
            .await
            .wrap_err_with(|| format!("failed to update labels for issue #{number}"))?;

        Ok(issue_summary_from_octocrab(issue))
    }
}

pub fn load_gh_token() -> Result<String> {
    let output = Command::new("gh")
        .args(["auth", "token"])
        .output()
        .wrap_err("failed to run `gh auth token`; install GitHub CLI and run `gh auth login`")?;

    if !output.status.success() {
        return Err(eyre!(
            "`gh auth token` failed; run `gh auth login` before starting tissue"
        ));
    }

    parse_gh_token_output(String::from_utf8_lossy(&output.stdout).as_ref())
}

fn parse_gh_token_output(output: &str) -> Result<String> {
    let token = output.trim();

    if token.is_empty() {
        return Err(eyre!("`gh auth token` returned an empty token"));
    }

    Ok(token.to_string())
}

fn to_param_state(state: &IssueStateFilter) -> params::State {
    match state {
        IssueStateFilter::Open => params::State::Open,
        IssueStateFilter::Closed => params::State::Closed,
        IssueStateFilter::All => params::State::All,
    }
}

fn to_param_sort(sort: &IssueSort) -> params::issues::Sort {
    match sort {
        IssueSort::Updated => params::issues::Sort::Updated,
        IssueSort::Created => params::issues::Sort::Created,
        IssueSort::Comments => params::issues::Sort::Comments,
        IssueSort::Assignee => params::issues::Sort::Updated,
    }
}

fn template_name(file_name: &str) -> String {
    file_name
        .trim_end_matches(".markdown")
        .trim_end_matches(".md")
        .replace(['_', '-'], " ")
}

fn issue_summary_from_octocrab(issue: models::issues::Issue) -> IssueSummary {
    IssueSummary {
        number: issue.number,
        title: issue.title,
        state: match issue.state {
            models::IssueState::Open => IssueState::Open,
            models::IssueState::Closed => IssueState::Closed,
            _ => IssueState::Open,
        },
        labels: issue
            .labels
            .into_iter()
            .map(|label| Label { name: label.name })
            .collect(),
        assignees: issue
            .assignees
            .into_iter()
            .map(|user| User { login: user.login })
            .collect(),
        author: Some(User {
            login: issue.user.login,
        }),
        created_at: Some(issue.created_at),
        updated_at: Some(issue.updated_at),
        comment_count: issue.comments.into(),
    }
}

fn comment_from_octocrab(comment: models::issues::Comment) -> IssueComment {
    IssueComment {
        author: Some(User {
            login: comment.user.login,
        }),
        body: comment.body.unwrap_or_default(),
        created_at: Some(comment.created_at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_token_output() {
        assert_eq!(parse_gh_token_output("ghp_secret\n").unwrap(), "ghp_secret");
    }

    #[test]
    fn rejects_empty_token_output_without_echoing_token() {
        let err = parse_gh_token_output("   \n").unwrap_err();

        assert!(err.to_string().contains("empty token"));
        assert!(!err.to_string().contains("ghp_"));
    }
}
