use std::cmp::Ordering;

use crate::{
    domain::{IssueDetail, IssueSummary, IssueTemplate, Label, User},
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
pub enum IssueSort {
    Updated,
    Created,
    Comments,
    Assignee,
}

impl IssueSort {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Updated => "updated",
            Self::Created => "created",
            Self::Comments => "comments",
            Self::Assignee => "assignee",
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

impl AssigneeFilter {
    pub fn label(&self) -> String {
        match self {
            Self::Any => "any".to_string(),
            Self::Me => "me".to_string(),
            Self::None => "unassigned".to_string(),
            Self::User(login) => login.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssueFilters {
    pub state: IssueStateFilter,
    pub assignee: AssigneeFilter,
    pub labels: Vec<String>,
    pub query: String,
    pub sort: IssueSort,
}

impl Default for IssueFilters {
    fn default() -> Self {
        Self {
            state: IssueStateFilter::Open,
            assignee: AssigneeFilter::Any,
            labels: Vec::new(),
            query: String::new(),
            sort: IssueSort::Updated,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UiMode {
    Browsing,
    Command,
    FilterEditor,
    Search,
    CommentComposer,
    CloseComment,
    NewIssue,
    IssueEditor,
    AssigneeFilter,
    AssigneeEditor,
    IssueLabelEditor,
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
pub enum IssueEditField {
    Title,
    Body,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingAction {
    Refresh,
    LoadLabels,
    LoadCollaborators,
    LoadTemplates,
    CreateIssue,
    AddComment,
    CloseIssue,
    ReopenIssue,
    UpdateIssue,
    UpdateAssignees,
    UpdateLabels,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssigneeChoice {
    Any,
    Me,
    Unassigned,
    User(String),
}

impl AssigneeChoice {
    pub fn label(&self) -> String {
        match self {
            Self::Any => "any".to_string(),
            Self::Me => "me".to_string(),
            Self::Unassigned => "unassigned".to_string(),
            Self::User(login) => login.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FlashKind {
    Refresh,
    Success,
    Error,
}

const NEW_ISSUE_ANIMATION_FRAMES: u8 = 90;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IssueHighlightKind {
    New,
    Mention,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssueHighlight {
    pub number: u64,
    pub kind: IssueHighlightKind,
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
    pub repo_issue_templates: Vec<IssueTemplate>,
    pub selected_template_index: usize,
    pub repo_collaborators: Vec<User>,
    pub picker_index: usize,
    pub editing_assignees: Vec<String>,
    pub editing_issue_labels: Vec<String>,
    pub issue_edit_field: IssueEditField,
    pub selected_detail: Option<IssueDetail>,
    pub comments_expanded: bool,
    pub pending_action: Option<PendingAction>,
    pub flash: Option<FlashKind>,
    pub viewer_login: Option<String>,
    pub issue_highlights: Vec<IssueHighlight>,
    pub new_issue_animation_frame: u8,
    pub activity_frame: u8,
    pub detail_scroll: u16,
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
            repo_issue_templates: Vec::new(),
            selected_template_index: 0,
            repo_collaborators: Vec::new(),
            picker_index: 0,
            editing_assignees: Vec::new(),
            editing_issue_labels: Vec::new(),
            issue_edit_field: IssueEditField::Title,
            selected_detail: None,
            comments_expanded: true,
            pending_action: None,
            flash: None,
            viewer_login: None,
            issue_highlights: Vec::new(),
            new_issue_animation_frame: 0,
            activity_frame: 0,
            detail_scroll: 0,
            should_quit: false,
        }
    }

    pub fn set_issues(&mut self, issues: Vec<IssueSummary>) {
        self.issues = issues;
        self.sort_issues();
        self.clamp_selection();
        self.clear_stale_detail();
        self.clear_missing_issue_highlights();
    }

    pub fn selected_issue(&self) -> Option<&IssueSummary> {
        self.issues.get(self.selected_index)
    }

    pub fn select_issue_number(&mut self, number: u64) {
        let previous = self.selected_issue().map(|issue| issue.number);
        if let Some(index) = self.issues.iter().position(|issue| issue.number == number) {
            self.selected_index = index;
        }
        if self.selected_issue().map(|issue| issue.number) != previous {
            self.detail_scroll = 0;
        }
    }

    pub fn select_issue_index(&mut self, index: usize) {
        let previous = self.selected_issue().map(|issue| issue.number);
        if self.issues.is_empty() {
            self.selected_index = 0;
        } else {
            self.selected_index = index.min(self.issues.len() - 1);
        }
        if self.selected_issue().map(|issue| issue.number) != previous {
            self.detail_scroll = 0;
        }
    }

    pub fn upsert_issue_at_top(&mut self, issue: IssueSummary) {
        let previous = self.selected_issue().map(|issue| issue.number);
        self.issues.retain(|item| item.number != issue.number);
        self.issues.insert(0, issue);
        self.selected_index = 0;
        if self.selected_issue().map(|issue| issue.number) != previous {
            self.detail_scroll = 0;
        }
        self.clear_stale_detail();
        self.clear_missing_issue_highlights();
    }

    pub fn select_next(&mut self) {
        let previous = self.selected_issue().map(|issue| issue.number);
        if self.issues.is_empty() {
            self.selected_index = 0;
            return;
        }

        self.selected_index = (self.selected_index + 1).min(self.issues.len() - 1);
        if self.selected_issue().map(|issue| issue.number) != previous {
            self.detail_scroll = 0;
        }
    }

    pub fn select_previous(&mut self) {
        let previous = self.selected_issue().map(|issue| issue.number);
        self.selected_index = self.selected_index.saturating_sub(1);
        if self.selected_issue().map(|issue| issue.number) != previous {
            self.detail_scroll = 0;
        }
    }

    pub fn cycle_state_filter(&mut self) {
        self.filters.state = match self.filters.state {
            IssueStateFilter::Open => IssueStateFilter::Closed,
            IssueStateFilter::Closed => IssueStateFilter::All,
            IssueStateFilter::All => IssueStateFilter::Open,
        };
    }

    pub fn cycle_sort(&mut self) {
        self.filters.sort = match self.filters.sort {
            IssueSort::Updated => IssueSort::Created,
            IssueSort::Created => IssueSort::Comments,
            IssueSort::Comments => IssueSort::Assignee,
            IssueSort::Assignee => IssueSort::Updated,
        };
        self.sort_issues();
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.filters.query = query.into();
    }

    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
    }

    pub fn begin_action(&mut self, action: PendingAction, status: impl Into<String>) {
        self.pending_action = Some(action);
        self.mode = UiMode::Loading;
        self.flash = Some(FlashKind::Refresh);
        self.set_status(status);
    }

    pub fn finish_action(&mut self) {
        self.pending_action = None;
    }

    pub fn set_viewer_login(&mut self, login: impl Into<String>) {
        self.viewer_login = Some(login.into());
    }

    pub fn highlight_new_issues(&mut self, numbers: Vec<u64>) {
        self.highlight_issues(numbers, IssueHighlightKind::New);
    }

    pub fn highlight_mentioned_issues(&mut self, numbers: Vec<u64>) {
        self.highlight_issues(numbers, IssueHighlightKind::Mention);
    }

    fn highlight_issues(&mut self, numbers: Vec<u64>, kind: IssueHighlightKind) {
        self.issue_highlights
            .retain(|highlight| highlight.kind != kind && !numbers.contains(&highlight.number));
        self.issue_highlights
            .extend(numbers.iter().copied().map(|number| IssueHighlight {
                number,
                kind: kind.clone(),
            }));

        if !self.issue_highlights.is_empty() {
            self.new_issue_animation_frame = 0;
        }
    }

    pub fn advance_new_issue_animation(&mut self) {
        if self.issue_highlights.is_empty() {
            return;
        }

        self.new_issue_animation_frame = self.new_issue_animation_frame.saturating_add(1);
        if self.new_issue_animation_frame >= NEW_ISSUE_ANIMATION_FRAMES {
            self.issue_highlights.clear();
            self.new_issue_animation_frame = 0;
        }
    }

    pub fn advance_activity_indicator(&mut self) {
        self.activity_frame = self.activity_frame.wrapping_add(1);
    }

    pub fn scroll_detail_down(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_add(3);
    }

    pub fn scroll_detail_up(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(3);
    }

    pub fn scroll_detail_page_down(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_add(8);
    }

    pub fn scroll_detail_page_up(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(8);
    }

    pub fn is_new_issue_highlighted(&self, number: u64) -> bool {
        self.issue_highlight_kind(number) == Some(IssueHighlightKind::New)
    }

    pub fn issue_highlight_kind(&self, number: u64) -> Option<IssueHighlightKind> {
        self.issue_highlights
            .iter()
            .find(|highlight| highlight.number == number)
            .map(|highlight| highlight.kind.clone())
    }

    pub fn first_mentioned_issue_number(&self) -> Option<u64> {
        self.issue_highlights
            .iter()
            .find(|highlight| highlight.kind == IssueHighlightKind::Mention)
            .map(|highlight| highlight.number)
    }

    pub fn set_selected_detail(&mut self, detail: IssueDetail) {
        let previous = self
            .selected_detail
            .as_ref()
            .map(|detail| detail.summary.number);
        if previous != Some(detail.summary.number) {
            self.detail_scroll = 0;
        }
        self.selected_detail = Some(detail);
        self.comments_expanded = true;
    }

    pub fn clear_selected_detail(&mut self) {
        self.selected_detail = None;
        self.detail_scroll = 0;
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
        self.selected_template_index = 0;
        self.mode = UiMode::NewIssue;
    }

    pub fn set_repo_labels(&mut self, labels: Vec<Label>) {
        self.repo_labels = labels;
    }

    pub fn set_repo_issue_templates(&mut self, templates: Vec<IssueTemplate>) {
        self.repo_issue_templates = templates;
        self.selected_template_index = self
            .selected_template_index
            .min(self.repo_issue_templates.len().saturating_sub(1));
    }

    pub fn apply_next_issue_template(&mut self) -> bool {
        if self.repo_issue_templates.is_empty() {
            return false;
        }

        let template = &self.repo_issue_templates[self.selected_template_index];
        if self.input.trim().is_empty() {
            self.input = template.name.clone();
        }
        self.body_input = template.body.clone();
        self.selected_template_index =
            (self.selected_template_index + 1) % self.repo_issue_templates.len();
        true
    }

    pub fn set_repo_collaborators(&mut self, mut collaborators: Vec<User>) {
        collaborators.sort_by(|left, right| left.login.cmp(&right.login));
        collaborators.dedup_by(|left, right| left.login == right.login);
        self.repo_collaborators = collaborators;
        self.clamp_picker();
    }

    pub fn reset_picker(&mut self) {
        self.input.clear();
        self.picker_index = 0;
    }

    pub fn select_next_picker_item(&mut self, item_count: usize) {
        if item_count == 0 {
            self.picker_index = 0;
        } else {
            self.picker_index = (self.picker_index + 1).min(item_count - 1);
        }
    }

    pub fn select_previous_picker_item(&mut self) {
        self.picker_index = self.picker_index.saturating_sub(1);
    }

    pub fn assignee_filter_choices(&self) -> Vec<AssigneeChoice> {
        self.assignee_choices(true)
    }

    pub fn assignee_assignment_choices(&self) -> Vec<AssigneeChoice> {
        self.assignee_choices(false)
    }

    pub fn begin_assignee_edit(&mut self) {
        self.input.clear();
        self.picker_index = 0;
        self.editing_assignees = self
            .selected_issue()
            .map(|issue| {
                issue
                    .assignees
                    .iter()
                    .map(|assignee| assignee.login.clone())
                    .collect()
            })
            .unwrap_or_default();
        self.editing_assignees.sort();
        self.editing_assignees.dedup();
        self.mode = UiMode::AssigneeEditor;
    }

    pub fn toggle_editing_assignee(&mut self, login: &str) {
        if let Some(index) = self
            .editing_assignees
            .iter()
            .position(|selected| selected == login)
        {
            self.editing_assignees.remove(index);
        } else {
            self.editing_assignees.push(login.to_string());
            self.editing_assignees.sort();
            self.editing_assignees.dedup();
        }
    }

    pub fn clear_editing_assignees(&mut self) {
        self.editing_assignees.clear();
    }

    pub fn begin_issue_label_edit(&mut self) {
        self.input.clear();
        self.picker_index = 0;
        self.editing_issue_labels = self
            .selected_issue()
            .map(|issue| {
                issue
                    .labels
                    .iter()
                    .map(|label| label.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        self.mode = UiMode::IssueLabelEditor;
    }

    pub fn begin_issue_edit(&mut self, body: impl Into<String>) {
        let title = self
            .selected_issue()
            .map(|issue| issue.title.clone())
            .unwrap_or_default();
        self.input = title;
        self.body_input = body.into();
        self.issue_edit_field = IssueEditField::Title;
        self.mode = UiMode::IssueEditor;
    }

    pub fn next_issue_edit_field(&mut self) {
        self.issue_edit_field = match self.issue_edit_field {
            IssueEditField::Title => IssueEditField::Body,
            IssueEditField::Body => IssueEditField::Title,
        };
    }

    pub fn issue_label_choices(&self) -> Vec<String> {
        let query = self.input.trim().to_lowercase();
        self.repo_labels
            .iter()
            .map(|label| label.name.as_str())
            .filter(|name| query.is_empty() || name.to_lowercase().contains(&query))
            .map(ToOwned::to_owned)
            .collect()
    }

    pub fn toggle_editing_issue_label(&mut self, label: &str) {
        if let Some(index) = self
            .editing_issue_labels
            .iter()
            .position(|selected| selected == label)
        {
            self.editing_issue_labels.remove(index);
        } else {
            self.editing_issue_labels.push(label.to_string());
            self.editing_issue_labels.sort();
        }
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

    pub fn input_mention_suggestions(&self) -> Vec<String> {
        mention_suggestions(&self.repo_collaborators, &self.input)
    }

    pub fn body_mention_suggestions(&self) -> Vec<String> {
        mention_suggestions(&self.repo_collaborators, &self.body_input)
    }

    pub fn complete_input_mention(&mut self) -> bool {
        complete_mention(&self.repo_collaborators, &mut self.input)
    }

    pub fn complete_body_mention(&mut self) -> bool {
        complete_mention(&self.repo_collaborators, &mut self.body_input)
    }

    fn clamp_selection(&mut self) {
        if self.issues.is_empty() {
            self.selected_index = 0;
        } else {
            self.selected_index = self.selected_index.min(self.issues.len() - 1);
        }
    }

    fn sort_issues(&mut self) {
        match self.filters.sort {
            IssueSort::Updated => self.issues.sort_by(compare_updated),
            IssueSort::Created => self.issues.sort_by(compare_created),
            IssueSort::Comments => self.issues.sort_by(compare_comments),
            IssueSort::Assignee => self.issues.sort_by(compare_assignee),
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

    fn clear_missing_issue_highlights(&mut self) {
        self.issue_highlights.retain(|highlight| {
            self.issues
                .iter()
                .any(|issue| issue.number == highlight.number)
        });
        if self.issue_highlights.is_empty() {
            self.new_issue_animation_frame = 0;
        }
    }

    fn assignee_choices(&self, include_any: bool) -> Vec<AssigneeChoice> {
        let query = self.input.trim().to_lowercase();
        let mut choices = Vec::new();
        if include_any {
            choices.push(AssigneeChoice::Any);
        }
        choices.push(AssigneeChoice::Me);
        choices.push(AssigneeChoice::Unassigned);
        choices.extend(
            self.repo_collaborators
                .iter()
                .map(|user| AssigneeChoice::User(user.login.clone())),
        );

        choices
            .into_iter()
            .filter(|choice| query.is_empty() || choice.label().to_lowercase().contains(&query))
            .collect()
    }

    fn clamp_picker(&mut self) {
        let item_count = match self.mode {
            UiMode::AssigneeFilter => self.assignee_filter_choices().len(),
            UiMode::AssigneeEditor => self.assignee_assignment_choices().len(),
            UiMode::IssueLabelEditor => self.issue_label_choices().len(),
            _ => 0,
        };
        if item_count == 0 {
            self.picker_index = 0;
        } else {
            self.picker_index = self.picker_index.min(item_count - 1);
        }
    }
}

fn mention_suggestions(collaborators: &[User], text: &str) -> Vec<String> {
    let Some((_, prefix)) = active_mention_range(text) else {
        return Vec::new();
    };
    let prefix = prefix.to_lowercase();

    collaborators
        .iter()
        .map(|user| user.login.as_str())
        .filter(|login| prefix.is_empty() || login.to_lowercase().starts_with(&prefix))
        .take(5)
        .map(ToOwned::to_owned)
        .collect()
}

fn complete_mention(collaborators: &[User], text: &mut String) -> bool {
    let Some((start, _)) = active_mention_range(text) else {
        return false;
    };
    let Some(login) = mention_suggestions(collaborators, text).into_iter().next() else {
        return false;
    };

    text.replace_range(start.., &format!("@{login} "));
    true
}

fn active_mention_range(text: &str) -> Option<(usize, &str)> {
    let start = text.rfind('@')?;
    if start > 0 {
        let before = text[..start].chars().next_back()?;
        if before.is_ascii_alphanumeric() || before == '-' {
            return None;
        }
    }

    let prefix = &text[start + 1..];
    if prefix
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        Some((start, prefix))
    } else {
        None
    }
}

fn compare_updated(left: &IssueSummary, right: &IssueSummary) -> Ordering {
    right.updated_at.cmp(&left.updated_at)
}

fn compare_created(left: &IssueSummary, right: &IssueSummary) -> Ordering {
    right.created_at.cmp(&left.created_at)
}

fn compare_comments(left: &IssueSummary, right: &IssueSummary) -> Ordering {
    right
        .comment_count
        .cmp(&left.comment_count)
        .then_with(|| compare_updated(left, right))
}

fn compare_assignee(left: &IssueSummary, right: &IssueSummary) -> Ordering {
    let left_assignee = left
        .assignees
        .first()
        .map(|assignee| assignee.login.as_str())
        .unwrap_or("~");
    let right_assignee = right
        .assignees
        .first()
        .map(|assignee| assignee.login.as_str())
        .unwrap_or("~");
    left_assignee
        .cmp(right_assignee)
        .then_with(|| compare_updated(left, right))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{IssueState, IssueSummary, IssueTemplate, User};

    fn issue(number: u64, title: &str) -> IssueSummary {
        IssueSummary {
            number,
            title: title.to_string(),
            state: IssueState::Open,
            labels: Vec::new(),
            assignees: Vec::new(),
            author: None,
            created_at: None,
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
    fn cycles_sort_mode_and_orders_by_assignee() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.filters.sort = IssueSort::Assignee;
        let unassigned = issue(1, "one");
        let mut assigned = issue(2, "two");
        assigned.assignees = vec![User {
            login: "alice".to_string(),
        }];
        app.set_issues(vec![unassigned, assigned]);

        assert_eq!(app.issues[0].number, 2);

        app.cycle_sort();
        assert_eq!(app.filters.sort, IssueSort::Updated);
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
    fn applies_issue_template_to_new_issue_body() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_repo_issue_templates(vec![IssueTemplate {
            name: "bug report".to_string(),
            body: "## Expected\n".to_string(),
        }]);

        assert!(app.apply_next_issue_template());
        assert_eq!(app.input, "bug report");
        assert_eq!(app.body_input, "## Expected\n");
    }

    #[test]
    fn tracks_pending_action_for_loading_feedback() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());

        app.begin_action(PendingAction::AddComment, "Adding comment");

        assert_eq!(app.mode, UiMode::Loading);
        assert_eq!(app.pending_action, Some(PendingAction::AddComment));
        assert_eq!(app.status, "Adding comment");
        assert_eq!(app.flash, Some(FlashKind::Refresh));

        app.finish_action();
        assert_eq!(app.pending_action, None);
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

    #[test]
    fn filters_collaborator_choices_from_picker_input() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_repo_collaborators(vec![
            User {
                login: "alice".to_string(),
            },
            User {
                login: "bob".to_string(),
            },
        ]);
        app.input = "ali".to_string();

        assert_eq!(
            app.assignee_filter_choices(),
            vec![AssigneeChoice::User("alice".to_string())]
        );
    }

    #[test]
    fn suggests_and_completes_collaborator_mentions_from_text() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_repo_collaborators(vec![
            User {
                login: "alice".to_string(),
            },
            User {
                login: "bob".to_string(),
            },
        ]);
        app.input = "Can @al".to_string();
        app.body_input = "cc @b".to_string();

        assert_eq!(app.input_mention_suggestions(), vec!["alice".to_string()]);
        assert!(app.complete_input_mention());
        assert_eq!(app.input, "Can @alice ");

        assert_eq!(app.body_mention_suggestions(), vec!["bob".to_string()]);
        assert!(app.complete_body_mention());
        assert_eq!(app.body_input, "cc @bob ");
    }

    #[test]
    fn toggles_selected_issue_labels_for_editing() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.toggle_editing_issue_label("bug");
        app.toggle_editing_issue_label("docs");
        app.toggle_editing_issue_label("bug");

        assert_eq!(app.editing_issue_labels, vec!["docs"]);
    }

    #[test]
    fn toggles_selected_assignees_for_editing() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.toggle_editing_assignee("alice");
        app.toggle_editing_assignee("bob");
        app.toggle_editing_assignee("alice");

        assert_eq!(app.editing_assignees, vec!["bob"]);
    }

    #[test]
    fn new_issue_highlights_expire_after_animation_frames() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_issues(vec![issue(1, "one"), issue(2, "two")]);

        app.highlight_new_issues(vec![2]);
        assert!(app.is_new_issue_highlighted(2));

        for _ in 0..NEW_ISSUE_ANIMATION_FRAMES {
            app.advance_new_issue_animation();
        }

        assert!(!app.is_new_issue_highlighted(2));
        assert_eq!(app.new_issue_animation_frame, 0);
    }

    #[test]
    fn new_issue_highlights_drop_when_issue_disappears() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_issues(vec![issue(1, "one"), issue(2, "two")]);
        app.highlight_new_issues(vec![2]);

        app.set_issues(vec![issue(1, "one")]);

        assert!(app.issue_highlights.is_empty());
        assert_eq!(app.new_issue_animation_frame, 0);
    }

    #[test]
    fn mention_highlights_are_tracked_separately_from_new_issues() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_issues(vec![issue(1, "one"), issue(2, "two")]);

        app.highlight_new_issues(vec![1]);
        app.highlight_mentioned_issues(vec![2]);

        assert_eq!(app.issue_highlight_kind(1), Some(IssueHighlightKind::New));
        assert_eq!(
            app.issue_highlight_kind(2),
            Some(IssueHighlightKind::Mention)
        );
    }
}
