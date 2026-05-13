use std::{
    collections::{BTreeSet, HashMap},
    io::{self, Write},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use color_eyre::eyre::Result;
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::DefaultTerminal;

use crate::{
    app::{
        App, AssigneeChoice, AssigneeFilter, FlashKind, IssueEditField, IssueSort,
        IssueStateFilter, NewIssueField, PendingAction, TextCursorMove, UiMode,
    },
    domain::{IssueComment, IssueState, IssueSummary},
    github::IssueBackend,
    ui::{self, SkunkworkEffects},
};

const AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
#[cfg(target_os = "macos")]
const MACOS_NOTIFICATION_SOUND: &str = "/System/Library/Sounds/Glass.aiff";

pub async fn run<B: IssueBackend>(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    backend: &B,
) -> Result<()> {
    let mut effects = SkunkworkEffects::default();
    app.begin_action(PendingAction::Refresh, "Starting skunkwork");
    effects.trigger_startup_loading();
    draw_app(terminal, app, &mut effects, Duration::from_millis(120))?;
    if let Ok(login) = backend.current_login().await {
        app.set_viewer_login(login);
    }
    refresh(app, backend).await;

    let mut last_frame = Instant::now();
    let mut next_auto_refresh = Instant::now() + AUTO_REFRESH_INTERVAL;
    while !app.should_quit {
        let elapsed = last_frame.elapsed();
        last_frame = Instant::now();
        ui::trigger_flash_effect(app, &mut effects);

        draw_app(terminal, app, &mut effects, elapsed)?;
        app.advance_new_issue_animation();
        app.advance_activity_indicator();

        if can_auto_refresh(app) && Instant::now() >= next_auto_refresh {
            if auto_refresh(app, backend).await.has_notifications() {
                play_new_issue_notification();
            }
            next_auto_refresh = Instant::now() + AUTO_REFRESH_INTERVAL;
        } else if !can_auto_refresh(app) {
            next_auto_refresh = Instant::now() + AUTO_REFRESH_INTERVAL;
        }

        if event::poll(Duration::from_millis(33))? {
            match event::read()? {
                Event::Key(key) => {
                    if let Some((action, status)) = loading_preview(app, key) {
                        let mut preview = app.clone();
                        preview.begin_action(action, status);
                        ui::trigger_flash_effect(&mut preview, &mut effects);
                        draw_app(terminal, &preview, &mut effects, last_frame.elapsed())?;
                    }
                    handle_key(app, backend, key).await;
                }
                Event::Mouse(mouse) => {
                    handle_mouse(app, backend, mouse, terminal.size()?.into()).await;
                }
                _ => {}
            }
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
    mention_issue_numbers: Vec<u64>,
}

impl AutoRefreshOutcome {
    fn has_new_issues(&self) -> bool {
        !self.new_issue_numbers.is_empty()
    }

    fn has_notifications(&self) -> bool {
        self.has_new_issues() || !self.mention_issue_numbers.is_empty()
    }
}

fn draw_app(
    terminal: &mut DefaultTerminal,
    app: &App,
    effects: &mut SkunkworkEffects,
    elapsed: Duration,
) -> Result<()> {
    terminal.draw(|frame| {
        let area = frame.area();
        ui::render(app, area, frame.buffer_mut());
        let effect_area = if app.status == "Starting skunkwork" {
            area
        } else {
            ui::effect_area(app, area)
        };
        effects.process(elapsed, frame.buffer_mut(), effect_area);
    })?;
    Ok(())
}

fn loading_preview(app: &App, key: KeyEvent) -> Option<(PendingAction, String)> {
    match app.mode {
        UiMode::Browsing => match key.code {
            KeyCode::Char('n') => Some((
                PendingAction::LoadLabels,
                "Loading repository labels".to_string(),
            )),
            _ => None,
        },
        UiMode::Command if key.code == KeyCode::Enter => {
            let command = normalized_command(&app.input);
            if is_refresh_command(&command) || is_filter_state_command(&command) {
                Some((PendingAction::Refresh, "Refreshing issues".to_string()))
            } else if is_filter_assignee_command(&command) {
                Some((
                    PendingAction::LoadCollaborators,
                    "Loading collaborators".to_string(),
                ))
            } else {
                match command.as_str() {
                    "n" | "new" | "new issue" => Some((
                        PendingAction::LoadLabels,
                        "Loading repository labels".to_string(),
                    )),
                    "assign" | "assignees" => app.selected_issue().map(|issue| {
                        (
                            PendingAction::LoadCollaborators,
                            format!("Loading collaborators for #{}", issue.number),
                        )
                    }),
                    "labels" | "label" => app.selected_issue().map(|issue| {
                        (
                            PendingAction::LoadLabels,
                            format!("Loading labels for #{}", issue.number),
                        )
                    }),
                    _ => None,
                }
            }
        }
        UiMode::Search if key.code == KeyCode::Enter => {
            Some((PendingAction::Refresh, "Refreshing issues".to_string()))
        }
        UiMode::CommentComposer | UiMode::CloseComment
            if is_submit_key(key) && !app.input.trim().is_empty() =>
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
        UiMode::IssueEditor if is_submit_key(key) && !app.input.trim().is_empty() => {
            app.selected_issue().map(|issue| {
                (
                    PendingAction::UpdateIssue,
                    format!("Updating issue #{}", issue.number),
                )
            })
        }
        UiMode::AssigneeFilter if key.code == KeyCode::Enter => {
            Some((PendingAction::Refresh, "Refreshing issues".to_string()))
        }
        UiMode::AssigneeEditor if key.code == KeyCode::Enter => app.selected_issue().map(|issue| {
            (
                PendingAction::UpdateAssignees,
                format!("Updating issue #{}", issue.number),
            )
        }),
        UiMode::IssueLabelEditor if is_submit_key(key) => app.selected_issue().map(|issue| {
            (
                PendingAction::UpdateLabels,
                format!("Updating labels for #{}", issue.number),
            )
        }),
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

fn normalized_command(input: &str) -> String {
    input.trim().trim_start_matches(':').trim().to_lowercase()
}

fn is_refresh_command(command: &str) -> bool {
    matches!(command, "r" | "refresh" | "reload")
}

fn is_filter_state_command(command: &str) -> bool {
    matches!(
        command,
        "f" | "fs" | "f state" | "filter" | "filter state" | "state"
    )
}

fn is_filter_assignee_command(command: &str) -> bool {
    matches!(
        command,
        "fa" | "f assignee" | "filter assignee" | "assignee filter"
    )
}

fn is_search_prompt_command(command: &str) -> bool {
    matches!(command, "s" | "search" | "/")
}

fn search_query_command(command: &str) -> Option<String> {
    command
        .strip_prefix("search ")
        .or_else(|| command.strip_prefix("s "))
        .map(str::trim)
        .map(ToOwned::to_owned)
}

fn pending_state_action(state: IssueState) -> PendingAction {
    match state {
        IssueState::Open => PendingAction::CloseIssue,
        IssueState::Closed => PendingAction::ReopenIssue,
    }
}

fn should_submit_new_issue(app: &App, key: KeyEvent) -> bool {
    if is_submit_key(key) {
        return true;
    }

    key.code == KeyCode::Enter
        && app.new_issue_field == NewIssueField::Labels
        && app.label_input.trim().is_empty()
}

fn is_submit_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn cursor_movement(key: KeyEvent) -> Option<TextCursorMove> {
    match key.code {
        KeyCode::Left => Some(TextCursorMove::Left),
        KeyCode::Right => Some(TextCursorMove::Right),
        KeyCode::Home => Some(TextCursorMove::Home),
        KeyCode::End => Some(TextCursorMove::End),
        KeyCode::Up => Some(TextCursorMove::Up),
        KeyCode::Down => Some(TextCursorMove::Down),
        _ => None,
    }
}

async fn handle_key<B: IssueBackend>(app: &mut App, backend: &B, key: KeyEvent) {
    match app.mode {
        UiMode::Browsing => handle_browsing_key(app, backend, key).await,
        UiMode::Command => handle_command_key(app, backend, key).await,
        UiMode::Search => handle_search_key(app, backend, key).await,
        UiMode::CommentComposer => handle_comment_key(app, backend, key).await,
        UiMode::CloseComment => handle_close_comment_key(app, backend, key).await,
        UiMode::NewIssue => handle_new_issue_key(app, backend, key).await,
        UiMode::IssueEditor => handle_issue_editor_key(app, backend, key).await,
        UiMode::AssigneeFilter | UiMode::AssigneeEditor => {
            handle_assignee_picker_key(app, backend, key).await;
        }
        UiMode::IssueLabelEditor => handle_issue_label_key(app, backend, key).await,
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
        KeyCode::PageDown => app.scroll_detail_page_down(),
        KeyCode::PageUp => app.scroll_detail_page_up(),
        KeyCode::Enter if app.selected_detail.is_some() => app.toggle_comments(),
        KeyCode::Char(':') => open_command_prompt(app),
        KeyCode::Char('n') => open_new_issue(app, backend).await,
        KeyCode::Char('x') if app.selected_issue().is_some() => {
            match app.selected_issue().map(|issue| issue.state.clone()) {
                Some(IssueState::Open) => open_close_comment(app, backend).await,
                Some(IssueState::Closed) => {
                    app.mode = UiMode::ConfirmClose;
                }
                None => {}
            }
        }
        _ => {}
    }
}

async fn handle_mouse<B: IssueBackend>(
    app: &mut App,
    backend: &B,
    mouse: MouseEvent,
    area: ratatui::layout::Rect,
) {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(target) = ui::mouse_target(app, area, mouse.column, mouse.row) {
                handle_mouse_target(app, backend, target).await;
            }
        }
        MouseEventKind::ScrollDown if app.mode == UiMode::Browsing => {
            if ui::mouse_target(app, area, mouse.column, mouse.row)
                == Some(ui::MouseTarget::DetailPanel)
            {
                app.scroll_detail_down();
                return;
            }
            let previous = app.selected_issue().map(|issue| issue.number);
            app.select_next();
            if app.selected_issue().map(|issue| issue.number) != previous {
                refresh_selected_detail(app, backend).await;
            }
        }
        MouseEventKind::ScrollUp if app.mode == UiMode::Browsing => {
            if ui::mouse_target(app, area, mouse.column, mouse.row)
                == Some(ui::MouseTarget::DetailPanel)
            {
                app.scroll_detail_up();
                return;
            }
            let previous = app.selected_issue().map(|issue| issue.number);
            app.select_previous();
            if app.selected_issue().map(|issue| issue.number) != previous {
                refresh_selected_detail(app, backend).await;
            }
        }
        _ => {}
    }
}

async fn handle_mouse_target<B: IssueBackend>(app: &mut App, backend: &B, target: ui::MouseTarget) {
    match target {
        ui::MouseTarget::IssueRow(index) if app.mode == UiMode::Browsing => {
            let previous = app.selected_issue().map(|issue| issue.number);
            app.select_issue_index(index);
            if app.selected_issue().map(|issue| issue.number) != previous {
                refresh_selected_detail(app, backend).await;
            }
        }
        ui::MouseTarget::DetailPanel if app.mode == UiMode::Browsing => app.toggle_comments(),
        ui::MouseTarget::NewIssueField(field) if app.mode == UiMode::NewIssue => {
            app.new_issue_field = field;
        }
        ui::MouseTarget::IssueEditField(field) if app.mode == UiMode::IssueEditor => {
            app.issue_edit_field = field;
        }
        ui::MouseTarget::PrimaryAction => match app.mode {
            UiMode::CommentComposer => submit_comment(app, backend).await,
            UiMode::CloseComment => close_issue_with_comment(app, backend).await,
            UiMode::NewIssue => submit_new_issue(app, backend, true).await,
            UiMode::IssueEditor => save_issue_edit(app, backend).await,
            UiMode::AssigneeFilter | UiMode::AssigneeEditor => {
                submit_assignee_picker(app, backend).await;
            }
            UiMode::IssueLabelEditor => save_issue_labels(app, backend).await,
            UiMode::ConfirmClose => toggle_issue_state(app, backend).await,
            UiMode::Success => app.mode = UiMode::Browsing,
            _ => {}
        },
        ui::MouseTarget::CancelAction => cancel_active_screen(app),
        ui::MouseTarget::PickerItem(index) => {
            app.picker_index = index;
            match app.mode {
                UiMode::AssigneeFilter | UiMode::AssigneeEditor => {
                    if app.mode == UiMode::AssigneeFilter {
                        apply_selected_assignee_filter(app, backend).await;
                    } else {
                        toggle_selected_assignee(app);
                    }
                }
                UiMode::IssueLabelEditor => {
                    if let Some(label) = app.issue_label_choices().get(index).cloned() {
                        app.toggle_editing_issue_label(&label);
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

async fn open_comment_composer<B: IssueBackend>(app: &mut App, backend: &B) {
    app.clear_input();
    let mention_status = load_collaborators_for_mentions(app, backend).await;
    app.mode = UiMode::CommentComposer;
    app.set_status(mention_status.unwrap_or_else(|| {
        "Write a comment, Enter adds lines, Tab completes @mentions, Ctrl+S submits".to_string()
    }));
}

fn open_command_prompt(app: &mut App) {
    app.clear_input();
    app.mode = UiMode::Command;
    app.set_status("Type a command, for example :refresh or :filter state");
}

async fn open_close_comment<B: IssueBackend>(app: &mut App, backend: &B) {
    app.clear_input();
    let mention_status = load_collaborators_for_mentions(app, backend).await;
    app.mode = UiMode::CloseComment;
    app.set_status(mention_status.unwrap_or_else(|| {
        "Closing requires a comment, Tab completes @mentions, Ctrl+S closes".to_string()
    }));
}

async fn load_collaborators_for_mentions<B: IssueBackend>(
    app: &mut App,
    backend: &B,
) -> Option<String> {
    if !app.repo_collaborators.is_empty() {
        return None;
    }

    app.begin_action(PendingAction::LoadCollaborators, "Loading collaborators");
    match backend.list_collaborators(&app.repo).await {
        Ok(collaborators) => {
            app.finish_action();
            app.set_repo_collaborators(collaborators);
            None
        }
        Err(err) => {
            app.finish_action();
            app.flash = Some(FlashKind::Error);
            Some(format!(
                "Mention suggestions unavailable: {err:#}; writing still works"
            ))
        }
    }
}

fn cancel_active_screen(app: &mut App) {
    match app.mode {
        UiMode::CommentComposer | UiMode::CloseComment => {
            app.mode = UiMode::Browsing;
            app.clear_input();
        }
        UiMode::NewIssue => {
            app.mode = UiMode::Browsing;
            app.clear_input();
            app.clear_body_input();
            app.clear_label_input();
            app.new_issue_labels.clear();
        }
        UiMode::IssueEditor => {
            app.mode = UiMode::Browsing;
            app.clear_input();
            app.clear_body_input();
        }
        UiMode::AssigneeFilter | UiMode::AssigneeEditor | UiMode::IssueLabelEditor => {
            app.mode = UiMode::Browsing;
            app.clear_input();
            app.editing_assignees.clear();
            app.editing_issue_labels.clear();
        }
        UiMode::Success | UiMode::Error | UiMode::ConfirmClose => {
            app.mode = UiMode::Browsing;
        }
        UiMode::Command | UiMode::Search => {
            app.mode = UiMode::Browsing;
            app.clear_input();
        }
        _ => {}
    }
}

async fn handle_command_key<B: IssueBackend>(app: &mut App, backend: &B, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => cancel_active_screen(app),
        KeyCode::Enter => run_command(app, backend).await,
        KeyCode::Tab => complete_command(app),
        _ if cursor_movement(key).is_some() => {
            app.move_input_cursor(cursor_movement(key).expect("cursor movement checked"));
        }
        KeyCode::Backspace => {
            app.backspace_input();
        }
        KeyCode::Delete => {
            app.delete_input();
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.insert_input_char(c);
        }
        _ => {}
    }
}

async fn run_command<B: IssueBackend>(app: &mut App, backend: &B) {
    let command = normalized_command(&app.input);
    app.clear_input();

    match command.as_str() {
        "" => {
            app.mode = UiMode::Browsing;
        }
        "q" | "quit" => app.should_quit = true,
        command if is_refresh_command(command) => {
            app.mode = UiMode::Browsing;
            refresh(app, backend).await;
        }
        command if is_filter_state_command(command) => {
            app.cycle_state_filter();
            app.mode = UiMode::Browsing;
            refresh(app, backend).await;
        }
        command if is_filter_assignee_command(command) => {
            open_assignee_filter(app, backend).await;
        }
        command if is_search_prompt_command(command) => {
            app.set_input_text(app.filters.query.clone());
            app.mode = UiMode::Search;
            app.set_status("Search issue titles");
        }
        command if search_query_command(command).is_some() => {
            let query = search_query_command(command).expect("search query checked");
            app.set_query(query);
            app.mode = UiMode::Browsing;
            refresh(app, backend).await;
        }
        "me" | "mine" => {
            app.filters.assignee = AssigneeFilter::Me;
            app.mode = UiMode::Browsing;
            refresh(app, backend).await;
        }
        "unassigned" | "none" => {
            app.filters.assignee = AssigneeFilter::None;
            app.mode = UiMode::Browsing;
            refresh(app, backend).await;
        }
        "any" | "all assignees" => {
            app.filters.assignee = AssigneeFilter::Any;
            app.mode = UiMode::Browsing;
            refresh(app, backend).await;
        }
        "sort" => {
            app.cycle_sort();
            app.mode = UiMode::Browsing;
            refresh(app, backend).await;
        }
        command if sort_command(command).is_some() => {
            app.filters.sort = sort_command(command).expect("sort command checked");
            app.mode = UiMode::Browsing;
            refresh(app, backend).await;
        }
        command if label_filter_command(command).is_some() => {
            let label = label_filter_command(command).expect("label command checked");
            if label == "any" || label == "none" || label == "clear" {
                app.filters.labels.clear();
            } else {
                app.filters.labels = vec![label];
            }
            app.mode = UiMode::Browsing;
            refresh(app, backend).await;
        }
        "all" | "clear" | "clear filters" | "filters clear" | "reset filters" => {
            app.clear_filters();
            app.mode = UiMode::Browsing;
            refresh(app, backend).await;
        }
        "n" | "new" | "new issue" => open_new_issue(app, backend).await,
        "c" | "comment" if app.selected_issue().is_some() => {
            open_comment_composer(app, backend).await;
        }
        "mention" | "mentions" | "ping" => jump_to_first_mention(app, backend).await,
        "e" | "edit" if app.selected_issue().is_some() => open_issue_editor(app, backend).await,
        "a" | "assign" | "assignees" if app.selected_issue().is_some() => {
            open_assignee_editor(app, backend).await;
        }
        "l" | "label" | "labels" if app.selected_issue().is_some() => {
            open_issue_label_editor(app, backend).await;
        }
        "x" | "close" if app.selected_issue().is_some() => {
            match app.selected_issue().map(|issue| issue.state.clone()) {
                Some(IssueState::Open) => open_close_comment(app, backend).await,
                Some(IssueState::Closed) => {
                    app.mode = UiMode::ConfirmClose;
                }
                None => {}
            }
        }
        _ => {
            app.mode = UiMode::Browsing;
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("Unknown command: {command}"));
        }
    }
}

fn complete_command(app: &mut App) {
    if let Some(suggestion) = command_suggestions(&app.input).into_iter().next() {
        app.set_input_text(suggestion);
    }
}

async fn jump_to_first_mention<B: IssueBackend>(app: &mut App, backend: &B) {
    let Some(number) = app.first_mentioned_issue_number() else {
        app.mode = UiMode::Browsing;
        app.flash = Some(FlashKind::Error);
        app.set_status("No mention highlights to jump to");
        return;
    };

    app.select_issue_number(number);
    refresh_selected_detail(app, backend).await;
    app.mode = UiMode::Browsing;
    app.set_status(format!("Jumped to mention on issue #{number}"));
}

fn command_suggestions(input: &str) -> Vec<String> {
    let command = normalized_command(input);
    let commands = [
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
        "all",
        "clear",
        "ping",
        "mentions",
        "refresh",
        "quit",
    ];

    commands
        .into_iter()
        .filter(|candidate| command.is_empty() || candidate.starts_with(&command))
        .take(5)
        .map(ToOwned::to_owned)
        .collect()
}

fn sort_command(command: &str) -> Option<IssueSort> {
    let value = command.strip_prefix("sort ")?;
    match value.trim() {
        "updated" | "update" => Some(IssueSort::Updated),
        "created" | "create" => Some(IssueSort::Created),
        "comments" | "comment" => Some(IssueSort::Comments),
        "assignee" | "assignees" | "who" => Some(IssueSort::Assignee),
        _ => None,
    }
}

fn label_filter_command(command: &str) -> Option<String> {
    command
        .strip_prefix("label ")
        .or_else(|| command.strip_prefix("l "))
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(ToOwned::to_owned)
}

async fn handle_search_key<B: IssueBackend>(app: &mut App, backend: &B, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.mode = UiMode::Browsing,
        KeyCode::Enter => {
            app.set_query(app.input.trim().to_string());
            app.mode = UiMode::Browsing;
            refresh(app, backend).await;
        }
        _ if cursor_movement(key).is_some() => {
            app.move_input_cursor(cursor_movement(key).expect("cursor movement checked"));
        }
        KeyCode::Backspace => {
            app.backspace_input();
        }
        KeyCode::Delete => {
            app.delete_input();
        }
        KeyCode::Char(c) => app.insert_input_char(c),
        _ => {}
    }
}

async fn handle_comment_key<B: IssueBackend>(app: &mut App, backend: &B, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.mode = UiMode::Browsing;
            app.clear_input();
        }
        _ if cursor_movement(key).is_some() => {
            app.move_input_cursor(cursor_movement(key).expect("cursor movement checked"));
        }
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.insert_input_newline()
        }
        KeyCode::Enter => app.insert_input_newline(),
        KeyCode::Tab => {
            app.complete_input_mention();
        }
        KeyCode::Char(_) if is_submit_key(key) => {
            submit_comment(app, backend).await;
        }
        KeyCode::Backspace => {
            app.backspace_input();
        }
        KeyCode::Delete => {
            app.delete_input();
        }
        KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.insert_input_newline();
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.insert_input_char(c);
        }
        _ => {}
    }
}

async fn handle_close_comment_key<B: IssueBackend>(app: &mut App, backend: &B, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.mode = UiMode::Browsing;
            app.clear_input();
        }
        _ if cursor_movement(key).is_some() => {
            app.move_input_cursor(cursor_movement(key).expect("cursor movement checked"));
        }
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.insert_input_newline()
        }
        KeyCode::Enter => app.insert_input_newline(),
        KeyCode::Tab => {
            app.complete_input_mention();
        }
        KeyCode::Char(_) if is_submit_key(key) => {
            close_issue_with_comment(app, backend).await;
        }
        KeyCode::Backspace => {
            app.backspace_input();
        }
        KeyCode::Delete => {
            app.delete_input();
        }
        KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.insert_input_newline();
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.insert_input_char(c);
        }
        _ => {}
    }
}

