use std::time::Duration as StdDuration;

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, HighlightSpacing, Paragraph, Row, Table, TableState, Widget,
        Wrap,
    },
};
use ratatui_textarea::{CursorMove, TextArea};
use tachyonfx::{
    EffectManager, Interpolation, Motion, fx,
    pattern::WavePattern,
    wave::{Oscillator, WaveLayer},
};
use tui_tree_widget::{Tree, TreeItem, TreeState};

use crate::{
    app::{
        App, FlashKind, IssueHighlightKind, IssueStateFilter, NewIssueField, PendingAction, UiMode,
    },
    domain::{IssueComment, IssueDetail, IssueState, IssueSummary},
};

const ISSUE_LIST_PERCENT: u16 = 40;
const DETAIL_PANEL_PERCENT: u16 = 60;
const COMMENT_PRIMARY_LABEL: &str = "Submit";
const NEW_ISSUE_PRIMARY_LABEL: &str = "Create";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MouseTarget {
    IssueRow(usize),
    DetailPanel,
    NewIssueField(NewIssueField),
    PrimaryAction,
    CancelAction,
}

#[derive(Default)]
pub struct WorktrackEffects {
    manager: EffectManager<String>,
}

impl WorktrackEffects {
    pub fn trigger_startup_loading(&mut self) {
        self.manager.add_unique_effect(
            "startup-loading",
            fx::parallel(&[
                fx::coalesce_from(
                    Style::new().fg(Color::DarkGray),
                    (720, Interpolation::SineOut),
                ),
                fx::slide_in(
                    Motion::UpToDown,
                    8,
                    0,
                    Color::Reset,
                    (680, Interpolation::SineOut),
                ),
                fx::explode(1.6, 0.35, (520, Interpolation::SineOut)).reversed(),
            ]),
        );
    }

    pub fn trigger_refresh(&mut self) {
        self.manager.add_unique_effect(
            "routine-loading",
            fx::sequence(&[
                fx::slide_in(
                    Motion::LeftToRight,
                    5,
                    0,
                    Color::Reset,
                    (240, Interpolation::SineOut),
                ),
                fx::coalesce_from(
                    Style::new().fg(Color::DarkGray),
                    (360, Interpolation::SineOut),
                )
                .with_pattern(subtle_wave_pattern()),
            ]),
        );
    }

    pub fn trigger_success(&mut self) {
        self.manager.add_unique_effect(
            "success",
            fx::parallel(&[
                fx::slide_in(
                    Motion::DownToUp,
                    4,
                    0,
                    Color::Reset,
                    (260, Interpolation::SineOut),
                ),
                fx::coalesce_from(
                    Style::new().fg(Color::DarkGray),
                    (420, Interpolation::SineOut),
                ),
            ]),
        );
    }

    pub fn trigger_error(&mut self) {
        self.manager.add_unique_effect(
            "error",
            fx::parallel(&[
                fx::dissolve_to(Style::new().fg(Color::Red), (360, Interpolation::SineOut)),
                fx::fade_to_fg(Color::Red, (350, Interpolation::SineOut)),
            ]),
        );
    }

    pub fn process(&mut self, elapsed: StdDuration, buffer: &mut Buffer, area: Rect) {
        self.manager.process_effects(elapsed.into(), buffer, area);
    }

    pub fn has_effects(&self) -> bool {
        self.manager.is_running()
    }
}

fn subtle_wave_pattern() -> WavePattern {
    WavePattern::new(
        WaveLayer::new(Oscillator::sin(0.18, 0.0, 2.2))
            .average(Oscillator::cos(0.0, 0.45, 1.4))
            .amplitude(0.65),
    )
    .with_transition_width(0.2)
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
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(area);

    render_header(app, rows[0], buffer);
    render_filters(app, rows[1], buffer);
    render_body(app, rows[2], buffer);
    render_footer(app, rows[3], buffer);
    render_overlay(app, area, buffer);
}

