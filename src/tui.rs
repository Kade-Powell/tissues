use std::{
    collections::BTreeSet,
    io::{self, Write},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use color_eyre::eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::DefaultTerminal;

use crate::{
    app::{App, FlashKind, IssueStateFilter, NewIssueField, PendingAction, UiMode},
    domain::{IssueState, IssueSummary},
    github::IssueBackend,
    ui::{self, WorktrackEffects},
};

const AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
#[cfg(target_os = "macos")]
const MACOS_NOTIFICATION_SOUND: &str = "/System/Library/Sounds/Glass.aiff";

pub async fn run<B: IssueBackend>(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    backend: &B,
) -> Result<()> {
    let mut effects = WorktrackEffects::default();
    app.begin_action(PendingAction::Refresh, "Starting Worktrack");
    effects.trigger_startup_loading();
    draw_app(terminal, app, &mut effects, Duration::from_millis(120))?;
    refresh(app, backend).await;

    let mut last_frame = Instant::now();
    let mut next_auto_refresh = Instant::now() + AUTO_REFRESH_INTERVAL;
    while !app.should_quit {
        let elapsed = last_frame.elapsed();
        last_frame = Instant::now();
        ui::trigger_flash_effect(app, &mut effects);

        draw_app(terminal, app, &mut effects, elapsed)?;

        if can_auto_refresh(app) && Instant::now() >= next_auto_refresh {
            if auto_refresh(app, backend).await.has_new_issues() {
                play_new_issue_notification();
            }
            next_auto_refresh = Instant::now() + AUTO_REFRESH_INTERVAL;
        } else if !can_auto_refresh(app) {
            next_auto_refresh = Instant::now() + AUTO_REFRESH_INTERVAL;
        }

        if event::poll(Duration::from_millis(33))?
            && let Event::Key(key) = event::read()?
        {
            if let Some((action, status)) = loading_preview(app, key) {
                let mut preview = app.clone();
                preview.begin_action(action, status);
                ui::trigger_flash_effect(&mut preview, &mut effects);
                draw_app(terminal, &preview, &mut effects, last_frame.elapsed())?;
            }
            handle_key(app, backend, key).await;
        }
    }

    Ok(())
}

fn can_auto_refresh(app: &App) -> bool {
    matches!(app.mode, UiMode::Browsing)
}

fn ring_terminal_bell() {
    print!("\x07");
    let _ = io::stdout().flush();
}

fn play_new_issue_notification() {
    ring_terminal_bell();
    play_system_notification_sound();
}

fn play_system_notification_sound() {
    if let Some((program, args)) = system_notification_sound_command() {
        thread::spawn(move || {
            let _ = Command::new(program).args(args).status();
        });
    }
}

#[cfg(target_os = "macos")]
fn system_notification_sound_command() -> Option<(&'static str, &'static [&'static str])> {
    Some(("afplay", &[MACOS_NOTIFICATION_SOUND]))
}

#[cfg(not(target_os = "macos"))]
fn system_notification_sound_command() -> Option<(&'static str, &'static [&'static str])> {
    None
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct AutoRefreshOutcome {
    new_issue_numbers: Vec<u64>,
}

impl AutoRefreshOutcome {
    fn has_new_issues(&self) -> bool {
        !self.new_issue_numbers.is_empty()
    }
}

fn draw_app(
    terminal: &mut DefaultTerminal,
    app: &App,
    effects: &mut WorktrackEffects,
    elapsed: Duration,
) -> Result<()> {
    terminal.draw(|frame| {
        let area = frame.area();
        ui::render(app, area, frame.buffer_mut());
        effects.process(elapsed, frame.buffer_mut(), area);
    })?;
    Ok(())
}

fn loading_preview(app: &App, key: KeyEvent) -> Option<(PendingAction, String)> {
    match app.mode {
        UiMode::Browsing => match key.code {
            KeyCode::Char('r') | KeyCode::Char('f') => {
                Some((PendingAction::Refresh, "Refreshing issues".to_string()))
            }
            KeyCode::Char('n') => Some((
                PendingAction::LoadLabels,
                "Loading repository labels".to_string(),
            )),
            _ => None,
        },
        UiMode::Search if key.code == KeyCode::Enter => {
            Some((PendingAction::Refresh, "Refreshing issues".to_string()))
        }
        UiMode::CommentComposer | UiMode::CloseComment
            if key.code == KeyCode::Char('s')
                && key.modifiers.contains(KeyModifiers::CONTROL)
                && !app.input.trim().is_empty() =>
        {
            app.selected_issue().map(|issue| {
                let action = if app.mode == UiMode::CloseComment {
                    PendingAction::CloseIssue
                } else {
                    PendingAction::AddComment
                };
                let status = if app.mode == UiMode::CloseComment {
                    format!("Closing issue #{}", issue.number)
                } else {
                    format!("Adding comment to #{}", issue.number)
                };
                (action, status)
            })
        }
        UiMode::NewIssue if should_submit_new_issue(app, key) && !app.input.trim().is_empty() => {
            Some((PendingAction::CreateIssue, "Creating issue".to_string()))
        }
        UiMode::ConfirmClose
            if key.code == KeyCode::Enter || matches!(key.code, KeyCode::Char('y')) =>
        {
            app.selected_issue().map(|issue| {
                (
                    pending_state_action(issue.state.clone()),
                    format!("Updating issue #{}", issue.number),
                )
            })
        }
        _ => None,
    }
}

fn pending_state_action(state: IssueState) -> PendingAction {
    match state {
        IssueState::Open => PendingAction::CloseIssue,
        IssueState::Closed => PendingAction::ReopenIssue,
    }
}

fn should_submit_new_issue(app: &App, key: KeyEvent) -> bool {
    if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return true;
    }

    key.code == KeyCode::Enter
        && app.new_issue_field == NewIssueField::Labels
        && app.label_input.trim().is_empty()
}

async fn handle_key<B: IssueBackend>(app: &mut App, backend: &B, key: KeyEvent) {
    match app.mode {
        UiMode::Browsing => handle_browsing_key(app, backend, key).await,
        UiMode::Search => handle_search_key(app, backend, key).await,
        UiMode::CommentComposer => handle_comment_key(app, backend, key).await,
        UiMode::CloseComment => handle_close_comment_key(app, backend, key).await,
        UiMode::NewIssue => handle_new_issue_key(app, backend, key).await,
        UiMode::ConfirmClose => handle_confirm_key(app, backend, key).await,
        UiMode::Success => {
            app.mode = UiMode::Browsing;
        }
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
        KeyCode::Char('j') | KeyCode::Down => {
            let previous = app.selected_issue().map(|issue| issue.number);
            app.select_next();
            if app.selected_issue().map(|issue| issue.number) != previous {
                refresh_selected_detail(app, backend).await;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let previous = app.selected_issue().map(|issue| issue.number);
            app.select_previous();
            if app.selected_issue().map(|issue| issue.number) != previous {
                refresh_selected_detail(app, backend).await;
            }
        }
        KeyCode::Enter if app.selected_detail.is_some() => app.toggle_comments(),
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
            app.set_status("Write a comment, Enter adds lines, Ctrl+S submits");
        }
        KeyCode::Char('n') => open_new_issue(app, backend).await,
        KeyCode::Char('x') if app.selected_issue().is_some() => {
            match app.selected_issue().map(|issue| issue.state.clone()) {
                Some(IssueState::Open) => {
                    app.input.clear();
                    app.mode = UiMode::CloseComment;
                    app.set_status("Closing requires a comment, Enter adds lines, Ctrl+S closes");
                }
                Some(IssueState::Closed) => {
                    app.mode = UiMode::ConfirmClose;
                }
                None => {}
            }
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
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => app.input.push('\n'),
        KeyCode::Enter => app.input.push('\n'),
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            submit_comment(app, backend).await;
        }
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.input.push('\n');
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => app.input.push(c),
        _ => {}
    }
}

async fn handle_close_comment_key<B: IssueBackend>(app: &mut App, backend: &B, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.mode = UiMode::Browsing;
            app.input.clear();
        }
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => app.input.push('\n'),
        KeyCode::Enter => app.input.push('\n'),
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            close_issue_with_comment(app, backend).await;
        }
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.input.push('\n');
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
            app.body_input.clear();
            app.label_input.clear();
            app.new_issue_labels.clear();
        }
        KeyCode::Tab => app.next_new_issue_field(),
        KeyCode::BackTab => app.previous_new_issue_field(),
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            submit_new_issue(app, backend, true).await
        }
        KeyCode::Enter
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && app.new_issue_field == NewIssueField::Body =>
        {
            app.body_input.push('\n');
        }
        KeyCode::Enter if app.new_issue_field == NewIssueField::Title => {
            app.new_issue_field = NewIssueField::Body;
        }
        KeyCode::Enter if app.new_issue_field == NewIssueField::Body => {
            app.body_input.push('\n');
        }
        KeyCode::Enter => submit_new_issue(app, backend, false).await,
        KeyCode::Backspace => {
            backspace_new_issue_field(app);
        }
        KeyCode::Char('j')
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && app.new_issue_field == NewIssueField::Body =>
        {
            app.body_input.push('\n');
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            push_new_issue_char(app, c);
        }
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
    app.begin_action(PendingAction::Refresh, "Refreshing issues");
    match backend.list_issues(&app.repo, &app.filters).await {
        Ok(issues) => {
            app.set_issues(issues);
            app.mode = UiMode::Browsing;
            let loaded_status = format!(
                "Loaded {} {} issues",
                app.issues.len(),
                match app.filters.state {
                    IssueStateFilter::Open => "open",
                    IssueStateFilter::Closed => "closed",
                    IssueStateFilter::All => "total",
                }
            );
            match load_selected_detail(app, backend).await {
                Ok(()) => {
                    app.finish_action();
                    app.set_status(loaded_status);
                }
                Err(err) => {
                    app.finish_action();
                    app.flash = Some(FlashKind::Error);
                    app.set_status(format!("{loaded_status}; detail failed: {err:#}"));
                }
            }
        }
        Err(err) => {
            app.finish_action();
            app.mode = UiMode::Browsing;
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("Refresh failed: {err:#}"));
        }
    }
}