async fn handle_new_issue_key<B: IssueBackend>(app: &mut App, backend: &B, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.mode = UiMode::Browsing;
            app.clear_input();
            app.clear_body_input();
            app.clear_label_input();
            app.new_issue_labels.clear();
        }
        KeyCode::Tab
            if app.new_issue_field == NewIssueField::Body && app.complete_body_mention() => {}
        KeyCode::Tab => app.next_new_issue_field(),
        KeyCode::BackTab => app.previous_new_issue_field(),
        _ if cursor_movement(key).is_some() => {
            move_new_issue_field_cursor(
                app,
                cursor_movement(key).expect("cursor movement checked"),
            );
        }
        KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.apply_next_issue_template() {
                app.set_status("Applied issue template");
            } else {
                app.flash = Some(FlashKind::Error);
                app.set_status("No issue templates found");
            }
        }
        KeyCode::Char(_) if is_submit_key(key) => submit_new_issue(app, backend, true).await,
        KeyCode::Enter
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && app.new_issue_field == NewIssueField::Body =>
        {
            app.insert_body_newline();
        }
        KeyCode::Enter if app.new_issue_field == NewIssueField::Title => {
            app.new_issue_field = NewIssueField::Body;
        }
        KeyCode::Enter if app.new_issue_field == NewIssueField::Body => {
            app.insert_body_newline();
        }
        KeyCode::Enter => submit_new_issue(app, backend, false).await,
        KeyCode::Backspace => {
            backspace_new_issue_field(app);
        }
        KeyCode::Delete => {
            delete_new_issue_field(app);
        }
        KeyCode::Char('j')
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && app.new_issue_field == NewIssueField::Body =>
        {
            app.insert_body_newline();
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            push_new_issue_char(app, c);
        }
        _ => {}
    }
}

