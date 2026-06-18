use std::{cmp::Ordering, collections::HashMap};

use serde::{Deserialize, Serialize};

use crate::{
    config::{ProjectBoardConfig, SavedView},
    domain::{
        IssueDetail, IssueSummary, IssueTemplate, Label, ProjectBoard, ProjectBoardSummary, User,
    },
    repo::Repository,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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
    IssueDetail,
    IssueDetailClosing,
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
    ProjectBoardPicker,
    ConfirmClose,
    Doctor,
    Success,
    Loading,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IssueView {
    List,
    Board,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextCursorMove {
    Left,
    Right,
    Home,
    End,
    Up,
    Down,
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
    UpdateProjectItem,
    CreateBranch,
    RepairAuth,
    RunDoctor,
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
    DetailOpen,
    DetailClose,
}

pub fn dependency_issue_numbers(text: &str) -> Vec<u64> {
    let mut numbers = Vec::new();
    for line in text.lines() {
        let normalized = line.to_lowercase();
        if !relationship_line(&normalized) {
            continue;
        }
        for number in issue_references(line) {
            if !numbers.contains(&number) {
                numbers.push(number);
            }
        }
    }
    numbers
}

fn relationship_line(line: &str) -> bool {
    [
        "depends on",
        "dependency",
        "dependencies",
        "blocked by",
        "blocker",
        "requires",
        "required by",
        "parent",
        "after",
    ]
    .iter()
    .any(|marker| line.contains(marker))
}

fn issue_references(line: &str) -> Vec<u64> {
    let mut numbers = Vec::new();
    let chars = line.char_indices().collect::<Vec<_>>();
    let mut cursor = 0;
    while cursor < chars.len() {
        let (_, character) = chars[cursor];
        if character != '#' {
            cursor += 1;
            continue;
        }

        let start = cursor + 1;
        let mut end = start;
        while end < chars.len() && chars[end].1.is_ascii_digit() {
            end += 1;
        }
        if end > start {
            let byte_start = chars[start].0;
            let byte_end = chars.get(end).map_or(line.len(), |(index, _)| *index);
            if let Ok(number) = line[byte_start..byte_end].parse::<u64>() {
                numbers.push(number);
            }
        }
        cursor = end.max(cursor + 1);
    }
    numbers
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
pub struct ErrorDetail {
    pub title: String,
    pub details: String,
    pub hint: Option<String>,
    pub remediation: Option<ErrorRemediation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorRemediation {
    pub label: String,
    pub command: String,
    pub scopes: Vec<String>,
    pub retry: Option<RetryAction>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RetryAction {
    OpenProjectBoardChooser { force_picker: bool },
    LoadProjectBoard,
    MoveBoardItem(BoardMoveTarget),
    SubmitNewIssue { force_create: bool },
    RunDoctor,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoardMoveTarget {
    pub option_id: String,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DoctorCheckStatus {
    Pass,
    Warn,
    Fail,
}

impl DoctorCheckStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DoctorCheck {
    pub name: String,
    pub status: DoctorCheckStatus,
    pub detail: String,
    pub remediation: Option<ErrorRemediation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct App {
    pub repo: Repository,
    pub filters: IssueFilters,
    pub issues: Vec<IssueSummary>,
    pub selected_index: usize,
    pub issue_view: IssueView,
    pub mode: UiMode,
    pub status: String,
    pub input: String,
    pub body_input: String,
    pub label_input: String,
    input_cursor: Option<usize>,
    body_cursor: Option<usize>,
    label_cursor: Option<usize>,
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
    pub relationship_details: HashMap<u64, IssueDetail>,
    pub relationship_tree_enabled: bool,
    pub comments_expanded: bool,
    pub pending_action: Option<PendingAction>,
    pub flash: Option<FlashKind>,
    pub viewer_login: Option<String>,
    pub issue_highlights: Vec<IssueHighlight>,
    pub project_board: Option<ProjectBoard>,
    pub project_board_choices: Vec<ProjectBoardSummary>,
    pub project_board_config: ProjectBoardConfig,
    pub error_detail: Option<ErrorDetail>,
    pub doctor_checks: Vec<DoctorCheck>,
    pub saved_views: Vec<SavedView>,
    pub active_view: Option<String>,
    pub triage_mode: bool,
    pub pending_writes: Vec<String>,
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
            issue_view: IssueView::List,
            mode: UiMode::Browsing,
            status: "Ready".to_string(),
            input: String::new(),
            body_input: String::new(),
            label_input: String::new(),
            input_cursor: None,
            body_cursor: None,
            label_cursor: None,
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
            relationship_details: HashMap::new(),
            relationship_tree_enabled: false,
            comments_expanded: true,
            pending_action: None,
            flash: None,
            viewer_login: None,
            issue_highlights: Vec::new(),
            project_board: None,
            project_board_choices: Vec::new(),
            project_board_config: ProjectBoardConfig::default(),
            error_detail: None,
            doctor_checks: Vec::new(),
            saved_views: Vec::new(),
            active_view: None,
            triage_mode: false,
            pending_writes: Vec::new(),
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

    pub fn set_issue_view(&mut self, view: IssueView) {
        self.issue_view = view;
        self.mode = UiMode::Browsing;
    }

    pub fn cycle_issue_view(&mut self) {
        self.issue_view = match self.issue_view {
            IssueView::List => IssueView::Board,
            IssueView::Board => IssueView::List,
        };
        self.mode = UiMode::Browsing;
    }

    pub fn cycle_issue_view_reverse(&mut self) {
        self.issue_view = match self.issue_view {
            IssueView::Board => IssueView::List,
            IssueView::List => IssueView::Board,
        };
        self.mode = UiMode::Browsing;
    }

    pub fn cycle_state_filter(&mut self) {
        self.active_view = None;
        self.filters.state = match self.filters.state {
            IssueStateFilter::Open => IssueStateFilter::Closed,
            IssueStateFilter::Closed => IssueStateFilter::All,
            IssueStateFilter::All => IssueStateFilter::Open,
        };
    }

    pub fn cycle_sort(&mut self) {
        self.active_view = None;
        self.filters.sort = match self.filters.sort {
            IssueSort::Updated => IssueSort::Created,
            IssueSort::Created => IssueSort::Comments,
            IssueSort::Comments => IssueSort::Assignee,
            IssueSort::Assignee => IssueSort::Updated,
        };
        self.sort_issues();
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.active_view = None;
        self.filters.query = query.into();
    }

    pub fn clear_filters(&mut self) {
        self.active_view = None;
        let sort = self.filters.sort.clone();
        self.filters = IssueFilters {
            state: IssueStateFilter::All,
            sort,
            ..IssueFilters::default()
        };
    }

    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
    }

    pub fn show_error(
        &mut self,
        title: impl Into<String>,
        details: impl Into<String>,
        hint: Option<String>,
    ) {
        let title = title.into();
        self.error_detail = Some(ErrorDetail {
            title: title.clone(),
            details: details.into(),
            hint,
            remediation: None,
        });
        self.mode = UiMode::Error;
        self.flash = Some(FlashKind::Error);
        self.set_status(title);
    }

    pub fn show_error_with_remediation(
        &mut self,
        title: impl Into<String>,
        details: impl Into<String>,
        hint: Option<String>,
        remediation: Option<ErrorRemediation>,
    ) {
        let title = title.into();
        self.error_detail = Some(ErrorDetail {
            title: title.clone(),
            details: details.into(),
            hint,
            remediation,
        });
        self.mode = UiMode::Error;
        self.flash = Some(FlashKind::Error);
        self.set_status(title);
    }

    pub fn clear_error(&mut self) {
        self.error_detail = None;
    }

    pub fn error_detail_has_remediation(&self) -> bool {
        self.error_detail
            .as_ref()
            .and_then(|error| error.remediation.as_ref())
            .is_some()
    }

    pub fn set_doctor_checks(&mut self, checks: Vec<DoctorCheck>) {
        self.doctor_checks = checks;
        self.picker_index = self
            .picker_index
            .min(self.doctor_checks.len().saturating_sub(1));
    }

    pub fn selected_doctor_remediation(&self) -> Option<&ErrorRemediation> {
        self.doctor_checks
            .get(self.picker_index)
            .and_then(|check| check.remediation.as_ref())
    }

    pub fn set_project_board(&mut self, board: ProjectBoard) {
        self.project_board = Some(board);
    }

    pub fn clear_project_board(&mut self) {
        self.project_board = None;
    }

    pub fn set_project_board_choices(&mut self, choices: Vec<ProjectBoardSummary>) {
        self.project_board_choices = choices;
        self.picker_index = self
            .picker_index
            .min(self.project_board_choices.len().saturating_sub(1));
    }

    pub fn open_project_board_picker(&mut self, choices: Vec<ProjectBoardSummary>) {
        self.set_project_board_choices(choices);
        self.reset_picker();
        self.issue_view = IssueView::Board;
        self.mode = UiMode::ProjectBoardPicker;
        self.set_status("Choose a GitHub project board");
    }

    pub fn set_project_board_config(&mut self, config: ProjectBoardConfig) {
        self.project_board_config = config;
        self.clear_project_board();
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

    pub fn begin_pending_write(&mut self, label: impl Into<String>) {
        self.pending_writes.push(label.into());
    }

    pub fn finish_pending_write(&mut self, label: &str) {
        if let Some(index) = self.pending_writes.iter().position(|item| item == label) {
            self.pending_writes.remove(index);
        }
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
        self.relationship_details
            .insert(detail.summary.number, detail.clone());
        self.selected_detail = Some(detail);
        self.comments_expanded = true;
    }

    pub fn clear_selected_detail(&mut self) {
        self.selected_detail = None;
        self.detail_scroll = 0;
    }

    pub fn clear_relationship_details(&mut self) {
        self.relationship_details.clear();
    }

    pub fn set_relationship_detail(&mut self, detail: IssueDetail) {
        self.relationship_details
            .insert(detail.summary.number, detail);
    }

    pub fn enable_relationship_tree(&mut self) {
        self.relationship_tree_enabled = true;
    }

    pub fn disable_relationship_tree(&mut self) {
        self.relationship_tree_enabled = false;
        self.relationship_details.clear();
    }

    pub fn toggle_comments(&mut self) {
        self.comments_expanded = !self.comments_expanded;
    }

    pub fn set_saved_views(&mut self, views: Vec<SavedView>) {
        self.saved_views = views;
    }

    pub fn saved_view_names(&self) -> Vec<String> {
        self.saved_views
            .iter()
            .map(|view| view.name.clone())
            .collect()
    }

    pub fn apply_saved_view(&mut self, name: &str) -> bool {
        let Some(view) = self
            .saved_views
            .iter()
            .find(|view| view.name.eq_ignore_ascii_case(name))
            .cloned()
        else {
            return false;
        };

        self.filters = view.filters;
        self.active_view = Some(view.name);
        true
    }

    pub fn toggle_triage_mode(&mut self) {
        self.triage_mode = !self.triage_mode;
        if self.triage_mode {
            self.set_status("Triage queue on");
        } else {
            self.set_status("Triage queue off");
        }
    }

    pub fn skip_triage_issue(&mut self) {
        let skipped = self.selected_issue().map(|issue| issue.number);
        self.select_next();
        if let Some(number) = skipped {
            self.set_status(format!("Skipped issue #{number}"));
        }
    }

    pub fn apply_optimistic_comment(&mut self, number: u64, comment: crate::domain::IssueComment) {
        let mut next_count = None;
        if let Some(detail) = self
            .selected_detail
            .as_mut()
            .filter(|detail| detail.summary.number == number)
        {
            if !detail.comments.iter().any(|item| item == &comment) {
                detail.comments.push(comment);
            }
            detail.summary.comment_count = detail
                .summary
                .comment_count
                .max(detail.comments.len() as u64);
            next_count = Some(detail.summary.comment_count);
        }

        if let Some(issue) = self.issues.iter_mut().find(|issue| issue.number == number) {
            issue.comment_count = next_count.unwrap_or(issue.comment_count.saturating_add(1));
        }
    }

    pub fn apply_optimistic_state(&mut self, number: u64, state: crate::domain::IssueState) {
        if let Some(issue) = self.issues.iter_mut().find(|issue| issue.number == number) {
            issue.state = state.clone();
        }
        if let Some(detail) = self
            .selected_detail
            .as_mut()
            .filter(|detail| detail.summary.number == number)
        {
            detail.summary.state = state;
        }
    }

    pub fn apply_optimistic_assignees(&mut self, number: u64, assignees: Vec<String>) {
        let assignees = assignees
            .into_iter()
            .map(|login| User { login })
            .collect::<Vec<_>>();
        if let Some(issue) = self.issues.iter_mut().find(|issue| issue.number == number) {
            issue.assignees = assignees.clone();
        }
        if let Some(detail) = self
            .selected_detail
            .as_mut()
            .filter(|detail| detail.summary.number == number)
        {
            detail.summary.assignees = assignees;
        }
    }

    pub fn apply_optimistic_labels(&mut self, number: u64, labels: Vec<String>) {
        let labels = labels
            .into_iter()
            .map(|name| Label { name })
            .collect::<Vec<_>>();
        if let Some(issue) = self.issues.iter_mut().find(|issue| issue.number == number) {
            issue.labels = labels.clone();
        }
        if let Some(detail) = self
            .selected_detail
            .as_mut()
            .filter(|detail| detail.summary.number == number)
        {
            detail.summary.labels = labels;
        }
    }

    pub fn start_new_issue(&mut self) {
        self.clear_input();
        self.clear_body_input();
        self.clear_label_input();
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

        let template = self.repo_issue_templates[self.selected_template_index].clone();
        if self.input.trim().is_empty() {
            self.set_input_text(template.name.clone());
        }
        self.set_body_input_text(template.body.clone());
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
        self.clear_input();
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
        self.clear_input();
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
        self.clear_input();
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
        self.set_input_text(title);
        self.set_body_input_text(body.into());
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
        self.clear_label_input();
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
        mention_suggestions(&self.repo_collaborators, &self.input, self.input_cursor())
    }

    pub fn body_mention_suggestions(&self) -> Vec<String> {
        mention_suggestions(
            &self.repo_collaborators,
            &self.body_input,
            self.body_cursor(),
        )
    }

    pub fn complete_input_mention(&mut self) -> bool {
        complete_mention(
            &self.repo_collaborators,
            &mut self.input,
            &mut self.input_cursor,
        )
    }

    pub fn complete_body_mention(&mut self) -> bool {
        complete_mention(
            &self.repo_collaborators,
            &mut self.body_input,
            &mut self.body_cursor,
        )
    }

    pub fn set_input_text(&mut self, text: impl Into<String>) {
        self.input = text.into();
        self.input_cursor = Some(self.input.len());
    }

    pub fn set_body_input_text(&mut self, text: impl Into<String>) {
        self.body_input = text.into();
        self.body_cursor = Some(self.body_input.len());
    }

    pub fn set_label_input_text(&mut self, text: impl Into<String>) {
        self.label_input = text.into();
        self.label_cursor = Some(self.label_input.len());
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
        self.input_cursor = Some(0);
    }

    pub fn clear_body_input(&mut self) {
        self.body_input.clear();
        self.body_cursor = Some(0);
    }

    pub fn clear_label_input(&mut self) {
        self.label_input.clear();
        self.label_cursor = Some(0);
    }

    pub fn input_cursor(&self) -> usize {
        effective_cursor(&self.input, self.input_cursor)
    }

    pub fn body_cursor(&self) -> usize {
        effective_cursor(&self.body_input, self.body_cursor)
    }

    pub fn label_cursor(&self) -> usize {
        effective_cursor(&self.label_input, self.label_cursor)
    }

    pub fn input_cursor_position(&self) -> (u16, u16) {
        cursor_position(&self.input, self.input_cursor())
    }

    pub fn body_cursor_position(&self) -> (u16, u16) {
        cursor_position(&self.body_input, self.body_cursor())
    }

    pub fn label_cursor_position(&self) -> (u16, u16) {
        cursor_position(&self.label_input, self.label_cursor())
    }

    pub fn insert_input_char(&mut self, c: char) {
        insert_char(&mut self.input, &mut self.input_cursor, c);
    }

    pub fn insert_body_char(&mut self, c: char) {
        insert_char(&mut self.body_input, &mut self.body_cursor, c);
    }

    pub fn insert_label_char(&mut self, c: char) {
        insert_char(&mut self.label_input, &mut self.label_cursor, c);
    }

    pub fn insert_input_newline(&mut self) {
        self.insert_input_char('\n');
    }

    pub fn insert_body_newline(&mut self) {
        self.insert_body_char('\n');
    }

    pub fn backspace_input(&mut self) {
        backspace(&mut self.input, &mut self.input_cursor);
    }

    pub fn backspace_body(&mut self) {
        backspace(&mut self.body_input, &mut self.body_cursor);
    }

    pub fn backspace_label(&mut self) {
        backspace(&mut self.label_input, &mut self.label_cursor);
    }

    pub fn delete_input(&mut self) {
        delete(&mut self.input, &mut self.input_cursor);
    }

    pub fn delete_body(&mut self) {
        delete(&mut self.body_input, &mut self.body_cursor);
    }

    pub fn delete_label(&mut self) {
        delete(&mut self.label_input, &mut self.label_cursor);
    }

    pub fn move_input_cursor(&mut self, movement: TextCursorMove) {
        move_cursor(&self.input, &mut self.input_cursor, movement);
    }

    pub fn move_body_cursor(&mut self, movement: TextCursorMove) {
        move_cursor(&self.body_input, &mut self.body_cursor, movement);
    }

    pub fn move_label_cursor(&mut self, movement: TextCursorMove) {
        move_cursor(&self.label_input, &mut self.label_cursor, movement);
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
            UiMode::ProjectBoardPicker => self.project_board_choices.len(),
            UiMode::Doctor => self.doctor_checks.len(),
            _ => 0,
        };
        if item_count == 0 {
            self.picker_index = 0;
        } else {
            self.picker_index = self.picker_index.min(item_count - 1);
        }
    }
}

fn mention_suggestions(collaborators: &[User], text: &str, cursor: usize) -> Vec<String> {
    let Some((_, _, prefix)) = active_mention_range(text, cursor) else {
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

fn complete_mention(collaborators: &[User], text: &mut String, cursor: &mut Option<usize>) -> bool {
    let current_cursor = effective_cursor(text, *cursor);
    let Some((start, end, _)) = active_mention_range(text, current_cursor) else {
        return false;
    };
    let Some(login) = mention_suggestions(collaborators, text, current_cursor)
        .into_iter()
        .next()
    else {
        return false;
    };

    let replacement = format!("@{login} ");
    text.replace_range(start..end, &replacement);
    *cursor = Some(start + replacement.len());
    true
}

fn active_mention_range(text: &str, cursor: usize) -> Option<(usize, usize, &str)> {
    let cursor = clamp_cursor(text, cursor);
    let before_cursor = &text[..cursor];
    let start = before_cursor.rfind('@')?;
    if start > 0 {
        let before = text[..start].chars().next_back()?;
        if before.is_ascii_alphanumeric() || before == '-' {
            return None;
        }
    }

    let prefix = &text[start + 1..cursor];
    if prefix
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        Some((start, cursor, prefix))
    } else {
        None
    }
}

fn effective_cursor(text: &str, cursor: Option<usize>) -> usize {
    clamp_cursor(text, cursor.unwrap_or(text.len()))
}

fn clamp_cursor(text: &str, cursor: usize) -> usize {
    if cursor >= text.len() {
        return text.len();
    }

    let mut cursor = cursor;
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

fn insert_char(text: &mut String, cursor: &mut Option<usize>, c: char) {
    let current = effective_cursor(text, *cursor);
    text.insert(current, c);
    *cursor = Some(current + c.len_utf8());
}

fn backspace(text: &mut String, cursor: &mut Option<usize>) {
    let current = effective_cursor(text, *cursor);
    let Some(previous) = previous_cursor(text, current) else {
        *cursor = Some(0);
        return;
    };

    text.replace_range(previous..current, "");
    *cursor = Some(previous);
}

fn delete(text: &mut String, cursor: &mut Option<usize>) {
    let current = effective_cursor(text, *cursor);
    let Some(next) = next_cursor(text, current) else {
        *cursor = Some(text.len());
        return;
    };

    text.replace_range(current..next, "");
    *cursor = Some(current);
}

fn move_cursor(text: &str, cursor: &mut Option<usize>, movement: TextCursorMove) {
    let current = effective_cursor(text, *cursor);
    *cursor = Some(match movement {
        TextCursorMove::Left => previous_cursor(text, current).unwrap_or(0),
        TextCursorMove::Right => next_cursor(text, current).unwrap_or(text.len()),
        TextCursorMove::Home => line_start(text, current),
        TextCursorMove::End => line_end(text, current),
        TextCursorMove::Up => vertical_cursor(text, current, -1),
        TextCursorMove::Down => vertical_cursor(text, current, 1),
    });
}

fn previous_cursor(text: &str, cursor: usize) -> Option<usize> {
    text[..cursor].char_indices().last().map(|(index, _)| index)
}

fn next_cursor(text: &str, cursor: usize) -> Option<usize> {
    text[cursor..]
        .chars()
        .next()
        .map(|ch| cursor + ch.len_utf8())
}

fn line_start(text: &str, cursor: usize) -> usize {
    text[..cursor].rfind('\n').map_or(0, |index| index + 1)
}

fn line_end(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .find('\n')
        .map_or(text.len(), |offset| cursor + offset)
}

fn vertical_cursor(text: &str, cursor: usize, direction: isize) -> usize {
    let start = line_start(text, cursor);
    let column = text[start..cursor].chars().count();

    if direction < 0 {
        if start == 0 {
            return cursor;
        }
        let previous_end = start - 1;
        let previous_start = line_start(text, previous_end);
        cursor_at_column(text, previous_start, previous_end, column)
    } else {
        let end = line_end(text, cursor);
        if end == text.len() {
            return cursor;
        }
        let next_start = end + 1;
        let next_end = line_end(text, next_start);
        cursor_at_column(text, next_start, next_end, column)
    }
}

fn cursor_at_column(text: &str, start: usize, end: usize, column: usize) -> usize {
    text[start..end]
        .char_indices()
        .nth(column)
        .map_or(end, |(offset, _)| start + offset)
}

fn cursor_position(text: &str, cursor: usize) -> (u16, u16) {
    let cursor = clamp_cursor(text, cursor);
    let mut row = 0u16;
    let mut column = 0u16;
    for ch in text[..cursor].chars() {
        if ch == '\n' {
            row = row.saturating_add(1);
            column = 0;
        } else {
            column = column.saturating_add(1);
        }
    }
    (row, column)
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
        let app = App::new("owner/tissues".parse().unwrap());

        assert_eq!(app.filters.state, IssueStateFilter::Open);
        assert_eq!(app.mode, UiMode::Browsing);
        assert_eq!(app.issue_view, IssueView::List);
        assert!(app.filters.query.is_empty());
    }

    #[test]
    fn cycles_between_list_and_board_issue_views() {
        let mut app = App::new("owner/tissues".parse().unwrap());

        app.cycle_issue_view();
        assert_eq!(app.issue_view, IssueView::Board);

        app.cycle_issue_view();
        assert_eq!(app.issue_view, IssueView::List);
    }

    #[test]
    fn cycles_between_list_and_board_issue_views_in_reverse() {
        let mut app = App::new("owner/tissues".parse().unwrap());

        app.cycle_issue_view_reverse();
        assert_eq!(app.issue_view, IssueView::Board);

        app.cycle_issue_view_reverse();
        assert_eq!(app.issue_view, IssueView::List);
    }

    #[test]
    fn parses_dependency_issue_references_from_relationship_lines() {
        let numbers = dependency_issue_numbers(
            "Depends on #12 and #34\nMentioned by #99\nBlocked by #12\nRequires #56.",
        );

        assert_eq!(numbers, vec![12, 34, 56]);
    }

    #[test]
    fn cycles_state_filter() {
        let mut app = App::new("owner/tissues".parse().unwrap());

        app.cycle_state_filter();
        assert_eq!(app.filters.state, IssueStateFilter::Closed);
        app.cycle_state_filter();
        assert_eq!(app.filters.state, IssueStateFilter::All);
        app.cycle_state_filter();
        assert_eq!(app.filters.state, IssueStateFilter::Open);
    }

    #[test]
    fn cycles_sort_mode_and_orders_by_assignee() {
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());

        app.set_query("render bug");

        assert_eq!(app.filters.query, "render bug");
    }

    #[test]
    fn selection_is_clamped_to_loaded_issues() {
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.set_issues(vec![issue(1, "one"), issue(2, "two")]);

        app.select_next();
        app.select_next();
        assert_eq!(app.selected_index, 1);

        app.set_issues(vec![issue(1, "one")]);
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn stores_selected_issue_detail_and_toggles_comments() {
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());

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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.toggle_editing_issue_label("bug");
        app.toggle_editing_issue_label("docs");
        app.toggle_editing_issue_label("bug");

        assert_eq!(app.editing_issue_labels, vec!["docs"]);
    }

    #[test]
    fn toggles_selected_assignees_for_editing() {
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.toggle_editing_assignee("alice");
        app.toggle_editing_assignee("bob");
        app.toggle_editing_assignee("alice");

        assert_eq!(app.editing_assignees, vec!["bob"]);
    }

    #[test]
    fn new_issue_highlights_expire_after_animation_frames() {
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.set_issues(vec![issue(1, "one"), issue(2, "two")]);
        app.highlight_new_issues(vec![2]);

        app.set_issues(vec![issue(1, "one")]);

        assert!(app.issue_highlights.is_empty());
        assert_eq!(app.new_issue_animation_frame, 0);
    }

    #[test]
    fn mention_highlights_are_tracked_separately_from_new_issues() {
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.set_issues(vec![issue(1, "one"), issue(2, "two")]);

        app.highlight_new_issues(vec![1]);
        app.highlight_mentioned_issues(vec![2]);

        assert_eq!(app.issue_highlight_kind(1), Some(IssueHighlightKind::New));
        assert_eq!(
            app.issue_highlight_kind(2),
            Some(IssueHighlightKind::Mention)
        );
    }

    #[test]
    fn applies_named_saved_views_to_filters() {
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.filters.state = IssueStateFilter::Closed;
        app.set_saved_views(vec![crate::config::SavedView {
            name: "mine".to_string(),
            filters: IssueFilters {
                state: IssueStateFilter::Open,
                assignee: AssigneeFilter::Me,
                labels: vec!["bug".to_string()],
                query: "panic".to_string(),
                sort: IssueSort::Comments,
            },
        }]);

        assert!(app.apply_saved_view("mine"));

        assert_eq!(app.active_view.as_deref(), Some("mine"));
        assert_eq!(app.filters.state, IssueStateFilter::Open);
        assert_eq!(app.filters.assignee, AssigneeFilter::Me);
        assert_eq!(app.filters.labels, vec!["bug"]);
        assert_eq!(app.filters.query, "panic");
        assert_eq!(app.filters.sort, IssueSort::Comments);
    }

    #[test]
    fn triage_mode_toggles_and_skips_selected_issue() {
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.set_issues(vec![issue(1, "one"), issue(2, "two")]);

        app.toggle_triage_mode();
        app.skip_triage_issue();

        assert!(app.triage_mode);
        assert_eq!(app.selected_issue().unwrap().number, 2);
        assert_eq!(app.status, "Skipped issue #1");
    }

    #[test]
    fn optimistic_updates_keep_visible_issue_state_current() {
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.set_issues(vec![issue(1, "one")]);
        app.set_selected_detail(crate::domain::IssueDetail {
            summary: issue(1, "one"),
            body: "Body".to_string(),
            comments: Vec::new(),
        });

        app.apply_optimistic_comment(
            1,
            crate::domain::IssueComment {
                author: Some(User {
                    login: "kpowel".to_string(),
                }),
                body: "queued".to_string(),
                created_at: None,
            },
        );
        app.apply_optimistic_state(1, crate::domain::IssueState::Closed);
        app.apply_optimistic_assignees(1, vec!["kpowel".to_string()]);
        app.apply_optimistic_labels(1, vec!["bug".to_string()]);

        let issue = app.selected_issue().unwrap();
        assert_eq!(issue.comment_count, 1);
        assert_eq!(issue.state, crate::domain::IssueState::Closed);
        assert_eq!(issue.assignees[0].login, "kpowel");
        assert_eq!(issue.labels[0].name, "bug");
        let detail = app.selected_detail.as_ref().unwrap();
        assert_eq!(detail.comments[0].body, "queued");
        assert_eq!(detail.summary.state, crate::domain::IssueState::Closed);
    }
}