async fn auto_refresh<B: IssueBackend>(app: &mut App, backend: &B) -> AutoRefreshOutcome {
    let previous_numbers = app
        .issues
        .iter()
        .map(|issue| issue.number)
        .collect::<BTreeSet<_>>();
    let selected_issue_number = app.selected_issue().map(|issue| issue.number);

    match backend.list_issues(&app.repo, &app.filters).await {
        Ok(issues) => {
            let new_issue_numbers = issues
                .iter()
                .filter(|issue| !previous_numbers.contains(&issue.number))
                .map(|issue| issue.number)
                .collect::<Vec<_>>();
            let new_issue_status = new_issue_notice(&issues, &new_issue_numbers);

            app.set_issues(issues);
            if let Some(number) = selected_issue_number {
                app.select_issue_number(number);
            }

            match load_selected_detail(app, backend).await {
                Ok(()) => {
                    app.finish_action();
                    app.flash = Some(if new_issue_numbers.is_empty() {
                        FlashKind::Refresh
                    } else {
                        FlashKind::Success
                    });
                    if let Some(status) = new_issue_status {
                        app.set_status(status);
                    } else {
                        app.set_status("Auto-refreshed; no new issues");
                    }
                }
                Err(err) => {
                    app.finish_action();
                    app.flash = Some(FlashKind::Error);
                    app.set_status(format!("Auto-refresh detail failed: {err:#}"));
                }
            }

            AutoRefreshOutcome { new_issue_numbers }
        }
        Err(err) => {
            app.finish_action();
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("Auto-refresh failed: {err:#}"));
            AutoRefreshOutcome::default()
        }
    }
}

