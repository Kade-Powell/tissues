use std::time::Duration as StdDuration;

use chrono::Utc;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    symbols::border::Set,
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, HighlightSpacing, Paragraph, Row, Table, TableState, Widget,
        Wrap,
    },
};
use ratatui_textarea::{CursorMove, TextArea};
use tachyonfx::{EffectManager, Interpolation, fx};
use tui_tree_widget::{Tree, TreeItem, TreeState};

use crate::{
    app::{
        App, AssigneeChoice, AssigneeFilter, FlashKind, IssueEditField, IssueHighlightKind,
        IssueStateFilter, NewIssueField, PendingAction, UiMode,
    },
    domain::{IssueComment, IssueDetail, IssueState, IssueSummary},
};

const ISSUE_LIST_PERCENT: u16 = 40;
const DETAIL_PANEL_PERCENT: u16 = 60;
const COMMENT_PRIMARY_LABEL: &str = "Submit";
const NEW_ISSUE_PRIMARY_LABEL: &str = "Create";
const ISSUE_EDIT_PRIMARY_LABEL: &str = "Save";
const ASSIGNEE_FILTER_PRIMARY_LABEL: &str = "Apply";
const ASSIGNEE_EDITOR_PRIMARY_LABEL: &str = "Assign";
const ISSUE_LABEL_PRIMARY_LABEL: &str = "Save";

const TITLE_ACCENT: Color = Color::Magenta;
const LABEL_ACCENT: Color = Color::Magenta;
const ISSUE_ACCENT: Color = Color::Magenta;
const ERROR_ACCENT: Color = Color::Red;
const WARNING_ACCENT: Color = Color::Yellow;
const NOTICE_ACCENT: Color = Color::Yellow;
const OPEN_ACCENT: Color = Color::Green;
const PICKER_ACCENT: Color = Color::Cyan;
const ACTION_ACCENT: Color = Color::Cyan;
const DETAIL_ACCENT: Color = Color::Blue;
const DEFAULT_FG: Color = Color::Reset;
const MUTED_FG: Color = Color::Gray;
const DIM_FG: Color = Color::DarkGray;
const CLOSED_FG: Color = Color::DarkGray;
const ACTION_FRAME_ACCENT: Color = Color::DarkGray;

const EXABIND_FRAME: Set = Set {
    top_left: "▟",
    top_right: "▜",
    bottom_left: "▔",
    bottom_right: "▔",
    vertical_left: "▏",
    vertical_right: "▕",
    horizontal_top: "▔",
    horizontal_bottom: "▔",
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MouseTarget {
    IssueRow(usize),
    DetailPanel,
    NewIssueField(NewIssueField),
    IssueEditField(IssueEditField),
    PickerItem(usize),
    PrimaryAction,
    CancelAction,
}

#[derive(Default)]
pub struct WorktrackEffects {
    manager: EffectManager<String>,
}

impl WorktrackEffects {
    pub fn trigger_startup_loading(&mut self) {
        self.add_coalesce_effect("startup-loading");
    }

    pub fn trigger_refresh(&mut self) {
        self.add_coalesce_effect("routine-loading");
    }

    pub fn trigger_success(&mut self) {
        self.add_coalesce_effect("success");
    }

    pub fn trigger_error(&mut self) {
        self.manager.add_unique_effect(
            "error",
            fx::parallel(&[
                fx::dissolve_to(Style::new().fg(ERROR_ACCENT), (360, Interpolation::SineOut)),
                fx::fade_to_fg(ERROR_ACCENT, (350, Interpolation::SineOut)),
            ]),
        );
    }

    pub fn process(&mut self, elapsed: StdDuration, buffer: &mut Buffer, area: Rect) {
        self.manager.process_effects(elapsed.into(), buffer, area);
    }

    pub fn has_effects(&self) -> bool {
        self.manager.is_running()
    }

    fn add_coalesce_effect(&mut self, id: &'static str) {
        self.manager.add_unique_effect(
            id,
            fx::coalesce_from(Style::new().fg(DIM_FG), (600, Interpolation::BounceInOut)),
        );
    }
}

pub fn trigger_flash_effect(app: &mut App, effects: &mut WorktrackEffects) {
    match app.flash.take() {
        Some(FlashKind::Refresh) => effects.trigger_refresh(),
        Some(FlashKind::Success) => effects.trigger_success(),
        Some(FlashKind::Error) => effects.trigger_error(),
        None => {}
    }
}

pub fn render(app: &App, area: Rect, buffer: &mut Buffer) {
    Block::new().style(page_style()).render(area, buffer);

    if app.mode == UiMode::Command {
        let rows = command_mode_rows(area);
        render_header(app, rows[0], buffer);
        render_filters(app, rows[1], buffer);
        render_body(app, rows[2], buffer);
        render_command_bar(app, rows[3], buffer);
        render_footer(app, rows[4], buffer);
    } else {
        let rows = main_rows(area);
        render_header(app, rows[0], buffer);
        render_filters(app, rows[1], buffer);
        render_body(app, rows[2], buffer);
        render_footer(app, rows[3], buffer);
    }

    render_overlay(app, area, buffer);
}

fn page_style() -> Style {
    Style::new().fg(DEFAULT_FG)
}

fn surface_style() -> Style {
    Style::new().fg(DEFAULT_FG)
}

fn modal_style() -> Style {
    Style::new().fg(DEFAULT_FG)
}

fn frame_block(title: impl Into<String>, accent: Color) -> Block<'static> {
    let title = format!(" {} ", title.into());

    Block::bordered()
        .border_set(EXABIND_FRAME)
        .border_style(Style::new().fg(accent))
        .title(Span::styled(
            title,
            Style::new()
                .fg(accent)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::REVERSED),
        ))
        .style(surface_style())
}

fn modal_block(title: impl Into<String>, accent: Color) -> Block<'static> {
    let title = format!(" {} ", title.into());

    Block::bordered()
        .border_set(EXABIND_FRAME)
        .border_style(Style::new().fg(accent))
        .title(Span::styled(
            title,
            Style::new()
                .fg(accent)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::REVERSED),
        ))
        .style(modal_style())
}

pub fn effect_area(app: &App, area: Rect) -> Rect {
    match app.mode {
        UiMode::Search
        | UiMode::CommentComposer
        | UiMode::CloseComment
        | UiMode::NewIssue
        | UiMode::IssueEditor
        | UiMode::AssigneeFilter
        | UiMode::AssigneeEditor
        | UiMode::IssueLabelEditor
        | UiMode::ConfirmClose
        | UiMode::Success
        | UiMode::Loading
        | UiMode::Error => centered_rect(72, 55, area),
        UiMode::Command => command_bar_area(area),
        UiMode::Browsing => issue_list_area(area),
        UiMode::FilterEditor => area,
    }
}

fn render_header(app: &App, area: Rect, buffer: &mut Buffer) {
    let open = app
        .issues
        .iter()
        .filter(|issue| issue.state == IssueState::Open)
        .count();
    let closed = app
        .issues
        .iter()
        .filter(|issue| issue.state == IssueState::Closed)
        .count();
    let header = Line::from(vec![
        Span::styled(app.repo.to_string(), Style::new().fg(ACTION_ACCENT).bold()),
        Span::styled(
            format!(
                "    open: {open}  closed: {closed}  all: {}",
                app.issues.len()
            ),
            Style::new().fg(MUTED_FG),
        ),
    ]);

    Paragraph::new(header)
        .style(page_style())
        .render(area, buffer);
}