pub fn effect_area(app: &App, area: Rect) -> Rect {
    match app.mode {
        UiMode::Search
        | UiMode::CommentComposer
        | UiMode::CloseComment
        | UiMode::NewIssue
        | UiMode::ConfirmClose
        | UiMode::Success
        | UiMode::Loading
        | UiMode::Error => centered_rect(72, 55, area),
        UiMode::Browsing | UiMode::FilterEditor => area,
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
        Span::styled(app.repo.to_string(), Style::new().fg(Color::Cyan).bold()),
        Span::raw(format!(
            "    open: {open}  closed: {closed}  all: {}",
            app.issues.len()
        )),
    ]);

    Paragraph::new(header).render(area, buffer);
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
    let filters = format!(
        "[State: {state}] [Labels: {labels}] [Search: {}]",
        app.filters.query
    );

    Paragraph::new(filters)
        .block(Block::default().borders(Borders::BOTTOM))
        .style(Style::new().fg(Color::Yellow))
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
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Length(10),
        Constraint::Min(10),
    ];
    let header = Row::new(["Number", "State", "Labels", "Title"])
        .style(Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .bottom_margin(1);
    let rows = app.issues.iter().map(|issue| {
        issue_row(
            issue,
            app.issue_highlight_kind(issue.number),
            app.new_issue_animation_frame,
        )
    });
    let table = Table::new(rows, widths)
        .block(Block::bordered().title("Issues"))
        .header(header)
        .row_highlight_style(
            Style::new()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">")
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

    let title = if let Some(kind) = highlight.as_ref() {
        let badge = match kind {
            IssueHighlightKind::New => "NEW ",
            IssueHighlightKind::Mention => "PING ",
        };
        Cell::from(Line::from(vec![
            Span::styled(badge, Style::new().fg(Color::Yellow).bold()),
            Span::raw(issue.title.clone()),
        ]))
    } else {
        Cell::from(issue.title.clone())
    };

    let row = Row::new([
        Cell::from(format!("#{}", issue.number)).style(Style::new().fg(Color::Cyan)),
        Cell::from(state).style(state_style(issue.state.clone())),
        Cell::from(labels).style(Style::new().fg(Color::Magenta)),
        title,
    ]);

    if let Some(kind) = highlight {
        let pulse_is_high = (animation_frame / 8).is_multiple_of(2);
        let style = if pulse_is_high {
            match kind {
                IssueHighlightKind::New => Style::new().fg(Color::Black).bg(Color::LightYellow),
                IssueHighlightKind::Mention => Style::new().fg(Color::Black).bg(Color::LightCyan),
            }
            .add_modifier(Modifier::BOLD)
        } else {
            match kind {
                IssueHighlightKind::New => Style::new().fg(Color::Yellow).bg(Color::DarkGray),
                IssueHighlightKind::Mention => Style::new().fg(Color::Cyan).bg(Color::DarkGray),
            }
            .add_modifier(Modifier::BOLD)
        };
        row.style(style)
    } else {
        row
    }
}

fn state_style(state: IssueState) -> Style {
    match state {
        IssueState::Open => Style::new().fg(Color::Green),
        IssueState::Closed => Style::new().fg(Color::DarkGray),
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
        format!(
            "{}\n\nlabels: {}\ncomments: {}\n\nPress c to comment, x to close/reopen.",
            issue.title, labels, issue.comment_count
        )
    } else {
        "No issues loaded. Press r to refresh.".to_string()
    };

    Paragraph::new(detail)
        .block(Block::bordered().title("Detail"))
        .wrap(Wrap { trim: true })
        .render(area, buffer);
}

fn render_detail_tree(app: &App, detail: &IssueDetail, area: Rect, buffer: &mut Buffer) {
    let items = detail_tree_items(detail);
    let tree = Tree::new(&items)
        .expect("detail tree item identifiers are unique")
        .block(Block::bordered().title(format!("Detail #{}", detail.summary.number)))
        .highlight_style(Style::new().bg(Color::DarkGray).fg(Color::White))
        .node_open_symbol("- ")
        .node_closed_symbol("+ ")
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
        .style(Style::new().fg(Color::Gray))
        .render(area, buffer);
}

