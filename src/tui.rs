use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::DefaultTerminal;

use crate::{
    app::{App, FlashKind, IssueStateFilter, UiMode},
    domain::IssueState,
    github::IssueBackend,
    ui::{self, WorktrackEffects},
};

pub async fn run<B: IssueBackend>(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    backend: &B,
) -> Result<()> {
    let mut effects = WorktrackEffects::default();
    refresh(app, backend).await;

    let mut last_frame = Instant::now();
    while !app.should_quit {
        let elapsed = last_frame.elapsed();
        last_frame = Instant::now();
        ui::trigger_flash_effect(app, &mut effects);

        terminal.draw(|frame| {
            let area = frame.area();
            ui::render(app, area, frame.buffer_mut());
            effects.process(elapsed, frame.buffer_mut(), area);
        })?;

        if event::poll(Duration::from_millis(33))?
            && let Event::Key(key) = event::read()?
        {
            handle_key(app, backend, key).await;
        }
    }

    Ok(())
}

async fn handle_key<B: IssueBackend>(app: &mut App, backend: &B, key: KeyEvent) {
    match app.mode {
        UiMode::Browsing => handle_browsing_key(app, backend, key).await,
        UiMode::Search => handle_search_key(app, backend, key).await,
        UiMode::CommentComposer => handle_comment_key(app, backend, key).await,
        UiMode::NewIssue => handle_new_issue_key(app, backend, key).await,
        UiMode::ConfirmClose => handle_confirm_key(app, backend, key).await,
        UiMode::Error => {
            if key.code == KeyCode::Esc {
                app.mode = UiMode::Browsing;
            }
        }
        _ => app.mode = UiMode::Browsing,
    }
}

async fn handle_browsing_key<B: IssueBackend>(app: &mut App, backend: &B, key: KeyEvent) {
    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('j') | KeyCode::Down => app.select_next(),
        KeyCode::Char('k') | KeyCode::Up => app.select_previous(),
        KeyCode::Char('r') => refresh(app, backend).await,
        KeyCode::Char('/') => {
            app.input = app.filters.query.clone();
            app.mode = UiMode::Search;
        }
        KeyCode::Char('f') => {
            app.cycle_state_filter();
            refresh(app, backend).await;
        }
        KeyCode::Char('c') if app.selected_issue().is_some() => {
            app.input.clear();
            app.mode = UiMode::CommentComposer;
            app.set_status("Write a comment, Enter submits, Esc cancels");
        }
        KeyCode::Char('n') => {
            app.input.clear();
            app.mode = UiMode::NewIssue;
            app.set_status("New issue: title | optional body");
        }
        KeyCode::Char('x') if app.selected_issue().is_some() => {
            app.mode = UiMode::ConfirmClose;
        }
        _ => {}
    }
}

async fn handle_search_key<B: IssueBackend>(app: &mut App, backend: &B, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.mode = UiMode::Browsing,
        KeyCode::Enter => {
            app.set_query(app.input.trim().to_string());
            app.mode = UiMode::Browsing;
            refresh(app, backend).await;
        }
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::Char(c) => app.input.push(c),
        _ => {}
    }
}

async fn handle_comment_key<B: IssueBackend>(app: &mut App, backend: &B, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.mode = UiMode::Browsing;
            app.input.clear();
        }
        KeyCode::Enter => submit_comment(app, backend).await,
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => app.input.push(c),
        _ => {}
    }
}

async fn handle_new_issue_key<B: IssueBackend>(app: &mut App, backend: &B, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.mode = UiMode::Browsing;
            app.input.clear();
        }
        KeyCode::Enter => submit_new_issue(app, backend).await,
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => app.input.push(c),
        _ => {}
    }
}

async fn handle_confirm_key<B: IssueBackend>(app: &mut App, backend: &B, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('n') => app.mode = UiMode::Browsing,
        KeyCode::Char('y') | KeyCode::Enter => toggle_issue_state(app, backend).await,
        _ => {}
    }
}

pub async fn refresh<B: IssueBackend>(app: &mut App, backend: &B) {
    app.mode = UiMode::Loading;
    app.flash = Some(FlashKind::Refresh);
    match backend.list_issues(&app.repo, &app.filters).await {
        Ok(issues) => {
            app.set_issues(issues);
            app.mode = UiMode::Browsing;
            app.set_status(format!(
                "Loaded {} {} issues",
                app.issues.len(),
                match app.filters.state {
                    IssueStateFilter::Open => "open",
                    IssueStateFilter::Closed => "closed",
                    IssueStateFilter::All => "total",
                }
            ));
        }
        Err(err) => {
            app.mode = UiMode::Browsing;
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("Refresh failed: {err:#}"));
        }
    }
}

async fn submit_comment<B: IssueBackend>(app: &mut App, backend: &B) {
    let body = app.input.trim().to_string();
    let Some(issue) = app.selected_issue() else {
        app.mode = UiMode::Browsing;
        return;
    };
    let number = issue.number;

    if body.is_empty() {
        app.set_status("Comment cannot be empty");
        app.flash = Some(FlashKind::Error);
        return;
    }

    match backend.add_comment(&app.repo, number, &body).await {
        Ok(_) => {
            app.input.clear();
            app.mode = UiMode::Browsing;
            app.flash = Some(FlashKind::Success);
            app.set_status(format!("Commented on issue #{number}"));
            refresh(app, backend).await;
        }
        Err(err) => {
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("Comment failed: {err:#}"));
        }
    }
}

async fn submit_new_issue<B: IssueBackend>(app: &mut App, backend: &B) {
    let raw = app.input.trim().to_string();
    let (title, body) = raw
        .split_once('|')
        .map_or((raw.as_str(), ""), |(title, body)| {
            (title.trim(), body.trim())
        });

    if title.is_empty() {
        app.set_status("Issue title cannot be empty");
        app.flash = Some(FlashKind::Error);
        return;
    }

    match backend.create_issue(&app.repo, title, body, &[]).await {
        Ok(issue) => {
            app.input.clear();
            app.mode = UiMode::Browsing;
            app.flash = Some(FlashKind::Success);
            app.set_status(format!("Created issue #{}", issue.number));
            refresh(app, backend).await;
        }
        Err(err) => {
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("Create failed: {err:#}"));
        }
    }
}

async fn toggle_issue_state<B: IssueBackend>(app: &mut App, backend: &B) {
    let Some(issue) = app.selected_issue() else {
        app.mode = UiMode::Browsing;
        return;
    };
    let number = issue.number;
    let next_state = match issue.state {
        IssueState::Open => IssueState::Closed,
        IssueState::Closed => IssueState::Open,
    };

    match backend.set_issue_state(&app.repo, number, next_state).await {
        Ok(_) => {
            app.mode = UiMode::Browsing;
            app.flash = Some(FlashKind::Success);
            app.set_status(format!("Updated issue #{number}"));
            refresh(app, backend).await;
        }
        Err(err) => {
            app.mode = UiMode::Browsing;
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("Update failed: {err:#}"));
        }
    }
}