fn render_filters(app: &App, area: Rect, buffer: &mut Buffer) {
    let state = match app.filters.state {
        IssueStateFilter::Open => "open",
        IssueStateFilter::Closed => "closed",
        IssueStateFilter::All => "all",
    };
    let labels = if app.filters.labels.is_empty() {
        "any".to_string()
    } else {
        app.filters.labels.join(", ")
    };
    let assignee = app.filters.assignee.label();
    let filters = format!(
        "[State: {state}] [Assignee: {assignee}] [Labels: {labels}] [Search: {}] [Sort: {}]",
        app.filters.query,
        app.filters.sort.label()
    );

    Paragraph::new(filters)
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::new().fg(DIM_FG))
                .style(page_style()),
        )
        .style(Style::new().fg(NOTICE_ACCENT))
        .render(area, buffer);
}

fn render_body(app: &App, area: Rect, buffer: &mut Buffer) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(ISSUE_LIST_PERCENT),
            Constraint::Percentage(DETAIL_PANEL_PERCENT),
        ])
        .split(area);

    let widths = [
        Constraint::Length(6),
        Constraint::Length(5),
        Constraint::Length(9),
        Constraint::Length(4),
        Constraint::Length(5),
        Constraint::Min(12),
    ];
    let header = Row::new(["Number", "State", "Team", "Age", "Tags", "Title"])
        .style(Style::new().fg(NOTICE_ACCENT).add_modifier(Modifier::BOLD))
        .bottom_margin(1);
    let rows = app.issues.iter().map(|issue| {
        issue_row(
            issue,
            app.issue_highlight_kind(issue.number),
            app.new_issue_animation_frame,
        )
    });
    let table = Table::new(rows, widths)
        .block(frame_block("Issues", ISSUE_ACCENT))
        .style(surface_style())
        .header(header)
        .row_highlight_style(
            Style::new()
                .fg(DEFAULT_FG)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::REVERSED),
        )
        .highlight_symbol("▸")
        .highlight_spacing(HighlightSpacing::Always);
    let mut state = TableState::default().with_selected(Some(app.selected_index));
    ratatui::widgets::StatefulWidget::render(table, columns[0], buffer, &mut state);

    render_detail(app, columns[1], buffer);
}

fn issue_row(
    issue: &IssueSummary,
    highlight: Option<IssueHighlightKind>,
    animation_frame: u8,
) -> Row<'_> {
    let state = match issue.state {
        IssueState::Open => "open",
        IssueState::Closed => "closed",
    };
    let labels = if issue.labels.is_empty() {
        String::new()
    } else {
        issue
            .labels
            .iter()
            .take(2)
            .map(|label| label.name.as_str())
            .collect::<Vec<_>>()
            .join(",")
    };
    let author = issue
        .author
        .as_ref()
        .map(|author| author.login.as_str())
        .unwrap_or("unknown");
    let assignees = if issue.assignees.is_empty() {
        format!("{author}>-")
    } else {
        let assignees = issue
            .assignees
            .iter()
            .take(2)
            .map(|assignee| assignee.login.as_str())
            .collect::<Vec<_>>()
            .join(",");
        format!("{author}>{assignees}")
    };
    let updated = issue
        .updated_at
        .map(age_label)
        .unwrap_or_else(|| "-".to_string());
    let stale = is_stale(issue);

    let title = if highlight.is_some() || stale {
        let stale_badge = stale.then_some("STALE ");
        let badge = match highlight.as_ref() {
            Some(IssueHighlightKind::New) => Some("NEW "),
            Some(IssueHighlightKind::Mention) => Some("PING "),
            None => None,
        };
        let mut spans = Vec::new();
        if let Some(badge) = badge {
            spans.push(Span::styled(badge, Style::new().fg(NOTICE_ACCENT).bold()));
        }
        if let Some(stale_badge) = stale_badge {
            spans.push(Span::styled(
                stale_badge,
                Style::new().fg(WARNING_ACCENT).bold(),
            ));
        }
        spans.push(Span::styled(issue.title.clone(), surface_style()));
        Cell::from(Line::from(spans))
    } else {
        Cell::from(issue.title.clone())
    };

    let row = Row::new([
        Cell::from(format!("#{}", issue.number)).style(Style::new().fg(ACTION_ACCENT)),
        Cell::from(state).style(state_style(issue.state.clone())),
        Cell::from(assignees).style(Style::new().fg(DETAIL_ACCENT)),
        Cell::from(updated).style(if stale {
            Style::new().fg(WARNING_ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(MUTED_FG)
        }),
        Cell::from(labels).style(Style::new().fg(LABEL_ACCENT)),
        title,
    ])
    .style(surface_style());

    if let Some(kind) = highlight {
        let pulse_is_high = (animation_frame / 8).is_multiple_of(2);
        let style = if pulse_is_high {
            match kind {
                IssueHighlightKind::New => Style::new().fg(NOTICE_ACCENT),
                IssueHighlightKind::Mention => Style::new().fg(ACTION_ACCENT),
            }
            .add_modifier(Modifier::REVERSED)
            .add_modifier(Modifier::BOLD)
        } else {
            match kind {
                IssueHighlightKind::New => Style::new().fg(NOTICE_ACCENT),
                IssueHighlightKind::Mention => Style::new().fg(ACTION_ACCENT),
            }
            .add_modifier(Modifier::BOLD)
        };
        row.style(style)
    } else {
        row
    }
}

fn age_label(updated_at: chrono::DateTime<Utc>) -> String {
    let age = Utc::now().signed_duration_since(updated_at);
    if age.num_days() >= 1 {
        format!("{}d", age.num_days())
    } else if age.num_hours() >= 1 {
        format!("{}h", age.num_hours())
    } else {
        format!("{}m", age.num_minutes().max(0))
    }
}

fn is_stale(issue: &IssueSummary) -> bool {
    issue
        .updated_at
        .map(|updated_at| Utc::now().signed_duration_since(updated_at).num_days() >= 14)
        .unwrap_or(false)
}

fn state_style(state: IssueState) -> Style {
    match state {
        IssueState::Open => Style::new().fg(OPEN_ACCENT),
        IssueState::Closed => Style::new().fg(CLOSED_FG),
    }
}

fn render_detail(app: &App, area: Rect, buffer: &mut Buffer) {
    if let Some(detail) = app.selected_detail.as_ref() {
        render_detail_tree(app, detail, area, buffer);
        return;
    }

    let detail = if let Some(issue) = app.selected_issue() {
        let labels = issue
            .labels
            .iter()
            .map(|label| label.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let assignees = issue
            .assignees
            .iter()
            .map(|assignee| assignee.login.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{}\n\nauthor: {}\nassignees: {}\nlabels: {}\ncomments: {}\nupdated: {}\n\nUse :comment, :assign, :labels, :edit, or x close/reopen.",
            issue.title,
            issue
                .author
                .as_ref()
                .map_or("unknown", |author| author.login.as_str()),
            empty_label(&assignees),
            empty_label(&labels),
            issue.comment_count,
            issue
                .updated_at
                .map(age_label)
                .unwrap_or_else(|| "-".to_string())
        )
    } else {
        "No issues loaded. Use :refresh.".to_string()
    };

    Paragraph::new(detail)
        .block(frame_block("Detail", DETAIL_ACCENT))
        .style(surface_style())
        .wrap(Wrap { trim: true })
        .render(area, buffer);
}

fn render_detail_tree(app: &App, detail: &IssueDetail, area: Rect, buffer: &mut Buffer) {
    let items = detail_tree_items(detail);
    let tree = Tree::new(&items)
        .expect("detail tree item identifiers are unique")
        .block(frame_block(
            format!("Detail #{}", detail.summary.number),
            DETAIL_ACCENT,
        ))
        .style(surface_style())
        .highlight_style(Style::new().fg(DEFAULT_FG).add_modifier(Modifier::REVERSED))
        .node_open_symbol("▾ ")
        .node_closed_symbol("▸ ")
        .node_no_children_symbol("  ");
    let mut state = TreeState::<String>::default();
    state.open(vec!["description".to_string()]);
    if app.comments_expanded {
        state.open(vec!["comments".to_string()]);
        for index in 0..detail.comments.len() {
            state.open(vec!["comments".to_string(), format!("comment-{index}")]);
        }
    }

    ratatui::widgets::StatefulWidget::render(tree, area, buffer, &mut state);
}

fn detail_tree_items(detail: &IssueDetail) -> Vec<TreeItem<'_, String>> {
    vec![
        TreeItem::new(
            "description".to_string(),
            "Description",
            vec![markdown_leaf("description-body".to_string(), &detail.body)],
        )
        .expect("description item id is valid"),
        TreeItem::new(
            "comments".to_string(),
            format!("Comments ({})", detail.comments.len()),
            comment_tree_items(&detail.comments),
        )
        .expect("comments item id is valid"),
    ]
}