fn footer_shortcuts(app: &App) -> &'static str {
    match app.mode {
        UiMode::Browsing => {
            "q quit | r refresh | / search | f filter | Enter fold | c comment | n new issue | x close/reopen"
        }
        UiMode::Search => "Enter search | Esc cancel | Backspace delete",
        UiMode::CommentComposer => "Ctrl+D/S submit | Enter newline | Ctrl+J newline | Esc cancel",
        UiMode::CloseComment => "Ctrl+D/S close | Enter newline | Ctrl+J newline | Esc cancel",
        UiMode::NewIssue => {
            "Tab/Shift+Tab fields | Ctrl+D/S create | Enter edit/add label | Ctrl+J newline | Esc cancel"
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
        UiMode::ConfirmClose => Some("Confirm"),
        UiMode::Success => Some("Done"),
        UiMode::Loading => Some("Working"),
        UiMode::Error => Some("Error"),
        _ => None,
    };

    if let Some(title) = title {
        let popup = if app.mode == UiMode::NewIssue {
            centered_rect(72, 70, area)
        } else {
            centered_rect(72, 55, area)
        };
        Clear.render(popup, buffer);
        match app.mode {
            UiMode::NewIssue => render_new_issue_editor(app, popup, buffer),
            UiMode::Loading => render_loading_overlay(app, popup, buffer),
            UiMode::ConfirmClose => Paragraph::new("Press y to reopen, Esc to cancel")
                .block(Block::bordered().title(title))
                .wrap(Wrap { trim: false })
                .render(popup, buffer),
            UiMode::Error => Paragraph::new(app.status.clone())
                .block(Block::bordered().title(title))
                .wrap(Wrap { trim: false })
                .render(popup, buffer),
            UiMode::Success => Paragraph::new(format!("{}\n\nState reloaded.", app.status))
                .block(Block::bordered().title(title))
                .style(Style::new().fg(Color::Green))
                .wrap(Wrap { trim: false })
                .render(popup, buffer),
            _ => render_text_editor(app, title, popup, buffer),
        }
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
        .block(Block::bordered().title("Working"))
        .style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .wrap(Wrap { trim: false })
        .render(area, buffer);
}

fn pending_action_label(action: &PendingAction) -> &'static str {
    match action {
        PendingAction::Refresh => "Refreshing issues",
        PendingAction::LoadLabels => "Loading labels",
        PendingAction::CreateIssue => "Creating issue",
        PendingAction::AddComment => "Adding comment",
        PendingAction::CloseIssue => "Closing issue",
        PendingAction::ReopenIssue => "Reopening issue",
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
    textarea.set_block(Block::bordered().title(match app.mode {
        UiMode::CommentComposer => "Comment Body",
        UiMode::CloseComment => "Closing Comment",
        UiMode::NewIssue => "New Issue: title | body",
        UiMode::Search => "Search Issues",
        _ => title,
    }));
    textarea.set_placeholder_text(match app.mode {
        UiMode::CommentComposer => "Write a comment",
        UiMode::CloseComment => "Required comment before closing",
        UiMode::NewIssue => "Title | optional body",
        UiMode::Search => "Search issue titles",
        _ => "",
    });
    textarea.set_style(Style::new().fg(Color::White));
    set_visible_cursor(&mut textarea);
    (&textarea).render(rows[0], buffer);

    if matches!(app.mode, UiMode::CommentComposer | UiMode::CloseComment) {
        render_action_buttons(COMMENT_PRIMARY_LABEL, rows[1], buffer);
    }
}

fn render_new_issue_editor(app: &App, area: Rect, buffer: &mut Buffer) {
    let block = Block::bordered().title("New Issue");
    let inner = block.inner(area);
    block.render(area, buffer);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(6),
            Constraint::Length(3),
        ])
        .split(inner.inner(Margin {
            horizontal: 1,
            vertical: 0,
        }));

    let title_active = app.new_issue_field == NewIssueField::Title;
    let mut title = textarea_at_end(input_lines(&app.input));
    title.set_block(Block::bordered().title("Title"));
    title.set_style(field_style(
        &app.new_issue_field,
        &NewIssueField::Title,
        Color::White,
    ));
    if title_active {
        set_visible_cursor(&mut title);
    } else {
        hide_cursor(&mut title);
    }
    (&title).render(rows[0], buffer);

    let body_active = app.new_issue_field == NewIssueField::Body;
    let mut body = textarea_at_end(input_lines(&app.body_input));
    body.set_block(Block::bordered().title("Body (Markdown)"));
    body.set_placeholder_text("Write the issue body");
    body.set_style(field_style(
        &app.new_issue_field,
        &NewIssueField::Body,
        Color::White,
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
            Span::styled(" ", Style::new().fg(Color::Black).bg(Color::Cyan)),
        ])
    } else {
        Line::from(format!("input: {}", app.label_input))
    };
    Paragraph::new(vec![
        Line::from(format!("selected: {selected}")),
        input_line,
        Line::from(format!("suggestions: {suggestions}")),
    ])
    .block(Block::bordered().title("Labels"))
    .style(field_style(
        &app.new_issue_field,
        &NewIssueField::Labels,
        Color::Magenta,
    ))
    .wrap(Wrap { trim: false })
    .render(rows[2], buffer);

    render_action_buttons(NEW_ISSUE_PRIMARY_LABEL, rows[3], buffer);
}