async fn handle_issue_editor_key<B: IssueBackend>(app: &mut App, backend: &B, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.mode = UiMode::Browsing;
            app.clear_input();
            app.clear_body_input();
        }
        KeyCode::Tab
            if app.issue_edit_field == IssueEditField::Body && app.complete_body_mention() => {}
        KeyCode::Tab | KeyCode::BackTab => app.next_issue_edit_field(),
        _ if cursor_movement(key).is_some() => {
            move_issue_editor_cursor(app, cursor_movement(key).expect("cursor movement checked"));
        }
        KeyCode::Char(_) if is_submit_key(key) => save_issue_edit(app, backend).await,
        KeyCode::Enter
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && app.issue_edit_field == IssueEditField::Body =>
        {
            app.insert_body_newline();
        }
        KeyCode::Enter if app.issue_edit_field == IssueEditField::Title => {
            app.issue_edit_field = IssueEditField::Body;
        }
        KeyCode::Enter if app.issue_edit_field == IssueEditField::Body => {
            app.insert_body_newline();
        }
        KeyCode::Backspace => match app.issue_edit_field {
            IssueEditField::Title => {
                app.backspace_input();
            }
            IssueEditField::Body => {
                app.backspace_body();
            }
        },
        KeyCode::Delete => match app.issue_edit_field {
            IssueEditField::Title => {
                app.delete_input();
            }
            IssueEditField::Body => {
                app.delete_body();
            }
        },
        KeyCode::Char('j')
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && app.issue_edit_field == IssueEditField::Body =>
        {
            app.insert_body_newline();
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            match app.issue_edit_field {
                IssueEditField::Title => app.insert_input_char(c),
                IssueEditField::Body => app.insert_body_char(c),
            }
        }
        _ => {}
    }
}

