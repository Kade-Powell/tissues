use chrono::{DateTime, Utc};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Label {
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct User {
    pub login: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IssueState {
    Open,
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssueSummary {
    pub number: u64,
    pub title: String,
    pub state: IssueState,
    pub labels: Vec<Label>,
    pub assignees: Vec<User>,
    pub author: Option<User>,
    pub updated_at: Option<DateTime<Utc>>,
    pub comment_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssueComment {
    pub author: Option<User>,
    pub body: String,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssueDetail {
    pub summary: IssueSummary,
    pub body: String,
    pub comments: Vec<IssueComment>,
}
