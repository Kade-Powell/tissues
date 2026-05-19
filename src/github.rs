use std::process::Command;

use async_trait::async_trait;
use color_eyre::eyre::{Result, WrapErr, eyre};
use octocrab::{Octocrab, models, params};
use serde::Deserialize;
use serde_json::json;

use crate::{
    app::{AssigneeFilter, IssueFilters, IssueSort, IssueStateFilter},
    config::ProjectBoardConfig,
    domain::{
        IssueComment, IssueDetail, IssueState, IssueSummary, IssueTemplate, Label, ProjectBoard,
        ProjectBoardSummary, ProjectColumn, ProjectItemStatus, ProjectStatusOption, User,
    },
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
    async fn list_project_board(
        &self,
        repo: &Repository,
        config: &ProjectBoardConfig,
    ) -> Result<Option<ProjectBoard>>;
    async fn list_project_boards(&self, repo: &Repository) -> Result<Vec<ProjectBoardSummary>>;
    async fn update_project_item_status(
        &self,
        project_id: &str,
        item_id: &str,
        field_id: &str,
        option_id: &str,
    ) -> Result<()>;
    async fn refresh_auth_scopes(&self, scopes: &[String]) -> Result<()>;
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
    async fn delete_issue(&self, repo: &Repository, number: u64) -> Result<()>;
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
    gh_user: Option<String>,
}

impl GitHubClient {
    pub fn from_gh_cli() -> Result<Self> {
        Self::from_gh_cli_user(None)
    }

    pub fn from_gh_cli_user(gh_user: Option<&str>) -> Result<Self> {
        Self::from_token_for_user(load_gh_token(gh_user)?, gh_user)
    }

    pub fn from_token(token: String) -> Result<Self> {
        Self::from_token_for_user(token, None)
    }