fn render_action_buttons(primary: &'static str, area: Rect, buffer: &mut Buffer) {
    let buttons = Line::from(vec![
        Span::styled(
            format!(" {primary} Ctrl+D/S "),
            Style::new()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            " Cancel Esc ",
            Style::new()
                .fg(Color::White)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    Paragraph::new(buttons)
        .block(Block::bordered().title("Actions"))
        .render(area, buffer);
}

fn field_style(active: &NewIssueField, field: &NewIssueField, color: Color) -> Style {
    if active == field {
        Style::new()
            .fg(color)
            .add_modifier(Modifier::BOLD)
            .bg(Color::DarkGray)
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
    textarea.set_cursor_line_style(Style::new().bg(Color::DarkGray));
    textarea.set_cursor_style(Style::new().fg(Color::Black).bg(Color::Cyan));
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
        UiMode::Success => Some(MouseTarget::CancelAction),
        _ => None,
    }
}

fn browsing_mouse_target(app: &App, area: Rect, point: Rect) -> Option<MouseTarget> {
    let body = main_rows(area)[2];
    let columns = body_columns(body);
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
            Constraint::Length(6),
            Constraint::Length(3),
        ])
        .split(inner)
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
    format!(" {primary} Ctrl+D/S ").len() as u16
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
        domain::{IssueState, IssueSummary, Label},
    };
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

        let mut buffer = Buffer::empty(Rect::new(0, 0, 96, 24));
        render(&app, buffer.area, &mut buffer);
        let rendered = buffer_to_string(&buffer);

        assert!(rendered.contains("owner/skunkwork"));
        assert!(rendered.contains("State: open"));
        assert!(rendered.contains("Search: redraw"));
        assert!(rendered.contains("Number"));
        assert!(rendered.contains("State"));
        assert!(rendered.contains("Labels"));
        assert!(rendered.contains("Title"));
        assert!(rendered.contains("#122"));
        assert!(rendered.contains("open"));
        assert!(rendered.contains("Fix login redraw"));
        assert!(rendered.contains("c comment"));
        assert!(rendered.contains("n new issue"));
        assert!(rendered.contains("x close/reopen"));
    }

    #[test]
    fn footer_shortcuts_follow_active_screen() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());

        assert!(footer_shortcuts(&app).contains("q quit"));

        app.mode = UiMode::NewIssue;
        assert!(footer_shortcuts(&app).contains("Ctrl+D/S create"));
        assert!(!footer_shortcuts(&app).contains("q quit"));

        app.mode = UiMode::CommentComposer;
        assert!(footer_shortcuts(&app).contains("Ctrl+D/S submit"));
        assert!(!footer_shortcuts(&app).contains("n new issue"));

        app.mode = UiMode::CloseComment;
        assert!(footer_shortcuts(&app).contains("Ctrl+D/S close"));

        app.mode = UiMode::Search;
        assert!(footer_shortcuts(&app).contains("Enter search"));
    }

    #[test]
    fn renders_new_issue_highlight_in_issue_list() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_issues(vec![
            issue(122, "Fix login redraw", IssueState::Open, &["bug"]),
            issue(130, "Fresh", IssueState::Open, &[]),
        ]);
        app.highlight_new_issues(vec![130]);

        let mut buffer = Buffer::empty(Rect::new(0, 0, 96, 24));
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

        let mut buffer = Buffer::empty(Rect::new(0, 0, 96, 24));
        render(&app, buffer.area, &mut buffer);
        let rendered = buffer_to_string(&buffer);

        assert!(rendered.contains("#130"));
        assert!(rendered.contains("PING"));
        assert!(rendered.contains("Ping"));
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
        assert!(rendered.contains("Submit Ctrl+D/S"));
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
        assert!(rendered.contains("Submit Ctrl+D/S"));
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
        assert!(rendered.contains("Create Ctrl+D/S"));
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
