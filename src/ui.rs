use std::time::Duration as StdDuration;

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, HighlightSpacing, Paragraph, Row, Table, TableState, Widget,
        Wrap,
    },
};
use ratatui_textarea::TextArea;
use tachyonfx::{EffectManager, Interpolation, fx};
use tui_tree_widget::{Tree, TreeItem, TreeState};

use crate::{
    app::{App, FlashKind, IssueStateFilter, UiMode},
    domain::{IssueComment, IssueDetail, IssueState, IssueSummary},
};

#[derive(Default)]
pub struct WorktrackEffects {
    manager: EffectManager<String>,
}

impl WorktrackEffects {
    pub fn trigger_refresh(&mut self) {
        self.manager.add_unique_effect(
            "refresh",
            fx::fade_to_fg(Color::Cyan, (450, Interpolation::SineInOut)),
        );
    }

    pub fn trigger_success(&mut self) {
        self.manager.add_unique_effect(
            "success",
            fx::fade_to_fg(Color::Green, (350, Interpolation::SineOut)),
        );
    }

    pub fn trigger_error(&mut self) {
        self.manager.add_unique_effect(
            "error",
            fx::fade_to_fg(Color::Red, (350, Interpolation::SineOut)),
        );
    }

    pub fn process(&mut self, elapsed: StdDuration, buffer: &mut Buffer, area: Rect) {
        self.manager.process_effects(elapsed.into(), buffer, area);
    }

    pub fn has_effects(&self) -> bool {
        self.manager.is_running()
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
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let widths = [
        Constraint::Length(8),
        Constraint::Length(9),
        Constraint::Length(16),
        Constraint::Min(20),
    ];
    let header = Row::new(["Number", "State", "Labels", "Title"])
        .style(Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .bottom_margin(1);
    let rows = app.issues.iter().map(issue_row);
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

fn issue_row(issue: &IssueSummary) -> Row<'_> {
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

    Row::new([
        Cell::from(format!("#{}", issue.number)).style(Style::new().fg(Color::Cyan)),
        Cell::from(state).style(state_style(issue.state.clone())),
        Cell::from(labels).style(Style::new().fg(Color::Magenta)),
        Cell::from(issue.title.clone()),
    ])
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
    let footer = format!(
        "q quit | r refresh | / search | f filter | Enter fold | c comment | n new issue | x close/reopen\n{}",
        app.status
    );
    Paragraph::new(footer)
        .style(Style::new().fg(Color::Gray))
        .render(area, buffer);
}

fn render_overlay(app: &App, area: Rect, buffer: &mut Buffer) {
    let title = match app.mode {
        UiMode::Search => Some("Search"),
        UiMode::CommentComposer => Some("Comment"),
        UiMode::NewIssue => Some("New Issue"),
        UiMode::ConfirmClose => Some("Confirm"),
        UiMode::Error => Some("Error"),
        _ => None,
    };

    if let Some(title) = title {
        let popup = centered_rect(64, 35, area);
        Clear.render(popup, buffer);
        match app.mode {
            UiMode::ConfirmClose => Paragraph::new("Press y to confirm, Esc to cancel")
                .block(Block::bordered().title(title))
                .wrap(Wrap { trim: false })
                .render(popup, buffer),
            UiMode::Error => Paragraph::new(app.status.clone())
                .block(Block::bordered().title(title))
                .wrap(Wrap { trim: false })
                .render(popup, buffer),
            _ => render_text_editor(app, title, popup, buffer),
        }
    }
}

fn render_text_editor(app: &App, title: &'static str, area: Rect, buffer: &mut Buffer) {
    let lines = if app.input.is_empty() {
        vec![String::new()]
    } else {
        app.input.lines().map(ToOwned::to_owned).collect()
    };
    let mut textarea = TextArea::new(lines);
    textarea.set_block(Block::bordered().title(match app.mode {
        UiMode::CommentComposer => "Comment Body",
        UiMode::NewIssue => "New Issue: title | body",
        UiMode::Search => "Search Issues",
        _ => title,
    }));
    textarea.set_placeholder_text(match app.mode {
        UiMode::CommentComposer => "Write a comment",
        UiMode::NewIssue => "Title | optional body",
        UiMode::Search => "Search issue titles",
        _ => "",
    });
    textarea.set_cursor_line_style(Style::new().bg(Color::DarkGray));
    textarea.set_cursor_style(Style::new().fg(Color::Black).bg(Color::Cyan));
    textarea.set_style(Style::new().fg(Color::White));
    (&textarea).render(area, buffer);
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
    fn creates_refresh_effects() {
        let mut effects = WorktrackEffects::default();

        effects.trigger_refresh();
        assert!(effects.has_effects());
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