fn new_issue_notice(issues: &[IssueSummary], new_issue_numbers: &[u64]) -> Option<String> {
    let first_number = new_issue_numbers.first()?;
    let first_issue = issues.iter().find(|issue| issue.number == *first_number)?;

    if new_issue_numbers.len() == 1 {
        Some(format!(
            "New issue #{}: {}",
            first_issue.number, first_issue.title
        ))
    } else {
        Some(format!(
            "{} new issues; newest #{}: {}",
            new_issue_numbers.len(),
            first_issue.number,
            first_issue.title
        ))
    }
}

async fn refresh_after_action<B: IssueBackend>(
    app: &mut App,
    backend: &B,
    selected_issue_number: Option<u64>,
    success_status: String,
) {
    match backend.list_issues(&app.repo, &app.filters).await {
        Ok(issues) => {
            app.set_issues(issues);
            if let Some(number) = selected_issue_number {
                app.select_issue_number(number);
            }
            app.mode = UiMode::Browsing;
            match load_selected_detail(app, backend).await {
                Ok(()) => {
                    app.finish_action();
                    app.mode = UiMode::Success;
                    app.flash = Some(FlashKind::Success);
                    app.set_status(success_status);
                }
                Err(err) => {
                    app.finish_action();
                    app.flash = Some(FlashKind::Error);
                    app.set_status(format!("{success_status}; detail refresh failed: {err:#}"));
                }
            }
        }
        Err(err) => {
            app.finish_action();
            app.mode = UiMode::Browsing;
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("{success_status}; refresh failed: {err:#}"));
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

    app.begin_action(
        PendingAction::AddComment,
        format!("Adding comment to #{number}"),
    );
    match backend.add_comment(&app.repo, number, &body).await {
        Ok(_) => {
            app.input.clear();
            refresh_after_action(
                app,
                backend,
                Some(number),
                format!("Commented on issue #{number}"),
            )
            .await;
        }
        Err(err) => {
            app.finish_action();
            app.mode = UiMode::Browsing;
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("Comment failed: {err:#}"));
        }
    }
}

