use std::time::Duration as StdDuration;

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Widget, Wrap},
};
use tachyonfx::{EffectManager, Interpolation, fx};

use crate::{
    app::{App, FlashKind, IssueStateFilter, UiMode},
    domain::{IssueState, IssueSummary},
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

    let items: Vec<ListItem> = app.issues.iter().map(issue_row).collect();
    let list = List::new(items)
        .block(Block::bordered().title("Issues"))
        .highlight_style(
            Style::new()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">");
    let mut state = ratatui::widgets::ListState::default().with_selected(Some(app.selected_index));
    ratatui::widgets::StatefulWidget::render(list, columns[0], buffer, &mut state);

    render_detail(app, columns[1], buffer);
}

fn issue_row(issue: &IssueSummary) -> ListItem<'_> {
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

    ListItem::new(Line::from(vec![
        Span::styled(
            format!("#{: <5}", issue.number),
            Style::new().fg(Color::Cyan),
        ),
        Span::styled(format!("{state: <8}"), state_style(issue.state.clone())),
        Span::styled(format!("{labels: <16}"), Style::new().fg(Color::Magenta)),
        Span::raw(issue.title.clone()),
    ]))
}

fn state_style(state: IssueState) -> Style {
    match state {
        IssueState::Open => Style::new().fg(Color::Green),
        IssueState::Closed => Style::new().fg(Color::DarkGray),
    }
}

fn render_detail(app: &App, area: Rect, buffer: &mut Buffer) {
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

fn render_footer(app: &App, area: Rect, buffer: &mut Buffer) {
    let footer = format!(
        "q quit | r refresh | / search | f filters | c comment | n new issue | x close/reopen\n{}",
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
        let body = match app.mode {
            UiMode::ConfirmClose => "Press y to confirm, Esc to cancel".to_string(),
            UiMode::Error => app.status.clone(),
            _ => app.input.clone(),
        };
        Paragraph::new(body)
            .block(Block::bordered().title(title))
            .wrap(Wrap { trim: false })
            .render(popup, buffer);
    }
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
}