fn comment_tree_items(comments: &[IssueComment]) -> Vec<TreeItem<'_, String>> {
    if comments.is_empty() {
        return vec![TreeItem::new_leaf(
            "no-comments".to_string(),
            "No comments yet.",
        )];
    }

    comments
        .iter()
        .enumerate()
        .map(|(index, comment)| {
            let author = comment
                .author
                .as_ref()
                .map_or("unknown".to_string(), |author| author.login.clone());
            TreeItem::new(
                format!("comment-{index}"),
                author,
                vec![markdown_leaf(
                    format!("comment-{index}-body"),
                    &comment.body,
                )],
            )
            .expect("comment item id is valid")
        })
        .collect()
}

fn markdown_leaf(id: String, markdown: &str) -> TreeItem<'_, String> {
    if markdown.trim().is_empty() {
        TreeItem::new_leaf(id, "No description.")
    } else {
        TreeItem::new_leaf(id, tui_markdown::from_str(markdown))
    }
}

fn render_footer(app: &App, area: Rect, buffer: &mut Buffer) {
    let footer = format!("{}\n{}", footer_shortcuts(app), app.status);
    Paragraph::new(footer)
        .style(Style::new().fg(DIM_FG))
        .render(area, buffer);
}

fn footer_shortcuts(app: &App) -> &'static str {
    match app.mode {
        UiMode::Browsing => ": commands | n new | x close | j/k move | Enter details | q quit",
        UiMode::Command => "Enter run | Esc cancel | Backspace delete",
        UiMode::Search => "Enter search | Esc cancel | Backspace delete",
        UiMode::CommentComposer => "Ctrl+S submit | Enter newline | Ctrl+J newline | Esc cancel",
        UiMode::CloseComment => "Ctrl+S close | Enter newline | Ctrl+J newline | Esc cancel",
        UiMode::NewIssue => {
            "Tab fields | Ctrl+T template | Ctrl+S create | Enter edit/add label | Ctrl+J newline | Esc cancel"
        }
        UiMode::IssueEditor => {
            "Tab fields | Ctrl+S save | Enter edit/newline | Ctrl+J newline | Esc cancel"
        }
        UiMode::AssigneeFilter => "Enter apply filter | type search | j/k move | Esc cancel",
        UiMode::AssigneeEditor => {
            "Space toggle assignee | Enter save | type search | j/k move | Esc cancel"
        }
        UiMode::IssueLabelEditor => {
            "Enter toggle label | Ctrl+S save | type search | j/k move | Esc cancel"
        }
        UiMode::ConfirmClose => "y/Enter reopen | Esc cancel",
        UiMode::Success => "Any key continue",
        UiMode::Loading => "Working",
        UiMode::Error => "Esc dismiss",
        UiMode::FilterEditor => "Esc cancel",
    }
}

fn render_overlay(app: &App, area: Rect, buffer: &mut Buffer) {
    let title = match app.mode {
        UiMode::Search => Some("Search"),
        UiMode::CommentComposer => Some("Comment"),
        UiMode::CloseComment => Some("Close Issue"),
        UiMode::NewIssue => Some("New Issue"),
        UiMode::IssueEditor => Some("Edit Issue"),
        UiMode::AssigneeFilter => Some("Assignee Filter"),
        UiMode::AssigneeEditor => Some("Assign Issue"),
        UiMode::IssueLabelEditor => Some("Edit Labels"),
        UiMode::ConfirmClose => Some("Confirm"),
        UiMode::Success => Some("Done"),
        UiMode::Loading => Some("Working"),
        UiMode::Error => Some("Error"),
        _ => None,
    };

    if let Some(title) = title {
        let popup = if matches!(app.mode, UiMode::NewIssue | UiMode::IssueEditor) {
            centered_rect(72, 70, area)
        } else {
            centered_rect(72, 55, area)
        };
        Clear.render(popup, buffer);
        match app.mode {
            UiMode::NewIssue => render_new_issue_editor(app, popup, buffer),
            UiMode::IssueEditor => render_issue_editor(app, popup, buffer),
            UiMode::AssigneeFilter | UiMode::AssigneeEditor => {
                render_assignee_picker(app, title, popup, buffer)
            }
            UiMode::IssueLabelEditor => render_issue_label_editor(app, popup, buffer),
            UiMode::Loading => render_loading_overlay(app, popup, buffer),
            UiMode::ConfirmClose => Paragraph::new("Press y to reopen, Esc to cancel")
                .block(modal_block(title, WARNING_ACCENT))
                .style(modal_style())
                .wrap(Wrap { trim: false })
                .render(popup, buffer),
            UiMode::Error => Paragraph::new(app.status.clone())
                .block(modal_block(title, ERROR_ACCENT))
                .style(Style::new().fg(ERROR_ACCENT))
                .wrap(Wrap { trim: false })
                .render(popup, buffer),
            UiMode::Success => Paragraph::new(format!("{}\n\nState reloaded.", app.status))
                .block(modal_block(title, OPEN_ACCENT))
                .style(Style::new().fg(OPEN_ACCENT))
                .wrap(Wrap { trim: false })
                .render(popup, buffer),
            _ => render_text_editor(app, title, popup, buffer),
        }
    }
}

fn render_command_bar(app: &App, area: Rect, buffer: &mut Buffer) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(1)])
        .split(area);
    let mut textarea = textarea_at_end(vec![command_prompt_line(&app.input)]);
    textarea.set_block(modal_block(
        "Command  :fs filter state  :fa filter assignee  :s <text> search  :assign  :labels  :comment",
        ACTION_ACCENT,
    ));
    textarea.set_style(modal_style());
    set_visible_cursor(&mut textarea);
    (&textarea).render(rows[0], buffer);
    Paragraph::new(command_suggestions_line(&app.input))
        .style(Style::new().fg(MUTED_FG))
        .render(rows[1], buffer);
}

fn command_prompt_line(input: &str) -> String {
    let command = input.strip_prefix(':').unwrap_or(input);
    format!(":{command}")
}

fn command_suggestions_line(input: &str) -> String {
    let command = input.trim().trim_start_matches(':').trim().to_lowercase();
    let suggestions = [
        "fs",
        "fa",
        "s ",
        "me",
        "unassigned",
        "label ",
        "sort updated",
        "sort created",
        "sort comments",
        "sort assignee",
        "edit",
        "comment",
        "assign",
        "labels",
        "new",
        "close",
        "refresh",
        "quit",
    ]
    .into_iter()
    .filter(|candidate| command.is_empty() || candidate.starts_with(&command))
    .take(5)
    .collect::<Vec<_>>()
    .join("  ");

    if suggestions.is_empty() {
        "no matching commands".to_string()
    } else {
        format!("Tab completes: {suggestions}")
    }
}

fn render_loading_overlay(app: &App, area: Rect, buffer: &mut Buffer) {
    let title = app
        .pending_action
        .as_ref()
        .map(pending_action_label)
        .unwrap_or("Working");
    let body = format!("[*] {title}\n\n{}", app.status);

    Paragraph::new(body)
        .block(modal_block("Working", ACTION_ACCENT))
        .style(Style::new().fg(ACTION_ACCENT).add_modifier(Modifier::BOLD))
        .wrap(Wrap { trim: false })
        .render(area, buffer);
}