async fn handle_assignee_picker_key<B: IssueBackend>(app: &mut App, backend: &B, key: KeyEvent) {
    let choices = if app.mode == UiMode::AssigneeFilter {
        app.assignee_filter_choices()
    } else {
        app.assignee_assignment_choices()
    };

    match key.code {
        KeyCode::Esc => cancel_active_screen(app),
        KeyCode::Down | KeyCode::Char('j') => app.select_next_picker_item(choices.len()),
        KeyCode::Up | KeyCode::Char('k') => app.select_previous_picker_item(),
        _ if cursor_movement(key).is_some() => {
            app.move_input_cursor(cursor_movement(key).expect("cursor movement checked"));
        }
        KeyCode::Backspace => {
            app.backspace_input();
            app.picker_index = 0;
        }
        KeyCode::Delete => {
            app.delete_input();
            app.picker_index = 0;
        }
        KeyCode::Enter => {
            submit_assignee_picker(app, backend).await;
        }
        KeyCode::Char(' ') if app.mode == UiMode::AssigneeEditor => {
            toggle_selected_assignee(app);
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.insert_input_char(c);
            app.picker_index = 0;
        }
        _ => {}
    }
}

async fn submit_assignee_picker<B: IssueBackend>(app: &mut App, backend: &B) {
    match app.mode {
        UiMode::AssigneeFilter => apply_selected_assignee_filter(app, backend).await,
        UiMode::AssigneeEditor => save_assignee_assignment(app, backend).await,
        _ => {}
    }
}

async fn apply_selected_assignee_filter<B: IssueBackend>(app: &mut App, backend: &B) {
    let choices = app.assignee_filter_choices();

    if let Some(choice) = choices.get(app.picker_index).cloned() {
        apply_assignee_filter(app, backend, choice).await;
    }
}

fn toggle_selected_assignee(app: &mut App) {
    let choices = app.assignee_assignment_choices();
    if let Some(choice) = choices.get(app.picker_index).cloned() {
        match choice {
            AssigneeChoice::Unassigned => app.clear_editing_assignees(),
            AssigneeChoice::Me => {
                if let Some(login) = app.viewer_login.clone() {
                    app.toggle_editing_assignee(&login);
                } else {
                    app.flash = Some(FlashKind::Error);
                    app.set_status("Authenticated GitHub login is unavailable");
                }
            }
            AssigneeChoice::User(login) => app.toggle_editing_assignee(&login),
            AssigneeChoice::Any => {}
        }
    }
}