    fn from_token_for_user(token: String, gh_user: Option<&str>) -> Result<Self> {
        let crab = Octocrab::builder()
            .personal_token(token)
            .build()
            .wrap_err("failed to build GitHub client")?;

        Ok(Self {
            crab,
            gh_user: gh_user.map(ToOwned::to_owned),
        })
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

    async fn list_project_board(
        &self,
        repo: &Repository,
        config: &ProjectBoardConfig,
    ) -> Result<Option<ProjectBoard>> {
        let Some(project_id) = self.project_id(repo, config).await? else {
            return Ok(None);
        };
        self.load_project_items(&project_id, &config.status_field)
            .await
    }

    async fn list_project_boards(&self, repo: &Repository) -> Result<Vec<ProjectBoardSummary>> {
        let response: RepositoryProjectChoicesResponse = self
            .crab
            .graphql(&json!({
                "query": REPOSITORY_PROJECT_CHOICES_QUERY,
                "variables": { "owner": repo.owner, "name": repo.name }
            }))
            .await
            .wrap_err("failed to list repository GitHub projects")?;
        Ok(response
            .repository
            .projects_v2
            .nodes
            .into_iter()
            .map(|project| ProjectBoardSummary {
                id: project.id,
                owner: project.owner.login(),
                number: project.number,
                title: project.title,
                item_count: project.items.total_count,
            })
            .collect())
    }

    async fn update_project_item_status(
        &self,
        project_id: &str,
        item_id: &str,
        field_id: &str,
        option_id: &str,
    ) -> Result<()> {
        let _: serde_json::Value = self
            .crab
            .graphql(&json!({
                "query": UPDATE_PROJECT_ITEM_STATUS_MUTATION,
                "variables": {
                    "projectId": project_id,
                    "itemId": item_id,
                    "fieldId": field_id,
                    "optionId": option_id,
                }
            }))
            .await
            .wrap_err("failed to update GitHub project item status")?;
        Ok(())
    }

    async fn refresh_auth_scopes(&self, scopes: &[String]) -> Result<()> {
        refresh_gh_auth_scopes(self.gh_user.as_deref(), scopes)
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

    async fn delete_issue(&self, repo: &Repository, number: u64) -> Result<()> {
        let response: IssueIdResponse = self
            .crab
            .graphql(&json!({
                "query": ISSUE_ID_QUERY,
                "variables": {
                    "owner": repo.owner,
                    "name": repo.name,
                    "number": number as i64,
                }
            }))
            .await
            .wrap_err_with(|| format!("failed to load issue #{number}"))?;
        let issue_id = response
            .repository
            .and_then(|repo| repo.issue)
            .map(|issue| issue.id)
            .ok_or_else(|| eyre!("issue #{number} not found"))?;

        let _: serde_json::Value = self
            .crab
            .graphql(&json!({
                "query": DELETE_ISSUE_MUTATION,
                "variables": { "issueId": issue_id }
            }))
            .await
            .wrap_err_with(|| format!("failed to delete issue #{number}"))?;
        Ok(())
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

impl GitHubClient {
    async fn project_id(
        &self,
        repo: &Repository,
        config: &ProjectBoardConfig,
    ) -> Result<Option<String>> {
        if let Some(number) = config.number {
            let owner = config.owner.as_deref().unwrap_or(&repo.owner);
            if let Some(project_id) = self.attached_project_id(repo, owner, number).await? {
                return Ok(Some(project_id));
            }
            return self.owner_project_id(owner, number).await;
        }

        let response: RepositoryProjectsResponse = self
            .crab
            .graphql(&json!({
                "query": REPOSITORY_PROJECTS_QUERY,
                "variables": { "owner": repo.owner, "name": repo.name }
            }))
            .await
            .wrap_err("failed to list repository GitHub projects")?;
        Ok(response
            .repository
            .projects_v2
            .nodes
            .into_iter()
            .next()
            .map(|project| project.id))
    }

    async fn owner_project_id(&self, owner: &str, number: u32) -> Result<Option<String>> {
        if let Ok(Some(project_id)) = self.user_project_id(owner, number).await {
            return Ok(Some(project_id));
        }
        self.organization_project_id(owner, number).await
    }

    async fn user_project_id(&self, owner: &str, number: u32) -> Result<Option<String>> {
        let response: UserProjectLookupResponse = self
            .crab
            .graphql(&json!({
                "query": PROJECT_BY_USER_QUERY,
                "variables": { "owner": owner, "number": number }
            }))
            .await
            .wrap_err_with(|| {
                format!("failed to load user GitHub project #{number} for {owner}")
            })?;
        Ok(response
            .user
            .and_then(|owner| owner.project_v2)
            .map(|project| project.id))
    }

    async fn organization_project_id(&self, owner: &str, number: u32) -> Result<Option<String>> {
        let response: OrganizationProjectLookupResponse = self
            .crab
            .graphql(&json!({
                "query": PROJECT_BY_ORGANIZATION_QUERY,
                "variables": { "owner": owner, "number": number }
            }))
            .await
            .wrap_err_with(|| {
                format!("failed to load organization GitHub project #{number} for {owner}")
            })?;
        Ok(response
            .organization
            .and_then(|owner| owner.project_v2)
            .map(|project| project.id))
    }

    async fn attached_project_id(
        &self,
        repo: &Repository,
        owner: &str,
        number: u32,
    ) -> Result<Option<String>> {
        let response: RepositoryProjectChoicesResponse = self
            .crab
            .graphql(&json!({
                "query": REPOSITORY_PROJECT_CHOICES_QUERY,
                "variables": { "owner": repo.owner, "name": repo.name }
            }))
            .await
            .wrap_err("failed to list repository GitHub projects")?;
        Ok(response
            .repository
            .projects_v2
            .nodes
            .into_iter()
            .find(|project| {
                project.number == number
                    && project
                        .owner
                        .login
                        .as_deref()
                        .is_some_and(|login| login == owner)
            })
            .map(|project| project.id))
    }

    async fn load_project_items(
        &self,
        project_id: &str,
        status_field: &str,
    ) -> Result<Option<ProjectBoard>> {
        let mut cursor: Option<String> = None;
        let mut title = None;
        let mut status_field_id = None;
        let mut status_options = Vec::<ProjectStatusOption>::new();
        let mut item_statuses = Vec::<ProjectItemStatus>::new();
        let mut columns = Vec::<ProjectColumn>::new();

        loop {
            let response: ProjectItemsResponse = self
                .crab
                .graphql(&json!({
                    "query": PROJECT_ITEMS_QUERY,
                    "variables": {
                        "projectId": project_id,
                        "cursor": cursor,
                        "statusField": status_field,
                    }
                }))
                .await
                .wrap_err("failed to load GitHub project items")?;
            let Some(project) = response.node else {
                return Ok(None);
            };
            if status_field_id.is_none() {
                if let Some(field) = project.status_field(status_field) {
                    status_field_id = field.id;
                    status_options = field
                        .options
                        .into_iter()
                        .map(|option| ProjectStatusOption {
                            id: option.id,
                            name: option.name,
                        })
                        .collect();
                }
            }
            title.get_or_insert(project.title);
            for item in project.items.nodes {
                let column = item
                    .status_name()
                    .unwrap_or_else(|| "No status".to_string());
                let item_id = item.id;
                let Some(content) = item.content else {
                    continue;
                };
                let Some(issue) = content.into_issue_summary() else {
                    continue;
                };
                item_statuses.push(ProjectItemStatus {
                    issue_number: issue.number,
                    item_id,
                    status_name: column.clone(),
                });
                upsert_project_column(&mut columns, column)
                    .issues
                    .push(issue);
            }
            if !project.items.page_info.has_next_page {
                break;
            }
            cursor = project.items.page_info.end_cursor;
        }

        Ok(Some(ProjectBoard {
            title: title.unwrap_or_else(|| "Project".to_string()),
            project_id: Some(project_id.to_string()),
            status_field_id,
            status_options,
            item_statuses,
            columns,
        }))
    }
}

fn upsert_project_column(columns: &mut Vec<ProjectColumn>, name: String) -> &mut ProjectColumn {
    if let Some(index) = columns.iter().position(|column| column.name == name) {
        return &mut columns[index];
    }
    columns.push(ProjectColumn {
        name,
        issues: Vec::new(),
    });
    columns.last_mut().expect("column just pushed")
}

const REPOSITORY_PROJECTS_QUERY: &str = r#"
query RepositoryProjects($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) {
    projectsV2(first: 1, orderBy: {field: UPDATED_AT, direction: DESC}) {
      nodes { id }
    }
  }
}
"#;

const REPOSITORY_PROJECT_CHOICES_QUERY: &str = r#"
query RepositoryProjectChoices($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) {
    projectsV2(first: 20, orderBy: {field: UPDATED_AT, direction: DESC}) {
      nodes {
        id
        title
        number
        owner {
          ... on User { login }
          ... on Organization { login }
        }
        items(first: 0) { totalCount }
      }
    }
  }
}
"#;

const PROJECT_BY_USER_QUERY: &str = r#"
query ProjectByUser($owner: String!, $number: Int!) {
  user(login: $owner) { projectV2(number: $number) { id } }
}
"#;

const PROJECT_BY_ORGANIZATION_QUERY: &str = r#"
query ProjectByOrganization($owner: String!, $number: Int!) {
  organization(login: $owner) { projectV2(number: $number) { id } }
}
"#;

const PROJECT_ITEMS_QUERY: &str = r#"
query ProjectItems($projectId: ID!, $cursor: String, $statusField: String!) {
  node(id: $projectId) {
    ... on ProjectV2 {
      id
      title
      fields(first: 50) {
        nodes {
          ... on ProjectV2SingleSelectField {
            id
            name
            options { id name }
          }
        }
      }
      items(first: 100, after: $cursor) {
        nodes {
          id
          content {
            ... on Issue {
              number
              title
              state
              createdAt
              updatedAt
              comments { totalCount }
              author { login }
              labels(first: 20) { nodes { name } }
              assignees(first: 10) { nodes { login } }
            }
          }
          fieldValueByName(name: $statusField) {
            ... on ProjectV2ItemFieldSingleSelectValue { name }
          }
        }
        pageInfo { hasNextPage endCursor }
      }
    }
  }
}
"#;

const UPDATE_PROJECT_ITEM_STATUS_MUTATION: &str = r#"
mutation UpdateProjectItemStatus(
  $projectId: ID!,
  $itemId: ID!,
  $fieldId: ID!,
  $optionId: String!
) {
  updateProjectV2ItemFieldValue(input: {
    projectId: $projectId,
    itemId: $itemId,
    fieldId: $fieldId,
    value: { singleSelectOptionId: $optionId }
  }) {
    projectV2Item { id }
  }
}
"#;

const ISSUE_ID_QUERY: &str = r#"
query IssueId($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    issue(number: $number) {
      id
    }
  }
}
"#;

const DELETE_ISSUE_MUTATION: &str = r#"
mutation DeleteIssue($issueId: ID!) {
  deleteIssue(input: { issueId: $issueId }) {
    clientMutationId
  }
}
"#;

#[derive(Debug, Deserialize)]
struct RepositoryProjectsResponse {
    repository: RepositoryProjects,
}

#[derive(Debug, Deserialize)]
struct RepositoryProjects {
    #[serde(rename = "projectsV2")]
    projects_v2: ProjectNodes,
}

#[derive(Debug, Deserialize)]
struct ProjectNodes {
    nodes: Vec<ProjectNode>,
}

#[derive(Debug, Deserialize)]
struct IssueIdResponse {
    repository: Option<IssueIdRepository>,
}

#[derive(Debug, Deserialize)]
struct IssueIdRepository {
    issue: Option<IssueIdNode>,
}

#[derive(Debug, Deserialize)]
struct IssueIdNode {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ProjectNode {
    id: String,
}

#[derive(Debug, Deserialize)]
struct RepositoryProjectChoicesResponse {
    repository: RepositoryProjectChoices,
}

#[derive(Debug, Deserialize)]
struct RepositoryProjectChoices {
    #[serde(rename = "projectsV2")]
    projects_v2: ProjectChoiceNodes,
}

#[derive(Debug, Deserialize)]
struct ProjectChoiceNodes {
    nodes: Vec<ProjectChoiceNode>,
}

#[derive(Debug, Deserialize)]
struct ProjectChoiceNode {
    id: String,
    title: String,
    number: u32,
    owner: ProjectChoiceOwner,
    items: ProjectChoiceItems,
}

#[derive(Debug, Deserialize)]
struct ProjectChoiceOwner {
    login: Option<String>,
}

impl ProjectChoiceOwner {
    fn login(self) -> String {
        self.login.unwrap_or_else(|| "unknown".to_string())
    }
}

#[derive(Debug, Deserialize)]
struct ProjectChoiceItems {
    #[serde(rename = "totalCount")]
    total_count: usize,
}

#[derive(Debug, Deserialize)]
struct UserProjectLookupResponse {
    user: Option<ProjectOwner>,
}

#[derive(Debug, Deserialize)]
struct OrganizationProjectLookupResponse {
    organization: Option<ProjectOwner>,
}

#[derive(Debug, Deserialize)]
struct ProjectOwner {
    #[serde(rename = "projectV2")]
    project_v2: Option<ProjectNode>,
}

#[derive(Debug, Deserialize)]
struct ProjectItemsResponse {
    node: Option<ProjectItemsNode>,
}

#[derive(Debug, Deserialize)]
struct ProjectItemsNode {
    title: String,
    fields: ProjectFields,
    items: ProjectItems,
}

impl ProjectItemsNode {
    fn status_field(&self, name: &str) -> Option<ProjectFieldNode> {
        self.fields
            .nodes
            .iter()
            .find(|field| field.id.is_some() && field.name.as_deref() == Some(name))
            .cloned()
    }
}

#[derive(Clone, Debug, Deserialize)]
struct ProjectFields {
    nodes: Vec<ProjectFieldNode>,
}

#[derive(Clone, Debug, Deserialize)]
struct ProjectFieldNode {
    id: Option<String>,
    name: Option<String>,
    #[serde(default)]
    options: Vec<ProjectFieldOption>,
}

#[derive(Clone, Debug, Deserialize)]
struct ProjectFieldOption {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ProjectItems {
    nodes: Vec<ProjectItem>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

#[derive(Debug, Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectItem {
    id: String,
    content: Option<ProjectItemContent>,
    #[serde(rename = "fieldValueByName")]
    field_value_by_name: Option<ProjectStatusValue>,
}

impl ProjectItem {
    fn status_name(&self) -> Option<String> {
        self.field_value_by_name
            .as_ref()
            .and_then(|value| value.name.clone())
            .filter(|name| !name.trim().is_empty())
    }
}

#[derive(Debug, Deserialize)]
struct ProjectStatusValue {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectItemContent {
    number: u64,
    title: String,
    state: String,
    #[serde(rename = "createdAt")]
    created_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(rename = "updatedAt")]
    updated_at: Option<chrono::DateTime<chrono::Utc>>,
    comments: ProjectCommentCount,
    author: Option<User>,
    labels: ProjectLabelNodes,
    assignees: ProjectUserNodes,
}

impl ProjectItemContent {
    fn into_issue_summary(self) -> Option<IssueSummary> {
        let state = match self.state.as_str() {
            "OPEN" => IssueState::Open,
            "CLOSED" => IssueState::Closed,
            _ => return None,
        };
        Some(IssueSummary {
            number: self.number,
            title: self.title,
            state,
            labels: self.labels.nodes,
            assignees: self.assignees.nodes,
            author: self.author,
            created_at: self.created_at,
            updated_at: self.updated_at,
            comment_count: self.comments.total_count,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ProjectCommentCount {
    #[serde(rename = "totalCount")]
    total_count: u64,
}

#[derive(Debug, Deserialize)]
struct ProjectLabelNodes {
    nodes: Vec<Label>,
}

#[derive(Debug, Deserialize)]
struct ProjectUserNodes {
    nodes: Vec<User>,
}

pub fn load_gh_token(gh_user: Option<&str>) -> Result<String> {
    let args = gh_token_args(gh_user);
    let output = Command::new("gh").args(&args).output().wrap_err_with(|| {
        format!(
            "failed to run `{}`; install GitHub CLI and run `gh auth login`",
            gh_command_display(&args)
        )
    })?;

    if !output.status.success() {
        return Err(eyre!(
            "`{}` failed; run `gh auth login` before starting tissues",
            gh_command_display(&args)
        ));
    }

    parse_gh_token_output(String::from_utf8_lossy(&output.stdout).as_ref())
}

pub fn refresh_gh_auth_scopes(gh_user: Option<&str>, scopes: &[String]) -> Result<()> {
    let scopes = scopes
        .iter()
        .map(|scope| scope.trim())
        .filter(|scope| !scope.is_empty())
        .collect::<Vec<_>>();
    if scopes.is_empty() {
        return Ok(());
    }

    if let Some(user) = gh_user.filter(|user| !user.trim().is_empty()) {
        let active_user = active_gh_user().ok();
        if active_user.as_deref() != Some(user) {
            return Err(eyre!(
                "GitHub CLI can only refresh scopes for the active account. Run `gh auth switch -u {user}` first, then `{}`.",
                gh_auth_refresh_command(&scopes)
            ));
        }
    }

    let args = gh_auth_refresh_args(&scopes);
    let status = Command::new("gh")
        .args(&args)
        .status()
        .wrap_err_with(|| format!("failed to run `{}`", gh_command_display(&args)))?;
    if !status.success() {
        return Err(eyre!("`{}` failed", gh_command_display(&args)));
    }
    Ok(())
}

fn active_gh_user() -> Result<String> {
    let output = Command::new("gh")
        .args(["auth", "status", "--active", "--json", "hosts"])
        .output()
        .wrap_err("failed to inspect active GitHub CLI account")?;
    if !output.status.success() {
        return Err(eyre!("`gh auth status --active --json hosts` failed"));
    }

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).wrap_err("failed to parse `gh auth status` JSON")?;
    value["hosts"]["github.com"]
        .as_array()
        .and_then(|accounts| accounts.iter().find(|account| account["active"] == true))
        .and_then(|account| account["login"].as_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| eyre!("no active GitHub CLI account found"))
}

fn gh_token_args(gh_user: Option<&str>) -> Vec<&str> {
    let mut args = vec!["auth", "token"];
    if let Some(user) = gh_user.filter(|user| !user.trim().is_empty()) {
        args.extend(["--user", user]);
    }
    args
}

fn gh_auth_refresh_args<'a>(scopes: &'a [&'a str]) -> Vec<&'a str> {
    let mut args = vec!["auth", "refresh"];
    for scope in scopes {
        args.extend(["-s", scope]);
    }
    args
}

fn gh_auth_refresh_command(scopes: &[&str]) -> String {
    gh_command_display(&gh_auth_refresh_args(scopes))
}

fn gh_command_display(args: &[&str]) -> String {
    format!("gh {}", args.join(" "))
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

    #[test]
    fn gh_token_args_use_active_account_when_no_user_is_configured() {
        assert_eq!(gh_token_args(None), vec!["auth", "token"]);
    }

    #[test]
    fn gh_token_args_can_request_a_specific_user_without_switching_accounts() {
        assert_eq!(
            gh_token_args(Some("Kade-Powell")),
            vec!["auth", "token", "--user", "Kade-Powell"]
        );
    }
}
