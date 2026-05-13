use crate::{domain::IssueSummary, repo::Repository};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IssueStateFilter {
    Open,
    Closed,
    All,
}

impl IssueStateFilter {
    pub fn as_query_value(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::All => "all",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssigneeFilter {
    Any,
    Me,
    None,
    User(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssueFilters {
    pub state: IssueStateFilter,
    pub assignee: AssigneeFilter,
    pub labels: Vec<String>,
    pub query: String,
}

impl Default for IssueFilters {
    fn default() -> Self {
        Self {
            state: IssueStateFilter::Open,
            assignee: AssigneeFilter::Any,
            labels: Vec::new(),
            query: String::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UiMode {
    Browsing,
    FilterEditor,
    Search,
    CommentComposer,
    NewIssue,
    ConfirmClose,
    Loading,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingAction {
    Refresh,
    CreateIssue,
    AddComment,
    CloseIssue,
    ReopenIssue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FlashKind {
    Refresh,
    Success,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct App {
    pub repo: Repository,
    pub filters: IssueFilters,
    pub issues: Vec<IssueSummary>,
    pub selected_index: usize,
    pub mode: UiMode,
    pub status: String,
    pub input: String,
    pub body_input: String,
    pub pending_action: Option<PendingAction>,
    pub flash: Option<FlashKind>,
    pub should_quit: bool,
}

impl App {
    pub fn new(repo: Repository) -> Self {
        Self {
            repo,
            filters: IssueFilters::default(),
            issues: Vec::new(),
            selected_index: 0,
            mode: UiMode::Browsing,
            status: "Ready".to_string(),
            input: String::new(),
            body_input: String::new(),
            pending_action: None,
            flash: None,
            should_quit: false,
        }
    }

    pub fn set_issues(&mut self, issues: Vec<IssueSummary>) {
        self.issues = issues;
        self.clamp_selection();
    }

    pub fn selected_issue(&self) -> Option<&IssueSummary> {
        self.issues.get(self.selected_index)
    }

    pub fn select_issue_number(&mut self, number: u64) {
        if let Some(index) = self.issues.iter().position(|issue| issue.number == number) {
            self.selected_index = index;
        }
    }

    pub fn select_next(&mut self) {
        if self.issues.is_empty() {
            self.selected_index = 0;
            return;
        }

        self.selected_index = (self.selected_index + 1).min(self.issues.len() - 1);
    }

    pub fn select_previous(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
    }

    pub fn cycle_state_filter(&mut self) {
        self.filters.state = match self.filters.state {
            IssueStateFilter::Open => IssueStateFilter::Closed,
            IssueStateFilter::Closed => IssueStateFilter::All,
            IssueStateFilter::All => IssueStateFilter::Open,
        };
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.filters.query = query.into();
    }

    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
    }

    fn clamp_selection(&mut self) {
        if self.issues.is_empty() {
            self.selected_index = 0;
        } else {
            self.selected_index = self.selected_index.min(self.issues.len() - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{IssueState, IssueSummary};

    fn issue(number: u64, title: &str) -> IssueSummary {
        IssueSummary {
            number,
            title: title.to_string(),
            state: IssueState::Open,
            labels: Vec::new(),
            assignees: Vec::new(),
            author: None,
            updated_at: None,
            comment_count: 0,
        }
    }

    #[test]
    fn starts_with_open_issues_and_browse_mode() {
        let app = App::new("owner/skunkwork".parse().unwrap());

        assert_eq!(app.filters.state, IssueStateFilter::Open);
        assert_eq!(app.mode, UiMode::Browsing);
        assert!(app.filters.query.is_empty());
    }

    #[test]
    fn cycles_state_filter() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());

        app.cycle_state_filter();
        assert_eq!(app.filters.state, IssueStateFilter::Closed);
        app.cycle_state_filter();
        assert_eq!(app.filters.state, IssueStateFilter::All);
        app.cycle_state_filter();
        assert_eq!(app.filters.state, IssueStateFilter::Open);
    }

    #[test]
    fn updates_search_query() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());

        app.set_query("render bug");

        assert_eq!(app.filters.query, "render bug");
    }

    #[test]
    fn selection_is_clamped_to_loaded_issues() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_issues(vec![issue(1, "one"), issue(2, "two")]);

        app.select_next();
        app.select_next();
        assert_eq!(app.selected_index, 1);

        app.set_issues(vec![issue(1, "one")]);
        assert_eq!(app.selected_index, 0);
    }
}