fn render_assignee_picker(app: &App, title: &'static str, area: Rect, buffer: &mut Buffer) {
    let rows = picker_rows_from_popup(area);
    let choices = if app.mode == UiMode::AssigneeFilter {
        app.assignee_filter_choices()
    } else {
        app.assignee_assignment_choices()
    };
    let mut lines = vec![
        Line::from(format!("search: {}", app.input)),
        Line::from(format!(
            "{}: {}",
            if app.mode == UiMode::AssigneeFilter {
                "current filter"
            } else {
                "selected"
            },
            if app.mode == UiMode::AssigneeFilter {
                app.filters.assignee.label()
            } else {
                empty_label(&app.editing_assignees.join(", ")).to_string()
            },
        )),
        Line::raw(""),
    ];
    if choices.is_empty() {
        lines.push(Line::from("No matching collaborators."));
    } else {
        lines.extend(choices.iter().enumerate().map(|(index, choice)| {
            let marker = if index == app.picker_index { ">" } else { " " };
            let active = assignee_choice_is_active(app, choice);
            let check = if active { "*" } else { " " };
            Line::from(format!("{marker} [{check}] {}", choice.label()))
        }));
    }

    Paragraph::new(lines)
        .block(modal_block(title, PICKER_ACCENT))
        .style(modal_style())
        .wrap(Wrap { trim: false })
        .render(rows[0], buffer);
    render_action_buttons(assignee_primary_label(app), rows[1], buffer);
}

fn render_issue_label_editor(app: &App, area: Rect, buffer: &mut Buffer) {
    let rows = picker_rows_from_popup(area);
    let choices = app.issue_label_choices();
    let selected = if app.editing_issue_labels.is_empty() {
        "none".to_string()
    } else {
        app.editing_issue_labels.join(", ")
    };
    let mut lines = vec![
        Line::from(format!("search: {}", app.input)),
        Line::from(format!("selected: {selected}")),
        Line::raw(""),
    ];
    if choices.is_empty() {
        lines.push(Line::from("No matching labels."));
    } else {
        lines.extend(choices.iter().enumerate().map(|(index, label)| {
            let marker = if index == app.picker_index { ">" } else { " " };
            let check = if app.editing_issue_labels.iter().any(|item| item == label) {
                "x"
            } else {
                " "
            };
            Line::from(format!("{marker} [{check}] {label}"))
        }));
    }

    Paragraph::new(lines)
        .block(modal_block("Edit Labels", LABEL_ACCENT))
        .style(Style::new().fg(LABEL_ACCENT))
        .wrap(Wrap { trim: false })
        .render(rows[0], buffer);
    render_action_buttons(ISSUE_LABEL_PRIMARY_LABEL, rows[1], buffer);
}

fn assignee_choice_is_active(app: &App, choice: &AssigneeChoice) -> bool {
    match (app.mode.clone(), choice) {
        (UiMode::AssigneeFilter, AssigneeChoice::Any) => {
            app.filters.assignee == AssigneeFilter::Any
        }
        (UiMode::AssigneeFilter, AssigneeChoice::Me) => app.filters.assignee == AssigneeFilter::Me,
        (UiMode::AssigneeFilter, AssigneeChoice::Unassigned) => {
            app.filters.assignee == AssigneeFilter::None
        }
        (UiMode::AssigneeFilter, AssigneeChoice::User(login)) => {
            app.filters.assignee == AssigneeFilter::User(login.clone())
        }
        (UiMode::AssigneeEditor, AssigneeChoice::Unassigned) => app.editing_assignees.is_empty(),
        (UiMode::AssigneeEditor, AssigneeChoice::Me) => {
            let Some(login) = app.viewer_login.as_ref() else {
                return false;
            };
            app.editing_assignees
                .iter()
                .any(|assignee| assignee == login)
        }
        (UiMode::AssigneeEditor, AssigneeChoice::User(login)) => app
            .editing_assignees
            .iter()
            .any(|assignee| assignee == login),
        _ => false,
    }
}

fn empty_label(value: &str) -> &str {
    if value.is_empty() { "none" } else { value }
}

fn pending_action_label(action: &PendingAction) -> &'static str {
    match action {
        PendingAction::Refresh => "Refreshing issues",
        PendingAction::LoadLabels => "Loading labels",
        PendingAction::LoadCollaborators => "Loading collaborators",
        PendingAction::LoadTemplates => "Loading templates",
        PendingAction::CreateIssue => "Creating issue",
        PendingAction::AddComment => "Adding comment",
        PendingAction::CloseIssue => "Closing issue",
        PendingAction::ReopenIssue => "Reopening issue",
        PendingAction::UpdateIssue => "Updating issue",
        PendingAction::UpdateAssignees => "Updating assignees",
        PendingAction::UpdateLabels => "Updating labels",
    }
}

fn render_text_editor(app: &App, title: &'static str, area: Rect, buffer: &mut Buffer) {
    let rows = if matches!(app.mode, UiMode::CommentComposer | UiMode::CloseComment) {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(3)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(100)])
            .split(area)
    };

    let mut textarea = textarea_at_end(input_lines(&app.input));
    textarea.set_block(modal_block(
        match app.mode {
            UiMode::CommentComposer => "Comment Body",
            UiMode::CloseComment => "Closing Comment",
            UiMode::NewIssue => "New Issue: title | body",
            UiMode::Search => "Search Issues",
            UiMode::Command => "Command",
            _ => title,
        },
        ACTION_ACCENT,
    ));
    textarea.set_placeholder_text(match app.mode {
        UiMode::CommentComposer => "Write a comment",
        UiMode::CloseComment => "Required comment before closing",
        UiMode::NewIssue => "Title | optional body",
        UiMode::Search => "Search issue titles",
        UiMode::Command => ":fs, :fa, :s <text>, :assign, :labels, :new, :quit",
        _ => "",
    });
    textarea.set_style(modal_style());
    set_visible_cursor(&mut textarea);
    (&textarea).render(rows[0], buffer);

    if matches!(app.mode, UiMode::CommentComposer | UiMode::CloseComment) {
        render_action_buttons(COMMENT_PRIMARY_LABEL, rows[1], buffer);
    }
}

fn render_new_issue_editor(app: &App, area: Rect, buffer: &mut Buffer) {
    let block = modal_block("New Issue", ISSUE_ACCENT);
    let inner = block.inner(area);
    block.render(area, buffer);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(7),
            Constraint::Length(3),
        ])
        .split(inner.inner(Margin {
            horizontal: 1,
            vertical: 0,
        }));

    let title_active = app.new_issue_field == NewIssueField::Title;
    let mut title = textarea_at_end(input_lines(&app.input));
    title.set_block(modal_block("Title", TITLE_ACCENT));
    title.set_style(field_style(
        &app.new_issue_field,
        &NewIssueField::Title,
        DEFAULT_FG,
    ));
    if title_active {
        set_visible_cursor(&mut title);
    } else {
        hide_cursor(&mut title);
    }
    (&title).render(rows[0], buffer);

    let body_active = app.new_issue_field == NewIssueField::Body;
    let mut body = textarea_at_end(input_lines(&app.body_input));
    body.set_block(modal_block("Body (Markdown)", ACTION_ACCENT));
    body.set_placeholder_text("Write the issue body");
    body.set_style(field_style(
        &app.new_issue_field,
        &NewIssueField::Body,
        DEFAULT_FG,
    ));
    if body_active {
        set_visible_cursor(&mut body);
    } else {
        hide_cursor(&mut body);
    }
    (&body).render(rows[1], buffer);

    let selected = if app.new_issue_labels.is_empty() {
        "none".to_string()
    } else {
        app.new_issue_labels.join(", ")
    };
    let suggestions = app.label_suggestions();
    let suggestions = if suggestions.is_empty() {
        "none".to_string()
    } else {
        suggestions.join(", ")
    };
    let input_line = if app.new_issue_field == NewIssueField::Labels {
        Line::from(vec![
            Span::raw("input: "),
            Span::raw(app.label_input.clone()),
            Span::styled(
                " ",
                Style::new()
                    .fg(ACTION_ACCENT)
                    .add_modifier(Modifier::REVERSED),
            ),
        ])
    } else {
        Line::from(format!("input: {}", app.label_input))
    };
    Paragraph::new(vec![
        Line::from(format!("selected: {selected}")),
        input_line,
        Line::from(format!("suggestions: {suggestions}")),
        Line::from(format!("templates: {}", template_summary(app))),
    ])
    .block(modal_block("Labels", LABEL_ACCENT))
    .style(field_style(
        &app.new_issue_field,
        &NewIssueField::Labels,
        LABEL_ACCENT,
    ))
    .wrap(Wrap { trim: false })
    .render(rows[2], buffer);

    render_action_buttons(NEW_ISSUE_PRIMARY_LABEL, rows[3], buffer);
}