async fn close_issue_with_comment<B: IssueBackend>(app: &mut App, backend: &B) {
    let body = app.input.trim().to_string();
    let Some(issue) = app.selected_issue() else {
        app.mode = UiMode::Browsing;
        return;
    };
    let number = issue.number;

    if body.is_empty() {
        app.set_status("Close comment cannot be empty");
        app.flash = Some(FlashKind::Error);
        return;
    }

    app.begin_action(
        PendingAction::CloseIssue,
        format!("Closing issue #{number}"),
    );
    match backend.add_comment(&app.repo, number, &body).await {
        Ok(_) => match backend
            .set_issue_state(&app.repo, number, IssueState::Closed)
            .await
        {
            Ok(_) => {
                app.input.clear();
                refresh_after_action(
                    app,
                    backend,
                    Some(number),
                    format!("Closed issue #{number} with comment"),
                )
                .await;
            }
            Err(err) => {
                app.finish_action();
                app.mode = UiMode::Browsing;
                app.flash = Some(FlashKind::Error);
                app.set_status(format!("Close failed after comment: {err:#}"));
            }
        },
        Err(err) => {
            app.finish_action();
            app.mode = UiMode::CloseComment;
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("Close comment failed: {err:#}"));
        }
    }
}

async fn submit_new_issue<B: IssueBackend>(app: &mut App, backend: &B, force_create: bool) {
    if app.new_issue_field == NewIssueField::Labels && !app.label_input.trim().is_empty() {
        if !app.accept_first_label_suggestion() {
            app.add_new_issue_label(app.label_input.trim().to_string());
        }
        if !force_create {
            return;
        }
    }

    let title = app.input.trim().to_string();
    let body = app.body_input.trim().to_string();
    let labels = app.new_issue_labels.clone();

    if title.is_empty() {
        app.set_status("Issue title cannot be empty");
        app.flash = Some(FlashKind::Error);
        return;
    }

    app.begin_action(PendingAction::CreateIssue, "Creating issue");
    match backend
        .create_issue(&app.repo, &title, &body, &labels)
        .await
    {
        Ok(issue) => {
            let number = issue.number;
            app.input.clear();
            app.body_input.clear();
            app.label_input.clear();
            app.new_issue_labels.clear();
            refresh_after_action(
                app,
                backend,
                Some(number),
                format!("Created issue #{number}"),
            )
            .await;
        }
        Err(err) => {
            app.finish_action();
            app.mode = UiMode::NewIssue;
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("Create failed: {err:#}"));
        }
    }
}

async fn open_new_issue<B: IssueBackend>(app: &mut App, backend: &B) {
    app.start_new_issue();
    app.begin_action(PendingAction::LoadLabels, "Loading repository labels");
    match backend.list_labels(&app.repo).await {
        Ok(labels) => {
            app.finish_action();
            app.mode = UiMode::NewIssue;
            app.set_repo_labels(labels);
            app.set_status("New issue: Tab fields, Enter edits, Ctrl+S creates");
        }
        Err(err) => {
            app.finish_action();
            app.mode = UiMode::NewIssue;
            app.set_repo_labels(Vec::new());
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("Labels unavailable: {err:#}; creating still works"));
        }
    }
}