async fn handle_issue_label_key<B: IssueBackend>(app: &mut App, backend: &B, key: KeyEvent) {
    let choices = app.issue_label_choices();
    match key.code {
        KeyCode::Esc => cancel_active_screen(app),
        KeyCode::Down | KeyCode::Char('j') => app.select_next_picker_item(choices.len()),
        KeyCode::Up | KeyCode::Char('k') => app.select_previous_picker_item(),
        _ if cursor_movement(key).is_some() => {
            app.move_input_cursor(cursor_movement(key).expect("cursor movement checked"));
        }
        KeyCode::Backspace => {
            app.backspace_input();
            app.picker_index = 0;
        }
        KeyCode::Delete => {
            app.delete_input();
            app.picker_index = 0;
        }
        KeyCode::Enter => {
            if let Some(label) = choices.get(app.picker_index) {
                app.toggle_editing_issue_label(label);
            }
        }
        KeyCode::Char(_) if is_submit_key(key) => save_issue_labels(app, backend).await,
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.insert_input_char(c);
            app.picker_index = 0;
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
                    app.flash = Some(FlashKind::Refresh);
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
    let previous_issue_meta = app
        .issues
        .iter()
        .map(|issue| (issue.number, (issue.comment_count, issue.updated_at)))
        .collect::<HashMap<_, _>>();
    let selected_issue_number = app.selected_issue().map(|issue| issue.number);

    match backend.list_issues(&app.repo, &app.filters).await {
        Ok(issues) => {
            let new_issue_numbers = issues
                .iter()
                .filter(|issue| !previous_numbers.contains(&issue.number))
                .map(|issue| issue.number)
                .collect::<Vec<_>>();
            let new_issue_status = new_issue_notice(&issues, &new_issue_numbers);
            let mention_issue_numbers = if let Some(login) = app.viewer_login.clone() {
                mentioned_issues(backend, app, &issues, &previous_issue_meta, &login).await
            } else {
                Vec::new()
            };
            let mention_status = mention_notice(&issues, &mention_issue_numbers);

            app.set_issues(issues);
            if let Some(number) = selected_issue_number {
                app.select_issue_number(number);
            }
            app.highlight_new_issues(new_issue_numbers.clone());
            app.highlight_mentioned_issues(mention_issue_numbers.clone());

            match load_selected_detail(app, backend).await {
                Ok(()) => {
                    app.finish_action();
                    app.flash = Some(FlashKind::Refresh);
                    if let Some(status) = new_issue_status {
                        app.set_status(status);
                    } else if let Some(status) = mention_status {
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

            AutoRefreshOutcome {
                new_issue_numbers,
                mention_issue_numbers,
            }
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

async fn mentioned_issues<B: IssueBackend>(
    backend: &B,
    app: &App,
    issues: &[IssueSummary],
    previous_issue_meta: &HashMap<u64, (u64, Option<chrono::DateTime<chrono::Utc>>)>,
    login: &str,
) -> Vec<u64> {
    let mut mentioned = Vec::new();

    for issue in issues {
        let previous = previous_issue_meta.get(&issue.number).copied();

        if new_comments_mention_login(backend, app, issue, previous, login).await
            || issue_body_mentions_login(backend, app, issue, previous, login).await
        {
            mentioned.push(issue.number);
        }
    }

    mentioned
}

async fn new_comments_mention_login<B: IssueBackend>(
    backend: &B,
    app: &App,
    issue: &IssueSummary,
    previous: Option<(u64, Option<chrono::DateTime<chrono::Utc>>)>,
    login: &str,
) -> bool {
    let Some((previous_count, _)) = previous else {
        return false;
    };
    if issue.comment_count <= previous_count {
        return false;
    }

    let new_comment_count = (issue.comment_count - previous_count) as usize;
    let Ok(comments) = backend.list_comments(&app.repo, issue.number).await else {
        return false;
    };

    comments
        .iter()
        .rev()
        .take(new_comment_count)
        .any(|comment| comment_mentions_login(comment, login))
}

async fn issue_body_mentions_login<B: IssueBackend>(
    backend: &B,
    app: &App,
    issue: &IssueSummary,
    previous: Option<(u64, Option<chrono::DateTime<chrono::Utc>>)>,
    login: &str,
) -> bool {
    if !issue_body_is_new_or_updated(issue, previous) {
        return false;
    }

    let Ok(detail) = backend.get_issue(&app.repo, issue.number).await else {
        return false;
    };

    text_mentions_login(&detail.body, login)
}

fn issue_body_is_new_or_updated(
    issue: &IssueSummary,
    previous: Option<(u64, Option<chrono::DateTime<chrono::Utc>>)>,
) -> bool {
    let Some((previous_count, previous_updated_at)) = previous else {
        return true;
    };
    if issue.comment_count != previous_count {
        return false;
    }

    match (issue.updated_at, previous_updated_at) {
        (Some(current), Some(previous)) => current > previous,
        (Some(_), None) => true,
        _ => false,
    }
}

fn comment_mentions_login(comment: &IssueComment, login: &str) -> bool {
    text_mentions_login(&comment.body, login)
}

fn text_mentions_login(text: &str, login: &str) -> bool {
    let needle = format!("@{}", login.to_lowercase());
    let text = text.to_lowercase();

    text.match_indices(&needle).any(|(index, _)| {
        let after_index = index + needle.len();
        text[after_index..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '-')
    })
}

fn mention_notice(issues: &[IssueSummary], mention_issue_numbers: &[u64]) -> Option<String> {
    let first_number = mention_issue_numbers.first()?;
    let first_issue = issues.iter().find(|issue| issue.number == *first_number)?;

    if mention_issue_numbers.len() == 1 {
        Some(format!(
            "Mentioned on #{}: {}",
            first_issue.number, first_issue.title
        ))
    } else {
        Some(format!(
            "{} new mentions; latest #{}: {}",
            mention_issue_numbers.len(),
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
    optimistic_issue: Option<IssueSummary>,
    optimistic_comment: Option<IssueComment>,
) {
    match backend.list_issues(&app.repo, &app.filters).await {
        Ok(issues) => {
            app.set_issues(issues);
            if let Some(issue) = optimistic_issue
                && should_keep_optimistic_issue(app, &issue)
                && !app.issues.iter().any(|item| item.number == issue.number)
            {
                app.upsert_issue_at_top(issue);
            }
            if let Some(number) = selected_issue_number {
                app.select_issue_number(number);
            }
            app.mode = UiMode::Browsing;
            match load_selected_detail(app, backend).await {
                Ok(()) => {
                    if let (Some(number), Some(comment)) =
                        (selected_issue_number, optimistic_comment)
                    {
                        keep_optimistic_comment_visible(app, number, comment);
                    }
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

fn should_keep_optimistic_issue(app: &App, issue: &IssueSummary) -> bool {
    let state_matches = match app.filters.state {
        IssueStateFilter::Open => issue.state == IssueState::Open,
        IssueStateFilter::Closed => issue.state == IssueState::Closed,
        IssueStateFilter::All => true,
    };
    let labels_match = app.filters.labels.iter().all(|filter_label| {
        issue
            .labels
            .iter()
            .any(|issue_label| issue_label.name == *filter_label)
    });
    let query = app.filters.query.trim().to_lowercase();
    let query_matches = query.is_empty() || issue.title.to_lowercase().contains(&query);
    let assignee_matches = match &app.filters.assignee {
        AssigneeFilter::Any => true,
        AssigneeFilter::Me => app.viewer_login.as_ref().is_some_and(|login| {
            issue
                .assignees
                .iter()
                .any(|assignee| &assignee.login == login)
        }),
        AssigneeFilter::None => issue.assignees.is_empty(),
        AssigneeFilter::User(login) => issue
            .assignees
            .iter()
            .any(|assignee| &assignee.login == login),
    };

    state_matches && labels_match && query_matches && assignee_matches
}

fn keep_optimistic_comment_visible(app: &mut App, number: u64, comment: IssueComment) {
    let Some(detail) = app
        .selected_detail
        .as_mut()
        .filter(|detail| detail.summary.number == number)
    else {
        return;
    };

    if !detail.comments.iter().any(|item| item == &comment) {
        detail.comments.push(comment);
    }
    detail.summary.comment_count = detail
        .summary
        .comment_count
        .max(detail.comments.len() as u64);
    if let Some(issue) = app.issues.iter_mut().find(|issue| issue.number == number) {
        issue.comment_count = issue.comment_count.max(detail.summary.comment_count);
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
        Ok(comment) => {
            app.clear_input();
            refresh_after_action(
                app,
                backend,
                Some(number),
                format!("Commented on issue #{number}"),
                None,
                Some(comment),
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
                app.clear_input();
                refresh_after_action(
                    app,
                    backend,
                    Some(number),
                    format!("Closed issue #{number} with comment"),
                    None,
                    None,
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
            app.clear_input();
            app.clear_body_input();
            app.clear_label_input();
            app.new_issue_labels.clear();
            refresh_after_action(
                app,
                backend,
                Some(number),
                format!("Created issue #{number}"),
                Some(issue),
                None,
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

async fn save_issue_edit<B: IssueBackend>(app: &mut App, backend: &B) {
    let title = app.input.trim().to_string();
    let body = app.body_input.trim().to_string();
    let Some(issue) = app.selected_issue() else {
        app.mode = UiMode::Browsing;
        return;
    };
    let number = issue.number;

    if title.is_empty() {
        app.set_status("Issue title cannot be empty");
        app.flash = Some(FlashKind::Error);
        return;
    }

    app.begin_action(
        PendingAction::UpdateIssue,
        format!("Updating issue #{number}"),
    );
    match backend.update_issue(&app.repo, number, &title, &body).await {
        Ok(issue) => {
            app.clear_input();
            app.clear_body_input();
            refresh_after_action(
                app,
                backend,
                Some(number),
                format!("Updated issue #{number}"),
                Some(issue),
                None,
            )
            .await;
        }
        Err(err) => {
            app.finish_action();
            app.mode = UiMode::IssueEditor;
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("Issue update failed: {err:#}"));
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
            let template_status = match backend.list_issue_templates(&app.repo).await {
                Ok(templates) => {
                    let count = templates.len();
                    app.set_repo_issue_templates(templates);
                    if count == 0 {
                        "no templates".to_string()
                    } else {
                        format!("{count} templates, Ctrl+T applies")
                    }
                }
                Err(_) => {
                    app.set_repo_issue_templates(Vec::new());
                    "templates unavailable".to_string()
                }
            };
            let mention_status = match backend.list_collaborators(&app.repo).await {
                Ok(collaborators) => {
                    app.set_repo_collaborators(collaborators);
                    "Tab completes @mentions"
                }
                Err(_) => "mention suggestions unavailable",
            };
            app.set_status(format!(
                "New issue: Tab fields, Ctrl+S creates, {template_status}, {mention_status}"
            ));
        }
        Err(err) => {
            app.finish_action();
            app.mode = UiMode::NewIssue;
            app.set_repo_labels(Vec::new());
            app.set_repo_issue_templates(Vec::new());
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("Labels unavailable: {err:#}; creating still works"));
        }
    }
}

async fn open_issue_editor<B: IssueBackend>(app: &mut App, backend: &B) {
    let Some(issue) = app.selected_issue() else {
        app.mode = UiMode::Browsing;
        return;
    };
    let number = issue.number;

    app.begin_action(PendingAction::Refresh, format!("Loading issue #{number}"));
    match backend.get_issue(&app.repo, number).await {
        Ok(detail) => {
            let body = detail.body.clone();
            app.finish_action();
            app.set_selected_detail(detail);
            app.begin_issue_edit(body);
            let mention_status = match backend.list_collaborators(&app.repo).await {
                Ok(collaborators) => {
                    app.set_repo_collaborators(collaborators);
                    "Tab completes @mentions"
                }
                Err(_) => "mention suggestions unavailable",
            };
            app.set_status(format!(
                "Edit title/body, Tab fields, Ctrl+S saves, {mention_status}"
            ));
        }
        Err(err) => {
            app.finish_action();
            app.mode = UiMode::Browsing;
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("Issue edit unavailable: {err:#}"));
        }
    }
}

async fn open_assignee_filter<B: IssueBackend>(app: &mut App, backend: &B) {
    app.reset_picker();
    app.begin_action(PendingAction::LoadCollaborators, "Loading collaborators");
    match backend.list_collaborators(&app.repo).await {
        Ok(collaborators) => {
            app.finish_action();
            app.set_repo_collaborators(collaborators);
            app.mode = UiMode::AssigneeFilter;
            app.set_status("Choose an assignee filter");
        }
        Err(err) => {
            app.finish_action();
            app.flash = Some(FlashKind::Error);
            app.mode = UiMode::Browsing;
            app.set_status(format!("Collaborators unavailable: {err:#}"));
        }
    }
}

async fn open_assignee_editor<B: IssueBackend>(app: &mut App, backend: &B) {
    app.reset_picker();
    app.begin_action(PendingAction::LoadCollaborators, "Loading collaborators");
    match backend.list_collaborators(&app.repo).await {
        Ok(collaborators) => {
            app.finish_action();
            app.set_repo_collaborators(collaborators);
            app.begin_assignee_edit();
            app.set_status("Space toggles assignees, Enter saves");
        }
        Err(err) => {
            app.finish_action();
            app.flash = Some(FlashKind::Error);
            app.mode = UiMode::Browsing;
            app.set_status(format!("Collaborators unavailable: {err:#}"));
        }
    }
}

async fn open_issue_label_editor<B: IssueBackend>(app: &mut App, backend: &B) {
    app.begin_action(PendingAction::LoadLabels, "Loading repository labels");
    match backend.list_labels(&app.repo).await {
        Ok(labels) => {
            app.finish_action();
            app.set_repo_labels(labels);
            app.begin_issue_label_edit();
            app.set_status("Toggle labels with Enter, save with Ctrl+S");
        }
        Err(err) => {
            app.finish_action();
            app.flash = Some(FlashKind::Error);
            app.mode = UiMode::Browsing;
            app.set_status(format!("Labels unavailable: {err:#}"));
        }
    }
}

async fn apply_assignee_filter<B: IssueBackend>(
    app: &mut App,
    backend: &B,
    choice: AssigneeChoice,
) {
    app.filters.assignee = match choice {
        AssigneeChoice::Any => AssigneeFilter::Any,
        AssigneeChoice::Me => AssigneeFilter::Me,
        AssigneeChoice::Unassigned => AssigneeFilter::None,
        AssigneeChoice::User(login) => AssigneeFilter::User(login),
    };
    app.clear_input();
    app.mode = UiMode::Browsing;
    refresh(app, backend).await;
}

async fn save_assignee_assignment<B: IssueBackend>(app: &mut App, backend: &B) {
    let Some(issue) = app.selected_issue() else {
        app.mode = UiMode::Browsing;
        return;
    };
    let number = issue.number;
    let mut assignees = app.editing_assignees.clone();
    assignees.sort();
    assignees.dedup();

    app.begin_action(
        PendingAction::UpdateAssignees,
        format!("Updating issue #{number}"),
    );
    match backend
        .set_issue_assignees(&app.repo, number, &assignees)
        .await
    {
        Ok(_) => {
            refresh_after_action(
                app,
                backend,
                Some(number),
                format!("Updated assignees for issue #{number}"),
                None,
                None,
            )
            .await;
        }
        Err(err) => {
            app.finish_action();
            app.flash = Some(FlashKind::Error);
            app.mode = UiMode::Browsing;
            app.set_status(format!("Assignee update failed: {err:#}"));
        }
    }
}

async fn save_issue_labels<B: IssueBackend>(app: &mut App, backend: &B) {
    let Some(issue) = app.selected_issue() else {
        app.mode = UiMode::Browsing;
        return;
    };
    let number = issue.number;
    let labels = app.editing_issue_labels.clone();

    app.begin_action(
        PendingAction::UpdateLabels,
        format!("Updating labels for #{number}"),
    );
    match backend.set_issue_labels(&app.repo, number, &labels).await {
        Ok(_) => {
            refresh_after_action(
                app,
                backend,
                Some(number),
                format!("Updated labels for issue #{number}"),
                None,
                None,
            )
            .await;
        }
        Err(err) => {
            app.finish_action();
            app.flash = Some(FlashKind::Error);
            app.mode = UiMode::IssueLabelEditor;
            app.set_status(format!("Label update failed: {err:#}"));
        }
    }
}

fn push_new_issue_char(app: &mut App, c: char) {
    match app.new_issue_field {
        NewIssueField::Title => app.insert_input_char(c),
        NewIssueField::Body => app.insert_body_char(c),
        NewIssueField::Labels => app.insert_label_char(c),
    }
}

fn backspace_new_issue_field(app: &mut App) {
    match app.new_issue_field {
        NewIssueField::Title => {
            app.backspace_input();
        }
        NewIssueField::Body => {
            app.backspace_body();
        }
        NewIssueField::Labels => {
            if app.label_input.is_empty() {
                app.pop_new_issue_label();
            } else {
                app.backspace_label();
            }
        }
    }
}

fn delete_new_issue_field(app: &mut App) {
    match app.new_issue_field {
        NewIssueField::Title => app.delete_input(),
        NewIssueField::Body => app.delete_body(),
        NewIssueField::Labels => app.delete_label(),
    }
}

fn move_new_issue_field_cursor(app: &mut App, movement: TextCursorMove) {
    match app.new_issue_field {
        NewIssueField::Title => app.move_input_cursor(movement),
        NewIssueField::Body => app.move_body_cursor(movement),
        NewIssueField::Labels => app.move_label_cursor(movement),
    }
}

fn move_issue_editor_cursor(app: &mut App, movement: TextCursorMove) {
    match app.issue_edit_field {
        IssueEditField::Title => app.move_input_cursor(movement),
        IssueEditField::Body => app.move_body_cursor(movement),
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
                None,
                None,
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
    use std::{collections::HashMap, sync::Mutex};

    use crate::{
        app::IssueFilters,
        domain::{IssueComment, IssueDetail, IssueSummary, IssueTemplate, Label, User},
        repo::Repository,
    };

    #[derive(Default)]
    struct MockBackend {
        list_calls: Mutex<usize>,
        detail_calls: Mutex<usize>,
        created_body: Mutex<Option<String>>,
        created_labels: Mutex<Vec<String>>,
        commented_body: Mutex<Option<String>>,
        updated_issue: Mutex<Option<(u64, String, String)>>,
        state_updates: Mutex<Vec<(u64, IssueState)>>,
        assignee_updates: Mutex<Vec<(u64, Vec<String>)>>,
        label_updates: Mutex<Vec<(u64, Vec<String>)>>,
        issues: Mutex<Vec<IssueSummary>>,
        detail_bodies: Mutex<HashMap<u64, String>>,
        comments: Mutex<HashMap<u64, Vec<IssueComment>>>,
        labels: Mutex<Vec<Label>>,
        collaborators: Mutex<Vec<User>>,
        templates: Mutex<Vec<IssueTemplate>>,
    }

    fn issue(number: u64, title: &str, state: IssueState, comment_count: u64) -> IssueSummary {
        IssueSummary {
            number,
            title: title.to_string(),
            state,
            labels: Vec::new(),
            assignees: Vec::new(),
            author: None,
            created_at: None,
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
            updated_issue: Mutex::new(None),
            state_updates: Mutex::new(Vec::new()),
            assignee_updates: Mutex::new(Vec::new()),
            label_updates: Mutex::new(Vec::new()),
            issues: Mutex::new(issues),
            detail_bodies: Mutex::new(HashMap::new()),
            comments: Mutex::new(HashMap::new()),
            labels: Mutex::new(vec![
                Label {
                    name: "bug".to_string(),
                },
                Label {
                    name: "docs".to_string(),
                },
            ]),
            collaborators: Mutex::new(vec![
                User {
                    login: "alice".to_string(),
                },
                User {
                    login: "bob".to_string(),
                },
            ]),
            templates: Mutex::new(vec![IssueTemplate {
                name: "bug report".to_string(),
                body: "## Expected\n\n## Actual\n".to_string(),
            }]),
        }
    }

    fn comment(body: &str) -> IssueComment {
        IssueComment {
            author: None,
            body: body.to_string(),
            created_at: None,
        }
    }

    #[async_trait]
    impl IssueBackend for MockBackend {
        async fn current_login(&self) -> Result<String> {
            Ok("kpowel".to_string())
        }

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
                .unwrap_or_else(|| issue(number, &format!("Issue {number}"), IssueState::Open, 0));
            let body = self
                .detail_bodies
                .lock()
                .unwrap()
                .get(&number)
                .cloned()
                .unwrap_or_else(|| format!("## Body for issue {number}"));
            Ok(IssueDetail {
                summary,
                body,
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
            number: u64,
        ) -> Result<Vec<IssueComment>> {
            Ok(self
                .comments
                .lock()
                .unwrap()
                .get(&number)
                .cloned()
                .unwrap_or_default())
        }

        async fn list_labels(&self, _repo: &Repository) -> Result<Vec<Label>> {
            Ok(self.labels.lock().unwrap().clone())
        }

        async fn list_issue_templates(&self, _repo: &Repository) -> Result<Vec<IssueTemplate>> {
            Ok(self.templates.lock().unwrap().clone())
        }

        async fn list_collaborators(&self, _repo: &Repository) -> Result<Vec<User>> {
            Ok(self.collaborators.lock().unwrap().clone())
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
            let comment = IssueComment {
                author: None,
                body: "done".to_string(),
                created_at: None,
            };
            self.comments
                .lock()
                .unwrap()
                .entry(1)
                .or_default()
                .push(comment.clone());
            Ok(comment)
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

        async fn update_issue(
            &self,
            _repo: &Repository,
            number: u64,
            title: &str,
            body: &str,
        ) -> Result<IssueSummary> {
            *self.updated_issue.lock().unwrap() =
                Some((number, title.to_string(), body.to_string()));
            let mut issues = self.issues.lock().unwrap();
            let issue = issues
                .iter_mut()
                .find(|issue| issue.number == number)
                .expect("test issue should exist");
            issue.title = title.to_string();
            Ok(issue.clone())
        }

        async fn set_issue_assignees(
            &self,
            _repo: &Repository,
            number: u64,
            assignees: &[String],
        ) -> Result<IssueSummary> {
            self.assignee_updates
                .lock()
                .unwrap()
                .push((number, assignees.to_vec()));
            let mut issues = self.issues.lock().unwrap();
            let issue = issues
                .iter_mut()
                .find(|issue| issue.number == number)
                .expect("test issue should exist");
            issue.assignees = assignees
                .iter()
                .map(|login| User {
                    login: login.clone(),
                })
                .collect();
            Ok(issue.clone())
        }

        async fn set_issue_labels(
            &self,
            _repo: &Repository,
            number: u64,
            labels: &[String],
        ) -> Result<IssueSummary> {
            self.label_updates
                .lock()
                .unwrap()
                .push((number, labels.to_vec()));
            let mut issues = self.issues.lock().unwrap();
            let issue = issues
                .iter_mut()
                .find(|issue| issue.number == number)
                .expect("test issue should exist");
            issue.labels = labels
                .iter()
                .map(|name| Label { name: name.clone() })
                .collect();
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
    async fn assignee_filter_picker_applies_collaborator_filter() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());

        open_assignee_filter(&mut app, &backend).await;
        app.input = "ali".to_string();

        handle_assignee_picker_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(
            app.filters.assignee,
            AssigneeFilter::User("alice".to_string())
        );
        assert_eq!(app.mode, UiMode::Browsing);
    }

    #[tokio::test]
    async fn colon_command_filter_state_cycles_state_filter() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());

        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE),
        )
        .await;
        app.input = "filter state".to_string();
        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.filters.state, IssueStateFilter::Closed);
        assert_eq!(app.mode, UiMode::Browsing);
        assert_eq!(*backend.list_calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn colon_command_fs_filters_state_and_animates_list_update() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.mode = UiMode::Command;
        app.input = "fs".to_string();

        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.filters.state, IssueStateFilter::Closed);
        assert_eq!(app.mode, UiMode::Browsing);
        assert_eq!(*backend.list_calls.lock().unwrap(), 1);
        assert_eq!(app.flash, Some(FlashKind::Refresh));
    }

    #[tokio::test]
    async fn colon_command_search_alias_updates_query_and_animates_list() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.mode = UiMode::Command;
        app.input = "s redraw".to_string();

        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.filters.query, "redraw");
        assert_eq!(app.mode, UiMode::Browsing);
        assert_eq!(*backend.list_calls.lock().unwrap(), 1);
        assert_eq!(app.flash, Some(FlashKind::Refresh));
    }

    #[tokio::test]
    async fn colon_command_team_filter_aliases_update_assignee_filter() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.mode = UiMode::Command;
        app.input = "me".to_string();

        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.filters.assignee, AssigneeFilter::Me);

        app.mode = UiMode::Command;
        app.input = "unassigned".to_string();
        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.filters.assignee, AssigneeFilter::None);
    }

    #[tokio::test]
    async fn colon_command_label_and_sort_aliases_update_list_filters() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.mode = UiMode::Command;
        app.input = "label bug".to_string();

        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.filters.labels, vec!["bug"]);

        app.mode = UiMode::Command;
        app.input = "sort comments".to_string();
        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.filters.sort, IssueSort::Comments);
    }

    #[tokio::test]
    async fn colon_command_all_and_clear_reset_list_filters() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.filters.state = IssueStateFilter::Closed;
        app.filters.assignee = AssigneeFilter::Me;
        app.filters.labels = vec!["bug".to_string()];
        app.filters.query = "redraw".to_string();
        app.filters.sort = IssueSort::Comments;
        app.mode = UiMode::Command;
        app.input = "all".to_string();

        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.filters.state, IssueStateFilter::All);
        assert_eq!(app.filters.assignee, AssigneeFilter::Any);
        assert!(app.filters.labels.is_empty());
        assert!(app.filters.query.is_empty());
        assert_eq!(app.filters.sort, IssueSort::Comments);
        assert_eq!(app.mode, UiMode::Browsing);

        app.filters.state = IssueStateFilter::Closed;
        app.filters.assignee = AssigneeFilter::None;
        app.filters.labels = vec!["docs".to_string()];
        app.filters.query = "tree".to_string();
        app.filters.sort = IssueSort::Assignee;
        app.mode = UiMode::Command;
        app.input = "clear filters".to_string();

        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.filters.state, IssueStateFilter::All);
        assert_eq!(app.filters.assignee, AssigneeFilter::Any);
        assert!(app.filters.labels.is_empty());
        assert!(app.filters.query.is_empty());
        assert_eq!(app.filters.sort, IssueSort::Assignee);
        assert_eq!(*backend.list_calls.lock().unwrap(), 2);
    }

    #[tokio::test]
    async fn colon_command_assign_opens_assignee_editor() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        refresh(&mut app, &backend).await;

        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE),
        )
        .await;
        app.input = "assign".to_string();
        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.mode, UiMode::AssigneeEditor);
        assert_eq!(app.status, "Space toggles assignees, Enter saves");
    }

    #[tokio::test]
    async fn colon_command_quit_sets_quit_flag() {
        let backend = backend_with_issues(Vec::new());
        let mut app = App::new("owner/skunkwork".parse().unwrap());

        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE),
        )
        .await;
        app.input = "quit".to_string();
        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert!(app.should_quit);
    }

    #[tokio::test]
    async fn tab_completes_command_prefix() {
        let backend = backend_with_issues(Vec::new());
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.mode = UiMode::Command;
        app.input = "sor".to_string();

        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.input, "sort updated");
        assert_eq!(app.mode, UiMode::Command);
    }

    #[tokio::test]
    async fn colon_command_ping_selects_first_mentioned_issue() {
        let backend = backend_with_issues(vec![
            issue(1, "Fix redraw", IssueState::Open, 0),
            issue(2, "Needs review", IssueState::Open, 0),
        ]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        refresh(&mut app, &backend).await;
        app.highlight_mentioned_issues(vec![2]);
        app.input = ":ping".to_string();
        app.mode = UiMode::Command;

        run_command(&mut app, &backend).await;

        assert_eq!(app.selected_issue().unwrap().number, 2);
        assert_eq!(app.selected_detail.as_ref().unwrap().summary.number, 2);
        assert_eq!(app.mode, UiMode::Browsing);
    }

    #[tokio::test]
    async fn colon_command_accepts_optional_leading_colon() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());

        app.mode = UiMode::Command;
        app.input = ":filter state".to_string();
        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.filters.state, IssueStateFilter::Closed);
    }

    #[tokio::test]
    async fn browsing_shortcuts_do_not_open_command_only_actions() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);

        for code in [
            KeyCode::Char('r'),
            KeyCode::Char('/'),
            KeyCode::Char('a'),
            KeyCode::Char('A'),
            KeyCode::Char('f'),
            KeyCode::Char('l'),
            KeyCode::Char('c'),
        ] {
            handle_browsing_key(&mut app, &backend, KeyEvent::new(code, KeyModifiers::NONE)).await;
            assert_eq!(app.mode, UiMode::Browsing);
        }

        assert_eq!(app.filters.state, IssueStateFilter::Open);
        assert_eq!(*backend.list_calls.lock().unwrap(), 0);
        assert!(app.repo_labels.is_empty());
        assert!(app.repo_collaborators.is_empty());
    }

    #[tokio::test]
    async fn assignee_editor_assigns_issue_to_collaborator() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        refresh(&mut app, &backend).await;
        open_assignee_editor(&mut app, &backend).await;
        app.picker_index = 2;

        handle_assignee_picker_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        )
        .await;
        handle_assignee_picker_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(
            *backend.assignee_updates.lock().unwrap(),
            vec![(1, vec!["alice".to_string()])]
        );
        assert_eq!(app.status, "Updated assignees for issue #1");
        assert_eq!(app.mode, UiMode::Success);
    }

    #[tokio::test]
    async fn assignee_editor_space_toggles_users_and_enter_submits_all() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        refresh(&mut app, &backend).await;
        open_assignee_editor(&mut app, &backend).await;
        app.picker_index = 2;

        handle_assignee_picker_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        )
        .await;
        app.picker_index = 3;
        handle_assignee_picker_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        )
        .await;

        assert!(backend.assignee_updates.lock().unwrap().is_empty());
        assert_eq!(
            app.editing_assignees,
            vec!["alice".to_string(), "bob".to_string()]
        );

        handle_assignee_picker_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(
            *backend.assignee_updates.lock().unwrap(),
            vec![(1, vec!["alice".to_string(), "bob".to_string()])]
        );
        assert_eq!(app.status, "Updated assignees for issue #1");
        assert_eq!(app.mode, UiMode::Success);
    }

    #[tokio::test]
    async fn mouse_click_assigns_issue_to_collaborator() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        refresh(&mut app, &backend).await;
        open_assignee_editor(&mut app, &backend).await;

        handle_mouse_target(&mut app, &backend, ui::MouseTarget::PickerItem(2)).await;
        handle_mouse_target(&mut app, &backend, ui::MouseTarget::PrimaryAction).await;

        assert_eq!(
            *backend.assignee_updates.lock().unwrap(),
            vec![(1, vec!["alice".to_string()])]
        );
        assert_eq!(app.status, "Updated assignees for issue #1");
        assert_eq!(app.mode, UiMode::Success);
    }

    #[tokio::test]
    async fn label_editor_toggles_and_saves_issue_labels() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        refresh(&mut app, &backend).await;
        open_issue_label_editor(&mut app, &backend).await;

        handle_issue_label_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;
        handle_issue_label_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        )
        .await;
        handle_issue_label_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;
        handle_issue_label_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        )
        .await;

        assert_eq!(
            *backend.label_updates.lock().unwrap(),
            vec![(1, vec!["bug".to_string(), "docs".to_string()])]
        );
        assert_eq!(app.status, "Updated labels for issue #1");
        assert_eq!(app.mode, UiMode::Success);
    }

    #[tokio::test]
    async fn mouse_click_toggles_label_and_save_button_updates_labels() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        refresh(&mut app, &backend).await;
        open_issue_label_editor(&mut app, &backend).await;

        handle_mouse_target(&mut app, &backend, ui::MouseTarget::PickerItem(0)).await;
        handle_mouse_target(&mut app, &backend, ui::MouseTarget::PrimaryAction).await;

        assert_eq!(
            *backend.label_updates.lock().unwrap(),
            vec![(1, vec!["bug".to_string()])]
        );
        assert_eq!(app.status, "Updated labels for issue #1");
        assert_eq!(app.mode, UiMode::Success);
    }

    #[tokio::test]
    async fn created_issue_is_shown_even_when_immediate_list_is_stale() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        refresh(&mut app, &backend).await;

        refresh_after_action(
            &mut app,
            &backend,
            Some(2),
            "Created issue #2".to_string(),
            Some(issue(2, "New task", IssueState::Open, 0)),
            None,
        )
        .await;

        assert_eq!(app.issues[0].number, 2);
        assert_eq!(app.selected_issue().unwrap().number, 2);
        assert_eq!(app.status, "Created issue #2");
        assert_eq!(app.mode, UiMode::Success);
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

    #[tokio::test]
    async fn mouse_click_selects_issue_and_loads_detail() {
        let backend = backend_with_issues(vec![
            issue(1, "Fix redraw", IssueState::Open, 1),
            issue(2, "Add mouse", IssueState::Open, 0),
        ]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        refresh(&mut app, &backend).await;

        handle_mouse_target(&mut app, &backend, ui::MouseTarget::IssueRow(1)).await;

        assert_eq!(app.selected_issue().unwrap().number, 2);
        assert_eq!(
            app.selected_detail.as_ref().unwrap().body,
            "## Body for issue 2"
        );
    }

    #[tokio::test]
    async fn mouse_wheel_over_detail_scrolls_detail_without_changing_issue() {
        let backend = backend_with_issues(vec![
            issue(1, "Fix redraw", IssueState::Open, 1),
            issue(2, "Add mouse", IssueState::Open, 0),
        ]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        refresh(&mut app, &backend).await;
        let detail_calls = *backend.detail_calls.lock().unwrap();

        handle_mouse(
            &mut app,
            &backend,
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 96,
                row: 12,
                modifiers: KeyModifiers::NONE,
            },
            ratatui::layout::Rect::new(0, 0, 120, 32),
        )
        .await;

        assert_eq!(app.selected_issue().unwrap().number, 1);
        assert_eq!(app.detail_scroll, 3);
        assert_eq!(*backend.detail_calls.lock().unwrap(), detail_calls);
    }

    #[test]
    fn predicts_loading_preview_for_network_actions() {
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);

        assert_eq!(
            loading_preview(&app, KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            loading_preview(&app, KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
            Some((
                PendingAction::LoadLabels,
                "Loading repository labels".to_string()
            ))
        );

        app.mode = UiMode::Command;
        app.input = "refresh".to_string();
        assert_eq!(
            loading_preview(&app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some((PendingAction::Refresh, "Refreshing issues".to_string()))
        );
        app.input = "fs".to_string();
        assert_eq!(
            loading_preview(&app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
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
                KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)
            ),
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
                KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)
            ),
            None
        );
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
                KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)
            ),
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
        assert_eq!(app.flash, Some(FlashKind::Refresh));
        assert!(app.is_new_issue_highlighted(3));
        assert_eq!(app.mode, UiMode::Browsing);
    }

    #[tokio::test]
    async fn auto_refresh_notifies_when_new_comment_mentions_viewer() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        backend
            .comments
            .lock()
            .unwrap()
            .insert(1, vec![comment("Initial note")]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_viewer_login("kpowel");
        refresh(&mut app, &backend).await;

        *backend.issues.lock().unwrap() = vec![issue(1, "Fix redraw", IssueState::Open, 2)];
        backend.comments.lock().unwrap().insert(
            1,
            vec![comment("Initial note"), comment("@kpowel can you look?")],
        );

        let outcome = auto_refresh(&mut app, &backend).await;

        assert_eq!(outcome.mention_issue_numbers, vec![1]);
        assert!(outcome.has_notifications());
        assert_eq!(app.status, "Mentioned on #1: Fix redraw");
        assert_eq!(
            app.issue_highlight_kind(1),
            Some(crate::app::IssueHighlightKind::Mention)
        );
    }

    #[tokio::test]
    async fn auto_refresh_notifies_when_new_issue_body_mentions_viewer() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 0)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_viewer_login("kpowel");
        refresh(&mut app, &backend).await;

        *backend.issues.lock().unwrap() = vec![
            issue(2, "Need review", IssueState::Open, 0),
            issue(1, "Fix redraw", IssueState::Open, 0),
        ];
        backend
            .detail_bodies
            .lock()
            .unwrap()
            .insert(2, "Please review @kpowel".to_string());

        let outcome = auto_refresh(&mut app, &backend).await;

        assert_eq!(outcome.mention_issue_numbers, vec![2]);
        assert_eq!(
            app.issue_highlight_kind(2),
            Some(crate::app::IssueHighlightKind::Mention)
        );
    }

    #[tokio::test]
    async fn auto_refresh_notifies_when_updated_issue_body_mentions_viewer() {
        let mut first = issue(1, "Fix redraw", IssueState::Open, 0);
        first.updated_at = Some(chrono::Utc::now() - chrono::Duration::minutes(5));
        let backend = backend_with_issues(vec![first.clone()]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_viewer_login("kpowel");
        refresh(&mut app, &backend).await;

        first.updated_at = Some(chrono::Utc::now());
        *backend.issues.lock().unwrap() = vec![first];
        backend
            .detail_bodies
            .lock()
            .unwrap()
            .insert(1, "Updated body for @kpowel".to_string());

        let outcome = auto_refresh(&mut app, &backend).await;

        assert_eq!(outcome.mention_issue_numbers, vec![1]);
        assert_eq!(app.status, "Mentioned on #1: Fix redraw");
    }

    #[tokio::test]
    async fn auto_refresh_ignores_old_mentions_without_new_comments() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        backend
            .comments
            .lock()
            .unwrap()
            .insert(1, vec![comment("@kpowel old note")]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_viewer_login("kpowel");
        refresh(&mut app, &backend).await;

        let outcome = auto_refresh(&mut app, &backend).await;

        assert!(outcome.mention_issue_numbers.is_empty());
        assert!(!outcome.has_notifications());
    }

    #[test]
    fn mention_matching_requires_github_login_boundary() {
        assert!(text_mentions_login("@kpowel please review", "kpowel"));
        assert!(text_mentions_login("cc @KPOWEL.", "kpowel"));
        assert!(!text_mentions_login(
            "@kpowel-extra should not match",
            "kpowel"
        ));
        assert!(!text_mentions_login("@kpowell should not match", "kpowel"));
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
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        )
        .await;

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
    async fn tab_completes_comment_and_close_comment_mentions() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_repo_collaborators(vec![User {
            login: "alice".to_string(),
        }]);

        app.mode = UiMode::CommentComposer;
        app.input = "cc @a".to_string();
        handle_comment_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        )
        .await;
        assert_eq!(app.input, "cc @alice ");

        app.mode = UiMode::CloseComment;
        app.input = "closing @a".to_string();
        handle_close_comment_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        )
        .await;
        assert_eq!(app.input, "closing @alice ");
    }

    #[tokio::test]
    async fn comment_editor_inserts_and_deletes_at_cursor() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.mode = UiMode::CommentComposer;
        app.input = "abcd".to_string();

        for key in [
            KeyCode::Left,
            KeyCode::Left,
            KeyCode::Char('X'),
            KeyCode::Left,
            KeyCode::Backspace,
            KeyCode::Home,
            KeyCode::Delete,
        ] {
            handle_comment_key(&mut app, &backend, KeyEvent::new(key, KeyModifiers::NONE)).await;
        }

        assert_eq!(app.input, "Xcd");
    }

    #[tokio::test]
    async fn mention_completion_uses_cursor_position() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_repo_collaborators(vec![
            User {
                login: "alice".to_string(),
            },
            User {
                login: "bob".to_string(),
            },
        ]);
        app.mode = UiMode::CommentComposer;
        app.input = "cc @a and @b".to_string();

        for _ in 0.." and @b".len() {
            handle_comment_key(
                &mut app,
                &backend,
                KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            )
            .await;
        }
        handle_comment_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.input, "cc @alice  and @b");
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
            "Closing requires a comment, Tab completes @mentions, Ctrl+S closes"
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
        assert_eq!(app.repo_issue_templates[0].name, "bug report");
    }

    #[tokio::test]
    async fn new_issue_can_apply_loaded_template() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        open_new_issue(&mut app, &backend).await;

        handle_new_issue_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL),
        )
        .await;

        assert_eq!(app.input, "bug report");
        assert_eq!(app.body_input, "## Expected\n\n## Actual\n");
        assert_eq!(app.status, "Applied issue template");
    }

    #[tokio::test]
    async fn edit_issue_updates_title_and_body() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        refresh(&mut app, &backend).await;

        open_issue_editor(&mut app, &backend).await;

        assert_eq!(app.mode, UiMode::IssueEditor);
        assert_eq!(app.input, "Fix redraw");
        assert_eq!(app.body_input, "## Body for issue 1");

        app.input = "Fix redraw properly".to_string();
        app.body_input = "Updated body".to_string();
        handle_issue_editor_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        )
        .await;

        assert_eq!(
            backend.updated_issue.lock().unwrap().as_ref(),
            Some(&(
                1,
                "Fix redraw properly".to_string(),
                "Updated body".to_string()
            ))
        );
        assert_eq!(app.status, "Updated issue #1");
        assert_eq!(app.mode, UiMode::Success);
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
    async fn tab_completes_mentions_in_new_issue_and_issue_edit_bodies() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.set_repo_collaborators(vec![User {
            login: "alice".to_string(),
        }]);

        app.mode = UiMode::NewIssue;
        app.new_issue_field = NewIssueField::Body;
        app.body_input = "Need @a".to_string();
        handle_new_issue_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        )
        .await;
        assert_eq!(app.body_input, "Need @alice ");
        assert_eq!(app.new_issue_field, NewIssueField::Body);

        app.mode = UiMode::IssueEditor;
        app.issue_edit_field = IssueEditField::Body;
        app.body_input = "Body @a".to_string();
        handle_issue_editor_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        )
        .await;
        assert_eq!(app.body_input, "Body @alice ");
        assert_eq!(app.issue_edit_field, IssueEditField::Body);
    }

    #[tokio::test]
    async fn body_enter_inserts_newline_and_ctrl_s_submits_new_issue() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        open_new_issue(&mut app, &backend).await;
        app.set_input_text("New task");
        app.new_issue_field = NewIssueField::Body;
        app.set_body_input_text("line one");

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
    async fn issue_body_editor_inserts_newline_at_cursor() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/skunkwork".parse().unwrap());
        app.mode = UiMode::IssueEditor;
        app.issue_edit_field = IssueEditField::Body;
        app.body_input = "one two".to_string();

        for _ in 0.."two".len() {
            handle_issue_editor_key(
                &mut app,
                &backend,
                KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            )
            .await;
        }
        handle_issue_editor_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.body_input, "one \ntwo");
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
