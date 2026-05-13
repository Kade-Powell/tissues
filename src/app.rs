use crate::{
    domain::{IssueDetail, IssueSummary, Label},
    repo::Repository,
};

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
    Success,
    Loading,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NewIssueField {
    Title,
    Body,
    Labels,
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
    pub label_input: String,
    pub new_issue_field: NewIssueField,
    pub new_issue_labels: Vec<String>,
    pub repo_labels: Vec<Label>,
    pub selected_detail: Option<IssueDetail>,
    pub comments_expanded: bool,
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
            label_input: String::new(),
            new_issue_field: NewIssueField::Title,
            new_issue_labels: Vec::new(),
            repo_labels: Vec::new(),
            selected_detail: None,
            comments_expanded: true,
            pending_action: None,
            flash: None,
            should_quit: false,
        }
    }

    pub fn set_issues(&mut self, issues: Vec<IssueSummary>) {
        self.issues = issues;
        self.clamp_selection();
        self.clear_stale_detail();
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

    pub fn set_selected_detail(&mut self, detail: IssueDetail) {
        self.selected_detail = Some(detail);
        self.comments_expanded = true;
    }

    pub fn clear_selected_detail(&mut self) {
        self.selected_detail = None;
    }

    pub fn toggle_comments(&mut self) {
        self.comments_expanded = !self.comments_expanded;
    }

    pub fn start_new_issue(&mut self) {
        self.input.clear();
        self.body_input.clear();
        self.label_input.clear();
        self.new_issue_labels.clear();
        self.new_issue_field = NewIssueField::Title;
        self.mode = UiMode::NewIssue;
    }

    pub fn set_repo_labels(&mut self, labels: Vec<Label>) {
        self.repo_labels = labels;
    }

    pub fn next_new_issue_field(&mut self) {
        self.new_issue_field = match self.new_issue_field {
            NewIssueField::Title => NewIssueField::Body,
            NewIssueField::Body => NewIssueField::Labels,
            NewIssueField::Labels => NewIssueField::Title,
        };
    }

    pub fn previous_new_issue_field(&mut self) {
        self.new_issue_field = match self.new_issue_field {
            NewIssueField::Title => NewIssueField::Labels,
            NewIssueField::Body => NewIssueField::Title,
            NewIssueField::Labels => NewIssueField::Body,
        };
    }

    pub fn add_new_issue_label(&mut self, label: impl Into<String>) {
        let label = label.into();
        if label.trim().is_empty() || self.new_issue_labels.iter().any(|item| item == &label) {
            return;
        }

        self.new_issue_labels.push(label);
        self.label_input.clear();
    }

    pub fn pop_new_issue_label(&mut self) {
        self.new_issue_labels.pop();
    }

    pub fn label_suggestions(&self) -> Vec<String> {
        let query = self.label_input.trim().to_lowercase();
        self.repo_labels
            .iter()
            .map(|label| label.name.as_str())
            .filter(|name| {
                !self
                    .new_issue_labels
                    .iter()
                    .any(|selected| selected == name)
            })
            .filter(|name| query.is_empty() || name.to_lowercase().contains(&query))
            .take(5)
            .map(ToOwned::to_owned)
            .collect()
    }

    pub fn accept_first_label_suggestion(&mut self) -> bool {
        if let Some(label) = self.label_suggestions().into_iter().next() {
            self.add_new_issue_label(label);
            true
        } else {
            false
        }
    }

    fn clamp_selection(&mut self) {
        if self.issues.is_empty() {
            self.selected_index = 0;
        } else {
            self.selected_index = self.selected_index.min(self.issues.len() - 1);
        }
    }

    fn clear_stale_detail(&mut self) {
        let selected_number = self.selected_issue().map(|issue| issue.number);
        let detail_number = self
            .selected_detail
            .as_ref()
            .map(|detail| detail.summary.number);

        if detail_number.is_some() && detail_number != selected_number {
            self.selected_detail = None;
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

    #[test]
    fn stores_selected_issue_detail_and_toggles_comments() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        let detail = crate::domain::IssueDetail {
            summary: issue(1, "one"),
            body: "## Description".to_string(),
            comments: Vec::new(),
        };

        app.set_selected_detail(detail);
        assert!(app.comments_expanded);
        assert_eq!(app.selected_detail.as_ref().unwrap().body, "## Description");

        app.toggle_comments();
        assert!(!app.comments_expanded);
        app.toggle_comments();
        assert!(app.comments_expanded);
    }

    #[test]
    fn starts_new_issue_form_with_separate_empty_fields() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.input = "old title".to_string();
        app.body_input = "old body".to_string();
        app.label_input = "bug".to_string();
        app.new_issue_labels = vec!["bug".to_string()];

        app.start_new_issue();

        assert_eq!(app.mode, UiMode::NewIssue);
        assert_eq!(app.new_issue_field, NewIssueField::Title);
        assert!(app.input.is_empty());
        assert!(app.body_input.is_empty());
        assert!(app.label_input.is_empty());
        assert!(app.new_issue_labels.is_empty());
    }

    #[test]
    fn suggests_unselected_repo_labels_from_label_input() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_repo_labels(vec![
            Label {
                name: "bug".to_string(),
            },
            Label {
                name: "docs".to_string(),
            },
            Label {
                name: "good first issue".to_string(),
            },
        ]);
        app.new_issue_labels = vec!["bug".to_string()];
        app.label_input = "do".to_string();

        assert_eq!(app.label_suggestions(), vec!["docs".to_string()]);
        assert!(app.accept_first_label_suggestion());
        assert_eq!(app.new_issue_labels, vec!["bug", "docs"]);
        assert!(app.label_input.is_empty());
    }
}