fn push_new_issue_char(app: &mut App, c: char) {
    match app.new_issue_field {
        NewIssueField::Title => app.input.push(c),
        NewIssueField::Body => app.body_input.push(c),
        NewIssueField::Labels => app.label_input.push(c),
    }
}

fn backspace_new_issue_field(app: &mut App) {
    match app.new_issue_field {
        NewIssueField::Title => {
            app.input.pop();
        }
        NewIssueField::Body => {
            app.body_input.pop();
        }
        NewIssueField::Labels => {
            if app.label_input.is_empty() {
                app.pop_new_issue_label();
            } else {
                app.label_input.pop();
            }
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
    let pending_action = match next_state {
        IssueState::Open => PendingAction::ReopenIssue,
        IssueState::Closed => PendingAction::CloseIssue,
    };

    app.begin_action(pending_action, format!("Updating issue #{number}"));
    match backend.set_issue_state(&app.repo, number, next_state).await {
        Ok(_) => {
            refresh_after_action(
                app,
                backend,
                Some(number),
                format!("Updated issue #{number}"),
            )
            .await;
        }
        Err(err) => {
            app.finish_action();
            app.mode = UiMode::Browsing;
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("Update failed: {err:#}"));
        }
    }
}

async fn refresh_selected_detail<B: IssueBackend>(app: &mut App, backend: &B) {
    if let Err(err) = load_selected_detail(app, backend).await {
        app.flash = Some(FlashKind::Error);
        app.set_status(format!("Detail refresh failed: {err:#}"));
    }
}

async fn load_selected_detail<B: IssueBackend>(app: &mut App, backend: &B) -> Result<()> {
    let Some(number) = app.selected_issue().map(|issue| issue.number) else {
        app.clear_selected_detail();
        return Ok(());
    };

    let detail = backend.get_issue(&app.repo, number).await?;
    app.set_selected_detail(detail);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use color_eyre::eyre::Result;
    use std::sync::Mutex;

    use crate::{
        app::IssueFilters,
        domain::{IssueComment, IssueDetail, IssueSummary, Label},
        repo::Repository,
    };

    #[derive(Default)]
    struct MockBackend {
        list_calls: Mutex<usize>,
        detail_calls: Mutex<usize>,
        created_body: Mutex<Option<String>>,
        created_labels: Mutex<Vec<String>>,
        commented_body: Mutex<Option<String>>,
        state_updates: Mutex<Vec<(u64, IssueState)>>,
        issues: Mutex<Vec<IssueSummary>>,
        labels: Mutex<Vec<Label>>,
    }

    fn issue(number: u64, title: &str, state: IssueState, comment_count: u64) -> IssueSummary {
        IssueSummary {
            number,
            title: title.to_string(),
            state,
            labels: Vec::new(),
            assignees: Vec::new(),
            author: None,
            updated_at: None,
            comment_count,
        }
    }

    fn backend_with_issues(issues: Vec<IssueSummary>) -> MockBackend {
        MockBackend {
            list_calls: Mutex::new(0),
            detail_calls: Mutex::new(0),
            created_body: Mutex::new(None),
            created_labels: Mutex::new(Vec::new()),
            commented_body: Mutex::new(None),
            state_updates: Mutex::new(Vec::new()),
            issues: Mutex::new(issues),
            labels: Mutex::new(vec![
                Label {
                    name: "bug".to_string(),
                },
                Label {
                    name: "docs".to_string(),
                },
            ]),
        }
    }

    #[async_trait]
    impl IssueBackend for MockBackend {
        async fn list_issues(
            &self,
            _repo: &Repository,
            _filters: &IssueFilters,
        ) -> Result<Vec<IssueSummary>> {
            *self.list_calls.lock().unwrap() += 1;
            Ok(self.issues.lock().unwrap().clone())
        }

        async fn get_issue(&self, _repo: &Repository, number: u64) -> Result<IssueDetail> {
            *self.detail_calls.lock().unwrap() += 1;
            let summary = self
                .issues
                .lock()
                .unwrap()
                .iter()
                .find(|issue| issue.number == number)
                .cloned()
                .unwrap();
            Ok(IssueDetail {
                summary,
                body: format!("## Body for issue {number}"),
                comments: vec![IssueComment {
                    author: None,
                    body: format!("Comment for issue {number}"),
                    created_at: None,
                }],
            })
        }

        async fn list_comments(
            &self,
            _repo: &Repository,
            _number: u64,
        ) -> Result<Vec<IssueComment>> {
            unreachable!("not used by these tests")
        }

        async fn list_labels(&self, _repo: &Repository) -> Result<Vec<Label>> {
            Ok(self.labels.lock().unwrap().clone())
        }

        async fn create_issue(
            &self,
            _repo: &Repository,
            _title: &str,
            body: &str,
            labels: &[String],
        ) -> Result<IssueSummary> {
            *self.created_body.lock().unwrap() = Some(body.to_string());
            *self.created_labels.lock().unwrap() = labels.to_vec();
            let created = issue(2, "New task", IssueState::Open, 0);
            *self.issues.lock().unwrap() =
                vec![issue(1, "Fix redraw", IssueState::Open, 1), created.clone()];
            Ok(created)
        }

        async fn add_comment(
            &self,
            _repo: &Repository,
            _number: u64,
            body: &str,
        ) -> Result<IssueComment> {
            *self.commented_body.lock().unwrap() = Some(body.to_string());
            *self.issues.lock().unwrap() = vec![issue(1, "Fix redraw", IssueState::Open, 2)];
            Ok(IssueComment {
                author: None,
                body: "done".to_string(),
                created_at: None,
            })
        }

        async fn set_issue_state(
            &self,
            _repo: &Repository,
            number: u64,
            state: IssueState,
        ) -> Result<IssueSummary> {
            self.state_updates
                .lock()
                .unwrap()
                .push((number, state.clone()));
            let mut issues = self.issues.lock().unwrap();
            let issue = issues
                .iter_mut()
                .find(|issue| issue.number == number)
                .expect("test issue should exist");
            issue.state = state;
            Ok(issue.clone())
        }
    }

    #[tokio::test]
    async fn submit_comment_refreshes_issue_state_and_preserves_action_status() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        refresh(&mut app, &backend).await;
        app.input = "done".to_string();
        app.mode = UiMode::CommentComposer;

        submit_comment(&mut app, &backend).await;

        assert_eq!(*backend.list_calls.lock().unwrap(), 2);
        assert_eq!(
            backend.commented_body.lock().unwrap().as_deref(),
            Some("done")
        );
        assert_eq!(app.selected_issue().unwrap().comment_count, 2);
        assert_eq!(app.status, "Commented on issue #1");
        assert_eq!(app.mode, UiMode::Success);
        assert_eq!(app.flash, Some(FlashKind::Success));
    }

    #[tokio::test]
    async fn submit_new_issue_refreshes_and_selects_created_issue() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        refresh(&mut app, &backend).await;
        app.start_new_issue();
        app.input = "New task".to_string();
        app.body_input = "add docs".to_string();
        app.new_issue_labels = vec!["docs".to_string()];

        submit_new_issue(&mut app, &backend, true).await;

        assert_eq!(*backend.list_calls.lock().unwrap(), 2);
        assert_eq!(
            backend.created_body.lock().unwrap().as_deref(),
            Some("add docs")
        );
        assert_eq!(*backend.created_labels.lock().unwrap(), vec!["docs"]);
        assert_eq!(app.selected_issue().unwrap().number, 2);
        assert_eq!(app.status, "Created issue #2");
        assert_eq!(app.mode, UiMode::Success);
        assert_eq!(app.flash, Some(FlashKind::Success));
    }

    #[tokio::test]
    async fn success_confirmation_dismisses_to_browsing() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.mode = UiMode::Success;
        app.set_status("Commented on issue #1");

        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.mode, UiMode::Browsing);
        assert_eq!(app.status, "Commented on issue #1");
    }

    #[test]
    fn predicts_loading_preview_for_network_actions() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);

        assert_eq!(
            loading_preview(&app, KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
            Some((PendingAction::Refresh, "Refreshing issues".to_string()))
        );

        app.mode = UiMode::CommentComposer;
        app.input = "done".to_string();
        assert_eq!(
            loading_preview(&app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            loading_preview(
                &app,
                KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)
            ),
            Some((
                PendingAction::AddComment,
                "Adding comment to #1".to_string()
            ))
        );

        app.mode = UiMode::CloseComment;
        app.input = "closing notes".to_string();
        assert_eq!(
            loading_preview(
                &app,
                KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)
            ),
            Some((PendingAction::CloseIssue, "Closing issue #1".to_string()))
        );

        app.mode = UiMode::NewIssue;
        app.input = "New task".to_string();
        app.new_issue_field = NewIssueField::Labels;
        app.label_input = "bug".to_string();
        assert_eq!(
            loading_preview(&app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            None
        );

        app.label_input.clear();
        app.new_issue_field = NewIssueField::Body;
        assert_eq!(
            loading_preview(&app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            None
        );

        assert_eq!(
            loading_preview(
                &app,
                KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)
            ),
            Some((PendingAction::CreateIssue, "Creating issue".to_string()))
        );
    }

    #[tokio::test]
    async fn refresh_loads_selected_issue_detail() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());

        refresh(&mut app, &backend).await;

        assert_eq!(*backend.detail_calls.lock().unwrap(), 1);
        assert_eq!(
            app.selected_detail.as_ref().unwrap().body,
            "## Body for issue 1"
        );
    }

    #[tokio::test]
    async fn auto_refresh_reports_new_issues_and_preserves_selection() {
        let backend = backend_with_issues(vec![
            issue(1, "Fix redraw", IssueState::Open, 1),
            issue(2, "Add tree", IssueState::Open, 0),
        ]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        refresh(&mut app, &backend).await;
        app.select_issue_number(1);

        *backend.issues.lock().unwrap() = vec![
            issue(3, "Handle webhook", IssueState::Open, 0),
            issue(1, "Fix redraw", IssueState::Open, 1),
            issue(2, "Add tree", IssueState::Open, 0),
        ];

        let outcome = auto_refresh(&mut app, &backend).await;

        assert_eq!(outcome.new_issue_numbers, vec![3]);
        assert!(outcome.has_new_issues());
        assert_eq!(app.selected_issue().unwrap().number, 1);
        assert_eq!(app.status, "New issue #3: Handle webhook");
        assert_eq!(app.flash, Some(FlashKind::Success));
        assert_eq!(app.mode, UiMode::Browsing);
    }

    #[tokio::test]
    async fn auto_refresh_keeps_quiet_status_when_no_new_issues_arrive() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        refresh(&mut app, &backend).await;

        let outcome = auto_refresh(&mut app, &backend).await;

        assert_eq!(outcome, AutoRefreshOutcome::default());
        assert_eq!(app.status, "Auto-refreshed; no new issues");
        assert_eq!(app.flash, Some(FlashKind::Refresh));
        assert_eq!(app.mode, UiMode::Browsing);
    }

    #[test]
    fn auto_refresh_only_runs_while_browsing() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        assert!(can_auto_refresh(&app));

        app.mode = UiMode::CommentComposer;
        assert!(!can_auto_refresh(&app));

        app.mode = UiMode::NewIssue;
        assert!(!can_auto_refresh(&app));
    }

    #[test]
    fn notification_sound_uses_platform_sound_when_available() {
        let command = system_notification_sound_command();

        #[cfg(target_os = "macos")]
        assert_eq!(
            command,
            Some(("afplay", &["/System/Library/Sounds/Glass.aiff"][..]))
        );

        #[cfg(not(target_os = "macos"))]
        assert_eq!(command, None);
    }

    #[tokio::test]
    async fn navigating_loads_new_selected_issue_detail() {
        let backend = backend_with_issues(vec![
            issue(1, "Fix redraw", IssueState::Open, 1),
            issue(2, "Add tree", IssueState::Open, 0),
        ]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        refresh(&mut app, &backend).await;

        handle_browsing_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(*backend.detail_calls.lock().unwrap(), 2);
        assert_eq!(app.selected_detail.as_ref().unwrap().summary.number, 2);
    }

    #[tokio::test]
    async fn enter_toggles_comment_tree() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        app.set_selected_detail(IssueDetail {
            summary: issue(1, "Fix redraw", IssueState::Open, 1),
            body: "Body".to_string(),
            comments: Vec::new(),
        });

        handle_browsing_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert!(!app.comments_expanded);
    }

    #[tokio::test]
    async fn composer_ctrl_enter_inserts_markdown_newline() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.mode = UiMode::CommentComposer;
        app.input = "**first**".to_string();

        handle_comment_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
        )
        .await;

        assert_eq!(app.input, "**first**\n");
        assert_eq!(app.mode, UiMode::CommentComposer);
    }

    #[tokio::test]
    async fn comment_enter_inserts_newline_and_ctrl_s_submits() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        refresh(&mut app, &backend).await;
        app.mode = UiMode::CommentComposer;
        app.input = "line one".to_string();

        handle_comment_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;
        handle_comment_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.input, "line one\nl");
        assert_eq!(backend.commented_body.lock().unwrap().as_deref(), None);

        handle_comment_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        )
        .await;

        assert_eq!(
            backend.commented_body.lock().unwrap().as_deref(),
            Some("line one\nl")
        );
        assert_eq!(app.mode, UiMode::Success);
    }

    #[tokio::test]
    async fn closing_open_issue_requires_comment_and_refreshes_state() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        refresh(&mut app, &backend).await;

        handle_browsing_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.mode, UiMode::CloseComment);
        assert_eq!(
            app.status,
            "Closing requires a comment, Enter adds lines, Ctrl+S closes"
        );

        handle_close_comment_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        )
        .await;

        assert_eq!(app.mode, UiMode::CloseComment);
        assert_eq!(app.status, "Close comment cannot be empty");
        assert!(backend.state_updates.lock().unwrap().is_empty());

        app.input = "Closing after verifying the fix".to_string();
        handle_close_comment_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        )
        .await;

        assert_eq!(
            backend.commented_body.lock().unwrap().as_deref(),
            Some("Closing after verifying the fix")
        );
        assert_eq!(
            *backend.state_updates.lock().unwrap(),
            vec![(1, IssueState::Closed)]
        );
        assert_eq!(app.selected_issue().unwrap().state, IssueState::Closed);
        assert_eq!(app.status, "Closed issue #1 with comment");
        assert_eq!(app.mode, UiMode::Success);
    }

    #[tokio::test]
    async fn reopening_closed_issue_keeps_confirmation_flow() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Closed, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        refresh(&mut app, &backend).await;

        handle_browsing_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.mode, UiMode::ConfirmClose);
    }

    #[tokio::test]
    async fn opening_new_issue_loads_repo_labels() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());

        open_new_issue(&mut app, &backend).await;

        assert_eq!(app.mode, UiMode::NewIssue);
        assert_eq!(
            app.repo_labels
                .iter()
                .map(|label| label.name.as_str())
                .collect::<Vec<_>>(),
            vec!["bug", "docs"]
        );
    }

    #[tokio::test]
    async fn new_issue_form_keeps_title_body_and_label_fields_separate() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        open_new_issue(&mut app, &backend).await;

        handle_new_issue_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('A'), KeyModifiers::NONE),
        )
        .await;
        handle_new_issue_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        )
        .await;
        handle_new_issue_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('*'), KeyModifiers::NONE),
        )
        .await;
        handle_new_issue_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;
        handle_new_issue_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
        )
        .await;
        handle_new_issue_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        )
        .await;
        app.label_input = "do".to_string();

        handle_new_issue_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.input, "A");
        assert_eq!(app.body_input, "*\nb");
        assert_eq!(app.new_issue_labels, vec!["docs"]);
    }

    #[tokio::test]
    async fn body_enter_inserts_newline_and_ctrl_s_submits_new_issue() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        open_new_issue(&mut app, &backend).await;
        app.input = "New task".to_string();
        app.new_issue_field = NewIssueField::Body;
        app.body_input = "line one".to_string();

        handle_new_issue_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;
        handle_new_issue_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.body_input, "line one\nl");
        assert_eq!(*backend.list_calls.lock().unwrap(), 0);

        handle_new_issue_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        )
        .await;

        assert_eq!(
            backend.created_body.lock().unwrap().as_deref(),
            Some("line one\nl")
        );
        assert_eq!(app.mode, UiMode::Success);
    }

    #[tokio::test]
    async fn ctrl_s_accepts_pending_label_text_before_creating_issue() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        open_new_issue(&mut app, &backend).await;
        app.input = "New task".to_string();
        app.new_issue_field = NewIssueField::Labels;
        app.label_input = "do".to_string();

        handle_new_issue_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        )
        .await;

        assert_eq!(*backend.created_labels.lock().unwrap(), vec!["docs"]);
        assert_eq!(app.mode, UiMode::Success);
    }
}
