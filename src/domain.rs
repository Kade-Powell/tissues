use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub login: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum IssueState {
    Open,
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IssueSummary {
    pub number: u64,
    pub title: String,
    pub state: IssueState,
    pub labels: Vec<Label>,
    pub assignees: Vec<User>,
    pub author: Option<User>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub comment_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IssueComment {
    pub author: Option<User>,
    pub body: String,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IssueDetail {
    pub summary: IssueSummary,
    pub body: String,
    pub comments: Vec<IssueComment>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IssueTemplate {
    pub name: String,
    pub body: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectBoard {
    pub title: String,
    pub project_id: Option<String>,
    pub status_field_id: Option<String>,
    pub status_options: Vec<ProjectStatusOption>,
    pub item_statuses: Vec<ProjectItemStatus>,
    pub columns: Vec<ProjectColumn>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectStatusOption {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectItemStatus {
    pub issue_number: u64,
    pub item_id: String,
    pub status_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectBoardSummary {
    pub id: String,
    pub owner: String,
    pub number: u32,
    pub title: String,
    pub item_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectColumn {
    pub name: String,
    pub issues: Vec<IssueSummary>,
}