fn render_issue_editor(app: &App, area: Rect, buffer: &mut Buffer) {
    let block = modal_block("Edit Issue", ISSUE_ACCENT);
    let inner = block.inner(area);
    block.render(area, buffer);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(inner.inner(Margin {
            horizontal: 1,
            vertical: 0,
        }));

    let mut title = textarea_at_end(input_lines(&app.input));
    title.set_block(modal_block("Title", TITLE_ACCENT));
    title.set_style(modal_style());
    if app.issue_edit_field == IssueEditField::Title {
        set_visible_cursor(&mut title);
    } else {
        hide_cursor(&mut title);
    }
    (&title).render(rows[0], buffer);

    let mut body = textarea_at_end(input_lines(&app.body_input));
    body.set_block(modal_block("Body (Markdown)", ACTION_ACCENT));
    body.set_style(modal_style());
    if app.issue_edit_field == IssueEditField::Body {
        set_visible_cursor(&mut body);
    } else {
        hide_cursor(&mut body);
    }
    (&body).render(rows[1], buffer);

    render_action_buttons(ISSUE_EDIT_PRIMARY_LABEL, rows[2], buffer);
}

fn template_summary(app: &App) -> String {
    if app.repo_issue_templates.is_empty() {
        return "none".to_string();
    }

    app.repo_issue_templates
        .iter()
        .take(3)
        .map(|template| template.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_action_buttons(primary: &'static str, area: Rect, buffer: &mut Buffer) {
    let buttons = Line::from(vec![
        Span::styled(
            primary_button_text(primary),
            Style::new()
                .fg(ACTION_ACCENT)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::REVERSED),
        ),
        Span::raw("  "),
        Span::styled(
            " Cancel Esc ",
            Style::new()
                .fg(MUTED_FG)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::REVERSED),
        ),
    ]);

    Paragraph::new(buttons)
        .block(modal_block("Actions", ACTION_FRAME_ACCENT))
        .style(modal_style())
        .render(area, buffer);
}

fn assignee_primary_label(app: &App) -> &'static str {
    if app.mode == UiMode::AssigneeFilter {
        ASSIGNEE_FILTER_PRIMARY_LABEL
    } else {
        ASSIGNEE_EDITOR_PRIMARY_LABEL
    }
}

fn field_style(active: &NewIssueField, field: &NewIssueField, color: Color) -> Style {
    if active == field {
        Style::new()
            .fg(color)
            .add_modifier(Modifier::BOLD)
            .add_modifier(Modifier::REVERSED)
    } else {
        Style::new().fg(color)
    }
}

fn input_lines(input: &str) -> Vec<String> {
    if input.is_empty() {
        vec![String::new()]
    } else {
        input.lines().map(ToOwned::to_owned).collect()
    }
}

fn textarea_at_end(lines: Vec<String>) -> TextArea<'static> {
    let mut textarea = TextArea::new(lines);
    textarea.move_cursor(CursorMove::Bottom);
    textarea.move_cursor(CursorMove::End);
    textarea
}

fn set_visible_cursor(textarea: &mut TextArea<'_>) {
    textarea.set_cursor_line_style(Style::new().add_modifier(Modifier::REVERSED));
    textarea.set_cursor_style(
        Style::new()
            .fg(ACTION_ACCENT)
            .add_modifier(Modifier::REVERSED),
    );
}

fn hide_cursor(textarea: &mut TextArea<'_>) {
    textarea.set_cursor_line_style(Style::new());
    textarea.set_cursor_style(Style::new());
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1]);
    horizontal[1]
}

pub fn mouse_target(app: &App, area: Rect, column: u16, row: u16) -> Option<MouseTarget> {
    let point = Rect::new(column, row, 1, 1);

    match app.mode {
        UiMode::Browsing => browsing_mouse_target(app, area, point),
        UiMode::CommentComposer | UiMode::CloseComment => action_mouse_target(
            comment_editor_action_area(area),
            COMMENT_PRIMARY_LABEL,
            point,
        ),
        UiMode::NewIssue => new_issue_mouse_target(area, point),
        UiMode::IssueEditor => issue_editor_mouse_target(area, point),
        UiMode::AssigneeFilter | UiMode::AssigneeEditor => {
            let item_count = if app.mode == UiMode::AssigneeFilter {
                app.assignee_filter_choices().len()
            } else {
                app.assignee_assignment_choices().len()
            };
            picker_mouse_target(area, point, item_count, assignee_primary_label(app))
        }
        UiMode::IssueLabelEditor => picker_mouse_target(
            area,
            point,
            app.issue_label_choices().len(),
            ISSUE_LABEL_PRIMARY_LABEL,
        ),
        UiMode::Success => Some(MouseTarget::CancelAction),
        _ => None,
    }
}

fn browsing_mouse_target(app: &App, area: Rect, point: Rect) -> Option<MouseTarget> {
    let columns = browsing_columns(area);
    if intersects(point, columns[0]) {
        let first_issue_row = columns[0].y.saturating_add(3);
        if point.y >= first_issue_row {
            let index = usize::from(point.y - first_issue_row);
            if index < app.issues.len() {
                return Some(MouseTarget::IssueRow(index));
            }
        }
    }

    if intersects(point, columns[1]) && app.selected_detail.is_some() {
        return Some(MouseTarget::DetailPanel);
    }

    None
}

fn new_issue_mouse_target(area: Rect, point: Rect) -> Option<MouseTarget> {
    let rows = new_issue_rows(area);
    if intersects(point, rows[0]) {
        return Some(MouseTarget::NewIssueField(NewIssueField::Title));
    }
    if intersects(point, rows[1]) {
        return Some(MouseTarget::NewIssueField(NewIssueField::Body));
    }
    if intersects(point, rows[2]) {
        return Some(MouseTarget::NewIssueField(NewIssueField::Labels));
    }

    action_mouse_target(rows[3], NEW_ISSUE_PRIMARY_LABEL, point)
}

fn issue_editor_mouse_target(area: Rect, point: Rect) -> Option<MouseTarget> {
    let rows = issue_editor_rows(area);
    if intersects(point, rows[0]) {
        return Some(MouseTarget::IssueEditField(IssueEditField::Title));
    }
    if intersects(point, rows[1]) {
        return Some(MouseTarget::IssueEditField(IssueEditField::Body));
    }

    action_mouse_target(rows[2], ISSUE_EDIT_PRIMARY_LABEL, point)
}

fn picker_mouse_target(
    area: Rect,
    point: Rect,
    item_count: usize,
    primary: &'static str,
) -> Option<MouseTarget> {
    let popup = centered_rect(72, 55, area);
    let rows = picker_rows_from_popup(popup);
    if let Some(target) = action_mouse_target(rows[1], primary, point) {
        return Some(target);
    }

    let list_area = rows[0];
    if !intersects(point, list_area) {
        return None;
    }

    let first_item_row = list_area.y.saturating_add(4);
    let last_visible_row = list_area
        .y
        .saturating_add(list_area.height.saturating_sub(1));
    if point.y < first_item_row || point.y >= last_visible_row {
        return None;
    }

    let index = usize::from(point.y - first_item_row);
    if index < item_count {
        Some(MouseTarget::PickerItem(index))
    } else {
        None
    }
}

fn main_rows(area: Rect) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(area)
}

fn command_mode_rows(area: Rect) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(4),
            Constraint::Length(2),
        ])
        .split(area)
}

fn command_bar_area(area: Rect) -> Rect {
    command_mode_rows(area)[3]
}

fn browsing_columns(area: Rect) -> std::rc::Rc<[Rect]> {
    body_columns(main_rows(area)[2])
}

fn issue_list_area(area: Rect) -> Rect {
    browsing_columns(area)[0]
}

fn body_columns(area: Rect) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(ISSUE_LIST_PERCENT),
            Constraint::Percentage(DETAIL_PANEL_PERCENT),
        ])
        .split(area)
}

fn new_issue_rows(area: Rect) -> std::rc::Rc<[Rect]> {
    let popup = centered_rect(72, 70, area);
    let inner = popup.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    let inner = inner.inner(Margin {
        horizontal: 1,
        vertical: 0,
    });

    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(7),
            Constraint::Length(3),
        ])
        .split(inner)
}

fn issue_editor_rows(area: Rect) -> std::rc::Rc<[Rect]> {
    let popup = centered_rect(72, 70, area);
    let inner = popup.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    let inner = inner.inner(Margin {
        horizontal: 1,
        vertical: 0,
    });

    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(inner)
}

fn picker_rows_from_popup(area: Rect) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(3)])
        .split(area)
}

fn comment_editor_action_area(area: Rect) -> Rect {
    let popup = centered_rect(72, 55, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(popup);
    rows[1]
}

fn action_mouse_target(area: Rect, primary: &'static str, point: Rect) -> Option<MouseTarget> {
    if !intersects(point, area) {
        return None;
    }

    let line_y = area.y.saturating_add(1);
    if point.y != line_y {
        return None;
    }

    let primary_start = area.x.saturating_add(1);
    let primary_end = primary_start.saturating_add(primary_button_width(primary));
    let cancel_start = primary_end.saturating_add(2);
    let cancel_end = cancel_start.saturating_add(" Cancel Esc ".len() as u16);

    if point.x >= primary_start && point.x < primary_end {
        Some(MouseTarget::PrimaryAction)
    } else if point.x >= cancel_start && point.x < cancel_end {
        Some(MouseTarget::CancelAction)
    } else {
        None
    }
}

fn primary_button_width(primary: &'static str) -> u16 {
    primary_button_text(primary).len() as u16
}

fn primary_button_text(primary: &'static str) -> String {
    let shortcut = match primary {
        ASSIGNEE_FILTER_PRIMARY_LABEL | ASSIGNEE_EDITOR_PRIMARY_LABEL => "Enter",
        _ => "Ctrl+S",
    };
    format!(" {primary} {shortcut} ")
}

fn intersects(point: Rect, area: Rect) -> bool {
    point.x >= area.x
        && point.x < area.x.saturating_add(area.width)
        && point.y >= area.y
        && point.y < area.y.saturating_add(area.height)
}

pub fn buffer_to_string(buffer: &Buffer) -> String {
    buffer
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<Vec<_>>()
        .join("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::App,
        domain::{IssueState, IssueSummary, IssueTemplate, Label, User},
    };
    use chrono::Duration;
    use ratatui::{buffer::Buffer, layout::Rect};

    fn issue(number: u64, title: &str, state: IssueState, labels: &[&str]) -> IssueSummary {
        IssueSummary {
            number,
            title: title.to_string(),
            state,
            labels: labels
                .iter()
                .map(|name| Label {
                    name: name.to_string(),
                })
                .collect(),
            assignees: Vec::new(),
            author: None,
            created_at: None,
            updated_at: None,
            comment_count: 0,
        }
    }

    #[test]
    fn renders_main_screen_with_filters_and_actions() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_issues(vec![
            issue(122, "Fix login redraw", IssueState::Open, &["bug"]),
            issue(101, "Clarify setup", IssueState::Closed, &["docs"]),
        ]);
        app.set_query("redraw");

        let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 24));
        render(&app, buffer.area, &mut buffer);
        let rendered = buffer_to_string(&buffer);

        assert!(rendered.contains("owner/skunkwork"));
        assert!(rendered.contains("State: open"));
        assert!(rendered.contains("Assignee: any"));
        assert!(rendered.contains("Search: redraw"));
        assert!(rendered.contains("Number"));
        assert!(rendered.contains("State"));
        assert!(rendered.contains("Team"));
        assert!(rendered.contains("Age"));
        assert!(rendered.contains("Sort: updated"));
        assert!(rendered.contains("Tags"));
        assert!(rendered.contains("Title"));
        assert!(rendered.contains("#122"));
        assert!(rendered.contains("open"));
        assert!(rendered.contains("Fix login redraw"));
        assert!(rendered.contains(": commands"));
        assert!(rendered.contains("n new"));
        assert!(rendered.contains("x close"));
        assert!(!rendered.contains("A assign"));
        assert!(!rendered.contains("c comment"));
        assert!(!rendered.contains("l labels"));
    }

    #[test]
    fn main_screen_uses_terminal_default_background() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_issues(vec![
            issue(122, "Fix login redraw", IssueState::Open, &["bug"]),
            issue(101, "Clarify setup", IssueState::Closed, &["docs"]),
        ]);

        let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 24));
        render(&app, buffer.area, &mut buffer);

        assert!(buffer.content.iter().all(|cell| cell.bg == Color::Reset));
    }

    #[test]
    fn modal_screens_use_terminal_default_background() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.start_new_issue();
        app.input = "Add terminal theme support".to_string();
        app.body_input = "Keep the user's terminal palette visible.".to_string();

        let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 32));
        render(&app, buffer.area, &mut buffer);

        assert!(buffer.content.iter().all(|cell| cell.bg == Color::Reset));
    }

    #[test]
    fn footer_shortcuts_follow_active_screen() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());

        assert!(footer_shortcuts(&app).contains(": commands"));
        assert!(footer_shortcuts(&app).contains("n new"));
        assert!(footer_shortcuts(&app).contains("x close"));
        assert!(!footer_shortcuts(&app).contains("A assign"));

        app.mode = UiMode::NewIssue;
        assert!(footer_shortcuts(&app).contains("Ctrl+S create"));
        assert!(!footer_shortcuts(&app).contains("q quit"));

        app.mode = UiMode::CommentComposer;
        assert!(footer_shortcuts(&app).contains("Ctrl+S submit"));
        assert!(!footer_shortcuts(&app).contains("n new"));

        app.mode = UiMode::CloseComment;
        assert!(footer_shortcuts(&app).contains("Ctrl+S close"));

        app.mode = UiMode::Search;
        assert!(footer_shortcuts(&app).contains("Enter search"));

        app.mode = UiMode::Command;
        assert!(footer_shortcuts(&app).contains("Enter run"));
        assert!(!footer_shortcuts(&app).contains("n new"));
    }

    #[test]
    fn renders_command_prompt_above_footer() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.mode = UiMode::Command;
        app.input = "sor".to_string();

        let area = Rect::new(0, 0, 120, 28);
        let mut buffer = Buffer::empty(area);
        render(&app, buffer.area, &mut buffer);
        let rendered = buffer_to_string(&buffer);
        let command_area = command_bar_area(area);

        assert!(rendered.contains("Command"));
        assert!(rendered.contains(":sor"));
        assert!(rendered.contains("sort updated"));
        assert!(rendered.contains("Enter run"));
        assert_eq!(command_area.height, 4);
        assert!(command_area.y > area.height / 2);
    }

    #[test]
    fn renders_issue_editor_with_title_and_body_fields() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.mode = UiMode::IssueEditor;
        app.input = "Fix redraw".to_string();
        app.body_input = "## Body".to_string();

        let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 32));
        render(&app, buffer.area, &mut buffer);
        let rendered = buffer_to_string(&buffer);

        assert!(rendered.contains("Edit Issue"));
        assert!(rendered.contains("Fix redraw"));
        assert!(rendered.contains("## Body"));
        assert!(rendered.contains("Save Ctrl+S"));
    }

    #[test]
    fn command_effect_area_uses_bottom_prompt() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.mode = UiMode::Command;

        let area = Rect::new(0, 0, 120, 40);
        let target = effect_area(&app, area);

        assert_eq!(target, command_bar_area(area));
        assert_eq!(target.height, 4);
        assert!(target.y > area.height / 2);
    }

    #[test]
    fn renders_new_issue_highlight_in_issue_list() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_issues(vec![
            issue(122, "Fix login redraw", IssueState::Open, &["bug"]),
            issue(130, "Fresh", IssueState::Open, &[]),
        ]);
        app.highlight_new_issues(vec![130]);

        let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 24));
        render(&app, buffer.area, &mut buffer);
        let rendered = buffer_to_string(&buffer);

        assert!(rendered.contains("#130"));
        assert!(rendered.contains("NEW"));
        assert!(rendered.contains("Fresh"));
    }

    #[test]
    fn renders_mention_highlight_in_issue_list() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_issues(vec![
            issue(122, "Fix login redraw", IssueState::Open, &["bug"]),
            issue(130, "Ping", IssueState::Open, &[]),
        ]);
        app.highlight_mentioned_issues(vec![130]);

        let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 24));
        render(&app, buffer.area, &mut buffer);
        let rendered = buffer_to_string(&buffer);

        assert!(rendered.contains("#130"));
        assert!(rendered.contains("PING"));
        assert!(rendered.contains("Ping"));
    }

    #[test]
    fn renders_team_metadata_and_stale_badge() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        let mut stale = issue(130, "Needs owner", IssueState::Open, &[]);
        stale.author = Some(User {
            login: "alice".to_string(),
        });
        stale.assignees = vec![User {
            login: "bob".to_string(),
        }];
        stale.updated_at = Some(Utc::now() - Duration::days(30));
        app.set_issues(vec![stale]);

        let mut buffer = Buffer::empty(Rect::new(0, 0, 140, 24));
        render(&app, buffer.area, &mut buffer);
        let rendered = buffer_to_string(&buffer);

        assert!(rendered.contains("alice>bob"));
        assert!(rendered.contains("STALE"));
        assert!(rendered.contains("30d"));
    }

    #[test]
    fn mouse_targets_issue_rows_and_detail_panel() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_issues(vec![
            issue(1, "Fix redraw", IssueState::Open, &[]),
            issue(2, "Add mouse", IssueState::Open, &[]),
        ]);
        app.selected_detail = Some(crate::domain::IssueDetail {
            summary: issue(1, "Fix redraw", IssueState::Open, &[]),
            body: String::new(),
            comments: Vec::new(),
        });

        let area = Rect::new(0, 0, 100, 30);

        assert_eq!(
            mouse_target(&app, area, 1, 7),
            Some(MouseTarget::IssueRow(0))
        );
        assert_eq!(
            mouse_target(&app, area, 1, 8),
            Some(MouseTarget::IssueRow(1))
        );
        assert_eq!(
            mouse_target(&app, area, 45, 7),
            Some(MouseTarget::DetailPanel)
        );
    }

    #[test]
    fn mouse_targets_new_issue_fields_and_buttons() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.mode = UiMode::NewIssue;
        let area = Rect::new(0, 0, 100, 30);
        let rows = new_issue_rows(area);

        assert_eq!(
            mouse_target(&app, area, rows[0].x + 1, rows[0].y + 1),
            Some(MouseTarget::NewIssueField(NewIssueField::Title))
        );
        assert_eq!(
            mouse_target(&app, area, rows[1].x + 1, rows[1].y + 1),
            Some(MouseTarget::NewIssueField(NewIssueField::Body))
        );
        assert_eq!(
            mouse_target(&app, area, rows[2].x + 1, rows[2].y + 1),
            Some(MouseTarget::NewIssueField(NewIssueField::Labels))
        );
        assert_eq!(
            mouse_target(&app, area, rows[3].x + 2, rows[3].y + 1),
            Some(MouseTarget::PrimaryAction)
        );
        assert_eq!(
            mouse_target(&app, area, rows[3].x + 22, rows[3].y + 1),
            Some(MouseTarget::CancelAction)
        );
    }

    #[test]
    fn mouse_targets_issue_editor_fields_and_buttons() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.mode = UiMode::IssueEditor;
        let area = Rect::new(0, 0, 100, 30);
        let rows = issue_editor_rows(area);

        assert_eq!(
            mouse_target(&app, area, rows[0].x + 1, rows[0].y + 1),
            Some(MouseTarget::IssueEditField(IssueEditField::Title))
        );
        assert_eq!(
            mouse_target(&app, area, rows[1].x + 1, rows[1].y + 1),
            Some(MouseTarget::IssueEditField(IssueEditField::Body))
        );
        assert_eq!(
            mouse_target(&app, area, rows[2].x + 2, rows[2].y + 1),
            Some(MouseTarget::PrimaryAction)
        );
    }

    #[test]
    fn mouse_targets_picker_rows_and_buttons() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.mode = UiMode::AssigneeEditor;
        app.set_repo_collaborators(vec![crate::domain::User {
            login: "alice".to_string(),
        }]);
        let area = Rect::new(0, 0, 100, 30);
        let popup = centered_rect(72, 55, area);
        let rows = picker_rows_from_popup(popup);

        assert_eq!(
            mouse_target(&app, area, rows[0].x + 2, rows[0].y + 4),
            Some(MouseTarget::PickerItem(0))
        );
        assert_eq!(
            mouse_target(&app, area, rows[1].x + 2, rows[1].y + 1),
            Some(MouseTarget::PrimaryAction)
        );
        assert_eq!(
            mouse_target(
                &app,
                area,
                rows[1].x + primary_button_width(ASSIGNEE_EDITOR_PRIMARY_LABEL) + 4,
                rows[1].y + 1
            ),
            Some(MouseTarget::CancelAction)
        );
    }

    #[test]
    fn detail_panel_is_wider_than_issue_list() {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(ISSUE_LIST_PERCENT),
                Constraint::Percentage(DETAIL_PANEL_PERCENT),
            ])
            .split(Rect::new(0, 0, 100, 20));

        assert!(columns[0].width < columns[1].width);
        assert_eq!(columns[0].width + columns[1].width, 100);
    }

    #[test]
    fn renders_assignee_picker_with_collaborators() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.mode = UiMode::AssigneeEditor;
        app.set_repo_collaborators(vec![crate::domain::User {
            login: "alice".to_string(),
        }]);

        let mut buffer = Buffer::empty(Rect::new(0, 0, 96, 24));
        render(&app, buffer.area, &mut buffer);
        let rendered = buffer_to_string(&buffer);

        assert!(rendered.contains("Assign Issue"));
        assert!(rendered.contains("unassigned"));
        assert!(rendered.contains("alice"));
        assert!(rendered.contains("Assign Enter"));
    }

    #[test]
    fn renders_issue_label_editor_with_selected_labels() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.mode = UiMode::IssueLabelEditor;
        app.set_repo_labels(vec![
            Label {
                name: "bug".to_string(),
            },
            Label {
                name: "docs".to_string(),
            },
        ]);
        app.set_repo_issue_templates(vec![IssueTemplate {
            name: "bug report".to_string(),
            body: "template".to_string(),
        }]);
        app.editing_issue_labels = vec!["bug".to_string()];

        let mut buffer = Buffer::empty(Rect::new(0, 0, 96, 24));
        render(&app, buffer.area, &mut buffer);
        let rendered = buffer_to_string(&buffer);

        assert!(rendered.contains("Edit Labels"));
        assert!(rendered.contains("[x] bug"));
        assert!(rendered.contains("[ ] docs"));
        assert!(rendered.contains("Save Ctrl+S"));
    }

    #[test]
    fn routine_refresh_starts_contained_loading_effects() {
        let mut effects = WorktrackEffects::default();

        effects.trigger_refresh();

        assert!(effects.has_effects());
    }

    #[test]
    fn creates_startup_loading_effects() {
        let mut effects = WorktrackEffects::default();

        effects.trigger_startup_loading();

        assert!(effects.has_effects());
    }

    #[test]
    fn loading_effects_are_targeted_to_modal_area() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.begin_action(PendingAction::Refresh, "Refreshing issues");

        let area = Rect::new(0, 0, 120, 40);
        let target = effect_area(&app, area);

        assert!(target.width < area.width);
        assert!(target.height < area.height);
    }

    #[test]
    fn browsing_refresh_effect_targets_issue_list() {
        let app = App::new("owner/skunkwork".parse().unwrap());
        let area = Rect::new(0, 0, 120, 40);

        let target = effect_area(&app, area);

        assert_eq!(target, issue_list_area(area));
        assert!(target.width < area.width);
        assert!(target.height < area.height);
    }

    #[test]
    fn renders_loading_overlay_for_pending_actions() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.begin_action(PendingAction::CreateIssue, "Creating issue");

        let mut buffer = Buffer::empty(Rect::new(0, 0, 96, 24));
        render(&app, buffer.area, &mut buffer);
        let rendered = buffer_to_string(&buffer);

        assert!(rendered.contains("Working"));
        assert!(rendered.contains("Creating issue"));
        assert!(rendered.contains("[*]"));
    }

    #[test]
    fn renders_comment_composer_as_text_editor() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.mode = UiMode::CommentComposer;
        app.input = "Looks good".to_string();

        let mut buffer = Buffer::empty(Rect::new(0, 0, 96, 24));
        render(&app, buffer.area, &mut buffer);
        let rendered = buffer_to_string(&buffer);

        assert!(rendered.contains("Comment Body"));
        assert!(rendered.contains("Looks good"));
        assert!(rendered.contains("Submit Ctrl+S"));
        assert!(rendered.contains("Cancel Esc"));
    }

    #[test]
    fn renders_close_comment_as_required_text_editor() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.mode = UiMode::CloseComment;
        app.input = "Closing after verification".to_string();

        let mut buffer = Buffer::empty(Rect::new(0, 0, 96, 24));
        render(&app, buffer.area, &mut buffer);
        let rendered = buffer_to_string(&buffer);

        assert!(rendered.contains("Closing Comment"));
        assert!(rendered.contains("Closing after verification"));
        assert!(rendered.contains("Submit Ctrl+S"));
        assert!(rendered.contains("Cancel Esc"));
    }

    #[test]
    fn renders_new_issue_as_separate_title_body_and_label_fields() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.start_new_issue();
        app.input = "Add label picker".to_string();
        app.body_input = "## Details\n\nUse markdown".to_string();
        app.new_issue_labels = vec!["bug".to_string()];
        app.label_input = "do".to_string();
        app.set_repo_labels(vec![
            Label {
                name: "bug".to_string(),
            },
            Label {
                name: "docs".to_string(),
            },
        ]);
        app.set_repo_issue_templates(vec![IssueTemplate {
            name: "bug report".to_string(),
            body: "template".to_string(),
        }]);

        let mut buffer = Buffer::empty(Rect::new(0, 0, 110, 32));
        render(&app, buffer.area, &mut buffer);
        let rendered = buffer_to_string(&buffer);

        assert!(rendered.contains("New Issue"));
        assert!(rendered.contains("Title"));
        assert!(rendered.contains("Add label picker"));
        assert!(rendered.contains("Body (Markdown)"));
        assert!(rendered.contains("Use markdown"));
        assert!(rendered.contains("Labels"));
        assert!(rendered.contains("selected: bug"));
        assert!(rendered.contains("input: do"));
        assert!(rendered.contains("suggestions: docs"));
        assert_eq!(template_summary(&app), "bug report");
        assert!(rendered.contains("Create Ctrl+S"));
        assert!(rendered.contains("Cancel Esc"));
    }

    #[test]
    fn textareas_render_cursor_at_the_end_of_current_input() {
        let textarea = textarea_at_end(input_lines("first\nsecond"));

        assert_eq!(textarea.cursor(), (1, 6));
    }

    #[test]
    fn renders_success_confirmation_over_reloaded_state() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_issues(vec![issue(122, "Fix login redraw", IssueState::Open, &[])]);
        app.mode = UiMode::Success;
        app.set_status("Commented on issue #122");

        let mut buffer = Buffer::empty(Rect::new(0, 0, 110, 32));
        render(&app, buffer.area, &mut buffer);
        let rendered = buffer_to_string(&buffer);

        assert!(rendered.contains("Fix login redraw"));
        assert!(rendered.contains("Done"));
        assert!(rendered.contains("Commented on issue #122"));
        assert!(rendered.contains("State reloaded."));
    }

    #[test]
    fn renders_issue_detail_as_markdown_tree_with_comments() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        let summary = issue(122, "Fix login redraw", IssueState::Open, &["bug"]);
        app.set_issues(vec![summary.clone()]);
        app.set_selected_detail(crate::domain::IssueDetail {
            summary,
            body: "## Description\n\n- redraw the form".to_string(),
            comments: vec![crate::domain::IssueComment {
                author: Some(crate::domain::User {
                    login: "octocat".to_string(),
                }),
                body: "**Looks good**\n\nShip it.".to_string(),
                created_at: None,
            }],
        });

        let mut buffer = Buffer::empty(Rect::new(0, 0, 110, 28));
        render(&app, buffer.area, &mut buffer);
        let rendered = buffer_to_string(&buffer);

        assert!(rendered.contains("Description"));
        assert!(rendered.contains("Comments (1)"));
        assert!(rendered.contains("redraw the form"));
        assert!(rendered.contains("octocat"));
        assert!(rendered.contains("Looks good"));
        assert!(rendered.contains("Ship it."));
    }

    #[test]
    fn renders_multiline_markdown_issue_description() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        let summary = issue(122, "Fix multiline body", IssueState::Open, &["bug"]);
        app.set_issues(vec![summary.clone()]);
        app.set_selected_detail(crate::domain::IssueDetail {
            summary,
            body: "Line one\n\nLine two\n\n### Notes\n\n- first item\n- second item".to_string(),
            comments: Vec::new(),
        });

        let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 34));
        render(&app, buffer.area, &mut buffer);
        let rendered = buffer_to_string(&buffer);

        assert!(rendered.contains("Line one"));
        assert!(rendered.contains("Line two"));
        assert!(rendered.contains("Notes"));
        assert!(rendered.contains("first item"));
        assert!(rendered.contains("second item"));
    }

    #[test]
    fn collapses_comment_bodies_in_detail_tree() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        let summary = issue(122, "Fix login redraw", IssueState::Open, &["bug"]);
        app.set_issues(vec![summary.clone()]);
        app.set_selected_detail(crate::domain::IssueDetail {
            summary,
            body: "Body".to_string(),
            comments: vec![crate::domain::IssueComment {
                author: Some(crate::domain::User {
                    login: "octocat".to_string(),
                }),
                body: "Hidden comment body".to_string(),
                created_at: None,
            }],
        });
        app.toggle_comments();

        let mut buffer = Buffer::empty(Rect::new(0, 0, 110, 28));
        render(&app, buffer.area, &mut buffer);
        let rendered = buffer_to_string(&buffer);

        assert!(rendered.contains("Comments (1)"));
        assert!(!rendered.contains("Hidden comment body"));
    }
}
