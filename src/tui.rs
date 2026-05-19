use std::{
    collections::{BTreeSet, HashMap},
    io::{self, Write},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use chrono::Utc;
use color_eyre::eyre::Result;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEvent, KeyModifiers, MouseButton,
        MouseEvent, MouseEventKind,
    },
    execute,
};
use ratatui::DefaultTerminal;

use crate::{
    app::{
        App, AssigneeChoice, AssigneeFilter, ErrorRemediation, FlashKind, IssueEditField,
        IssueSort, IssueStateFilter, IssueView, NewIssueField, PendingAction, TextCursorMove,
        UiMode,
    },
    cache::IssueCache,
    config::AppConfig,
    domain::{IssueComment, IssueState, IssueSummary, User},
    github::IssueBackend,
    message::TissueMsg,
    realm::TissueRealm,
    ui::{self, TissueEffects},
};

const AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
#[cfg(target_os = "macos")]
const MACOS_NOTIFICATION_SOUND: &str = "/System/Library/Sounds/Glass.aiff";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BoardMoveDirection {
    Previous,
    Next,
}

pub async fn run<B: IssueBackend>(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    backend: &B,
) -> Result<()> {
    let mut effects = TissueEffects::default();
    let mut realm = TissueRealm::new(app)?;
    let config = AppConfig::load();
    app.set_project_board_config(config.project_board);
    app.set_saved_views(config.views);
    load_cached_issues(app);
    app.begin_action(PendingAction::Refresh, "Starting tissues");
    effects.trigger_startup_loading();
    draw_app(
        terminal,
        &mut realm,
        app,
        &mut effects,
        Duration::from_millis(120),
        true,
    )?;
    if let Ok(login) = backend.current_login().await {
        app.set_viewer_login(login);
    }
    refresh(app, backend).await;
    let mut mouse_capture_enabled = true;
    sync_mouse_capture(app, &mut mouse_capture_enabled)?;

    let mut last_frame = Instant::now();
    let mut next_auto_refresh = Instant::now() + AUTO_REFRESH_INTERVAL;
    while !app.should_quit {
        let elapsed = last_frame.elapsed();
        last_frame = Instant::now();
        ui::trigger_flash_effect(app, &mut effects);
        sync_mouse_capture(app, &mut mouse_capture_enabled)?;

        draw_app(terminal, &mut realm, app, &mut effects, elapsed, false)?;
        if app.mode == UiMode::IssueDetailClosing && !effects.has_effects() {
            app.mode = UiMode::Browsing;
        }
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

        for msg in realm.tick(Duration::from_millis(33))? {
            match msg {
                TissueMsg::Key(key) => {
                    if let Some((action, status)) = loading_preview(app, key) {
                        let mut preview = app.clone();
                        preview.begin_action(action, status);
                        ui::trigger_flash_effect(&mut preview, &mut effects);
                        draw_app(
                            terminal,
                            &mut realm,
                            &preview,
                            &mut effects,
                            last_frame.elapsed(),
                            false,
                        )?;
                    }
                    handle_key(app, backend, key).await;
                }
                TissueMsg::Mouse(mouse) => {
                    handle_mouse(app, backend, mouse, terminal.size()?.into()).await;
                }
                TissueMsg::Resize(_, _) | TissueMsg::Tick => {}
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

fn sync_mouse_capture(app: &App, mouse_capture_enabled: &mut bool) -> Result<()> {
    let should_enable = mouse_capture_should_be_enabled(app);
    if should_enable == *mouse_capture_enabled {
        return Ok(());
    }

    if should_enable {
        execute!(io::stdout(), EnableMouseCapture)?;
    } else {
        execute!(io::stdout(), DisableMouseCapture)?;
    }
    *mouse_capture_enabled = should_enable;

    Ok(())
}

fn mouse_capture_should_be_enabled(app: &App) -> bool {
    app.mode != UiMode::Error
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
    realm: &mut TissueRealm,
    app: &App,
    effects: &mut TissueEffects,
    elapsed: Duration,
    startup_loading: bool,
) -> Result<()> {
    terminal.draw(|frame| {
        let area = frame.area();
        realm.render(app, frame, area);
        let effect_area = draw_effect_area(app, area, startup_loading);
        effects.process(elapsed, frame.buffer_mut(), effect_area);
    })?;
    Ok(())
}

fn draw_effect_area(
    app: &App,
    area: ratatui::layout::Rect,
    startup_loading: bool,
) -> ratatui::layout::Rect {
    if startup_loading {
        area
    } else {
        ui::effect_area(app, area)
    }
}

fn loading_preview(app: &App, key: KeyEvent) -> Option<(PendingAction, String)> {
    match app.mode {
        UiMode::Browsing => match key.code {
            KeyCode::Char('n') => Some((
                PendingAction::LoadLabels,
                "Loading repository labels".to_string(),
            )),
            KeyCode::Left | KeyCode::Right if app.issue_view == IssueView::Board => {
                app.selected_issue().map(|issue| {
                    (
                        PendingAction::UpdateProjectItem,
                        format!("Moving issue #{}", issue.number),
                    )
                })
            }
            _ => None,
        },
        UiMode::IssueDetail
            if app.issue_view == IssueView::Board
                && matches!(key.code, KeyCode::Left | KeyCode::Right) =>
        {
            app.selected_issue().map(|issue| {
                (
                    PendingAction::UpdateProjectItem,
                    format!("Moving issue #{}", issue.number),
                )
            })
        }
        UiMode::Command if key.code == KeyCode::Enter => {
            let command = normalized_command(&app.input);
            if is_refresh_command(&command)
                || is_filter_state_command(&command)
                || saved_view_command(&command).is_some()
                || assignee_direct_filter_command(&command).is_some()
            {
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
                    "board" | "boards" => {
                        Some((PendingAction::Refresh, "Loading project boards".to_string()))
                    }
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

fn is_global_quit_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
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
    if is_global_quit_key(key) {
        app.should_quit = true;
        return;
    }

    match app.mode {
        UiMode::Browsing => handle_browsing_key(app, backend, key).await,
        UiMode::IssueDetail => handle_issue_detail_key(app, backend, key).await,
        UiMode::IssueDetailClosing => {
            app.mode = UiMode::Browsing;
        }
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
        UiMode::ProjectBoardPicker => handle_project_board_picker_key(app, backend, key).await,
        UiMode::ConfirmClose => handle_confirm_key(app, backend, key).await,
        UiMode::Success => {
            app.mode = UiMode::Browsing;
        }
        UiMode::Error => match key.code {
            KeyCode::Esc => {
                app.clear_error();
                app.mode = UiMode::Browsing;
            }
            KeyCode::Char('r')
                if app
                    .error_detail
                    .as_ref()
                    .and_then(|error| error.remediation.as_ref())
                    .is_some() =>
            {
                repair_error(app, backend).await;
            }
            _ => {}
        },
        _ => app.mode = UiMode::Browsing,
    }
}

async fn handle_browsing_key<B: IssueBackend>(app: &mut App, backend: &B, key: KeyEvent) {
    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('t') => app.toggle_triage_mode(),
        KeyCode::Char('v') => toggle_issue_view(app, backend).await,
        KeyCode::Left if app.issue_view == IssueView::Board => {
            move_selected_project_issue(app, backend, BoardMoveDirection::Previous).await;
        }
        KeyCode::Right if app.issue_view == IssueView::Board => {
            move_selected_project_issue(app, backend, BoardMoveDirection::Next).await;
        }
        KeyCode::Char('s') if app.triage_mode => app.skip_triage_issue(),
        KeyCode::Char('a') if app.triage_mode && app.selected_issue().is_some() => {
            assign_selected_issue_to_viewer(app, backend).await;
        }
        KeyCode::Char('l') if app.triage_mode && app.selected_issue().is_some() => {
            open_issue_label_editor(app, backend).await;
        }
        KeyCode::Char('c') if app.triage_mode && app.selected_issue().is_some() => {
            open_comment_composer(app, backend).await;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_previous();
        }
        KeyCode::Enter => open_selected_detail(app, backend).await,
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

async fn handle_issue_detail_key<B: IssueBackend>(app: &mut App, backend: &B, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => close_issue_detail(app),
        KeyCode::Enter => app.toggle_comments(),
        KeyCode::Char('v') => toggle_issue_view(app, backend).await,
        KeyCode::Left if app.issue_view == IssueView::Board => {
            move_selected_project_issue(app, backend, BoardMoveDirection::Previous).await;
        }
        KeyCode::Right if app.issue_view == IssueView::Board => {
            move_selected_project_issue(app, backend, BoardMoveDirection::Next).await;
        }
        KeyCode::PageDown => app.scroll_detail_page_down(),
        KeyCode::PageUp => app.scroll_detail_page_up(),
        KeyCode::Char('j') | KeyCode::Down => app.scroll_detail_down(),
        KeyCode::Char('k') | KeyCode::Up => app.scroll_detail_up(),
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
            app.select_next();
        }
        MouseEventKind::ScrollUp if app.mode == UiMode::Browsing => {
            app.select_previous();
        }
        MouseEventKind::ScrollDown if app.mode == UiMode::IssueDetail => {
            if ui::mouse_target(app, area, mouse.column, mouse.row)
                == Some(ui::MouseTarget::DetailPanel)
            {
                app.scroll_detail_down();
                return;
            }
            app.select_next();
            open_selected_detail(app, backend).await;
        }
        MouseEventKind::ScrollUp if app.mode == UiMode::IssueDetail => {
            if ui::mouse_target(app, area, mouse.column, mouse.row)
                == Some(ui::MouseTarget::DetailPanel)
            {
                app.scroll_detail_up();
                return;
            }
            app.select_previous();
            open_selected_detail(app, backend).await;
        }
        _ => {}
    }
}

async fn handle_mouse_target<B: IssueBackend>(app: &mut App, backend: &B, target: ui::MouseTarget) {
    match target {
        ui::MouseTarget::IssueRow(index) if app.mode == UiMode::Browsing => {
            app.select_issue_index(index);
        }
        ui::MouseTarget::IssueRow(index) if app.mode == UiMode::IssueDetail => {
            app.select_issue_index(index);
            open_selected_detail(app, backend).await;
        }
        ui::MouseTarget::DetailPanel if app.mode == UiMode::IssueDetail => app.toggle_comments(),
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
            UiMode::ProjectBoardPicker => select_project_board_choice(app, backend).await,
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
                UiMode::ProjectBoardPicker => {
                    select_project_board_choice(app, backend).await;
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
        UiMode::AssigneeFilter
        | UiMode::AssigneeEditor
        | UiMode::IssueLabelEditor
        | UiMode::ProjectBoardPicker => {
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

    if let Some(number) = issue_jump_command(&command) {
        app.select_issue_number(number);
        open_selected_detail(app, backend).await;
        return;
    }

    if let Some(login) = assignee_direct_filter_command(&command) {
        app.filters.assignee = login;
        app.active_view = None;
        app.mode = UiMode::Browsing;
        refresh(app, backend).await;
        return;
    }

    if let Some(view_name) = saved_view_command(&command) {
        if app.apply_saved_view(view_name) {
            app.mode = UiMode::Browsing;
            refresh(app, backend).await;
        } else {
            app.mode = UiMode::Browsing;
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("Unknown view: {view_name}"));
        }
        return;
    }

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
            app.active_view = None;
            app.mode = UiMode::Browsing;
            refresh(app, backend).await;
        }
        "unassigned" | "none" => {
            app.filters.assignee = AssigneeFilter::None;
            app.active_view = None;
            app.mode = UiMode::Browsing;
            refresh(app, backend).await;
        }
        "any" | "all assignees" => {
            app.filters.assignee = AssigneeFilter::Any;
            app.active_view = None;
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
            app.active_view = None;
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
            app.active_view = None;
            app.mode = UiMode::Browsing;
            refresh(app, backend).await;
        }
        "all" | "clear" | "clear filters" | "filters clear" | "reset filters" => {
            app.clear_filters();
            app.mode = UiMode::Browsing;
            refresh(app, backend).await;
        }
        "n" | "new" | "new issue" => open_new_issue(app, backend).await,
        "t" | "triage" => {
            app.toggle_triage_mode();
            app.mode = UiMode::Browsing;
        }
        "list" => app.set_issue_view(IssueView::List),
        "board" => open_board_view(app, backend).await,
        "boards" => open_project_board_chooser(app, backend, true).await,
        "views" => {
            app.mode = UiMode::Browsing;
            let names = app.saved_view_names();
            if names.is_empty() {
                app.set_status("No saved views configured");
            } else {
                app.set_status(format!("Saved views: {}", names.join(", ")));
            }
        }
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

fn issue_jump_command(command: &str) -> Option<u64> {
    command
        .strip_prefix('#')
        .unwrap_or(command)
        .parse::<u64>()
        .ok()
}

fn assignee_direct_filter_command(command: &str) -> Option<AssigneeFilter> {
    let assignee = command.strip_prefix('@')?;
    match assignee {
        "me" => Some(AssigneeFilter::Me),
        "none" | "unassigned" => Some(AssigneeFilter::None),
        "" => None,
        login => Some(AssigneeFilter::User(login.to_string())),
    }
}

fn saved_view_command(command: &str) -> Option<&str> {
    command
        .strip_prefix("view ")
        .or_else(|| command.strip_prefix("v "))
        .map(str::trim)
        .filter(|name| !name.is_empty())
}

async fn jump_to_first_mention<B: IssueBackend>(app: &mut App, backend: &B) {
    let Some(number) = app.first_mentioned_issue_number() else {
        app.mode = UiMode::Browsing;
        app.flash = Some(FlashKind::Error);
        app.set_status("No mention highlights to jump to");
        return;
    };

    app.select_issue_number(number);
    open_selected_detail(app, backend).await;
    app.set_status(format!("Jumped to mention on issue #{number}"));
}

async fn toggle_issue_view<B: IssueBackend>(app: &mut App, backend: &B) {
    app.cycle_issue_view();
    if app.issue_view == IssueView::Board {
        open_project_board_chooser(app, backend, false).await;
    }
}

async fn open_board_view<B: IssueBackend>(app: &mut App, backend: &B) {
    app.set_issue_view(IssueView::Board);
    if app.project_board_config.is_configured() {
        load_project_board(app, backend).await;
    } else {
        open_project_board_chooser(app, backend, false).await;
    }
}

async fn open_project_board_chooser<B: IssueBackend>(
    app: &mut App,
    backend: &B,
    force_picker: bool,
) {
    app.begin_action(PendingAction::Refresh, "Loading project boards");
    match backend.list_project_boards(&app.repo).await {
        Ok(choices) if choices.is_empty() => {
            app.finish_action();
            load_project_board(app, backend).await;
        }
        Ok(choices) if choices.len() == 1 && !force_picker => {
            app.finish_action();
            select_project_board_summary(app, backend, choices[0].clone()).await;
        }
        Ok(choices) => {
            app.finish_action();
            app.open_project_board_picker(choices);
        }
        Err(err) => {
            app.finish_action();
            let details = format!("{err:#}");
            app.show_error_with_remediation(
                "Project board load failed",
                github_scope_error_details(&details, &["read:project"], "project board access"),
                Some(project_auth_hint()),
                github_scope_remediation(
                    &details,
                    &["read:project"],
                    "Refresh GitHub project access",
                ),
            );
        }
    }
}

async fn select_project_board_summary<B: IssueBackend>(
    app: &mut App,
    backend: &B,
    choice: crate::domain::ProjectBoardSummary,
) {
    let status_field = app.project_board_config.status_field.clone();
    app.set_project_board_config(crate::config::ProjectBoardConfig {
        owner: Some(choice.owner),
        number: Some(choice.number),
        status_field,
    });
    app.set_issue_view(IssueView::Board);
    load_project_board(app, backend).await;
}

async fn select_project_board_choice<B: IssueBackend>(app: &mut App, backend: &B) {
    let Some(choice) = app.project_board_choices.get(app.picker_index).cloned() else {
        app.mode = UiMode::Browsing;
        app.set_status("No GitHub project boards available");
        return;
    };
    select_project_board_summary(app, backend, choice).await;
}

async fn load_project_board<B: IssueBackend>(app: &mut App, backend: &B) {
    app.begin_action(PendingAction::Refresh, "Loading project board");
    match backend
        .list_project_board(&app.repo, &app.project_board_config)
        .await
    {
        Ok(Some(board)) => {
            app.finish_action();
            app.set_project_board(board);
            app.mode = UiMode::Browsing;
            app.set_status("Loaded GitHub project board");
        }
        Ok(None) => {
            app.finish_action();
            app.clear_project_board();
            app.mode = UiMode::Browsing;
            app.set_status("No GitHub project board found; showing issue state board");
        }
        Err(err) => {
            app.finish_action();
            app.clear_project_board();
            let details = format!("{err:#}");
            app.show_error_with_remediation(
                "Project board load failed",
                github_scope_error_details(&details, &["read:project"], "project board access"),
                Some(project_auth_hint()),
                github_scope_remediation(
                    &details,
                    &["read:project"],
                    "Refresh GitHub project access",
                ),
            );
        }
    }
}

fn project_auth_hint() -> String {
    "Run `gh auth status` to inspect scopes. For GitHub Projects, run `gh auth refresh -s read:project -s project`.".to_string()
}

fn github_scope_remediation(
    details: &str,
    scopes: &[&str],
    label: impl Into<String>,
) -> Option<ErrorRemediation> {
    if !looks_like_github_scope_error(details) {
        return None;
    }
    let scopes = scopes
        .iter()
        .map(|scope| (*scope).to_string())
        .collect::<Vec<_>>();
    Some(ErrorRemediation {
        label: label.into(),
        command: gh_auth_refresh_command(&scopes),
        scopes,
    })
}

fn github_scope_error_details(details: &str, scopes: &[&str], operation: &str) -> String {
    if !looks_like_github_scope_error(details) {
        return details.to_string();
    }

    let scope_list = scopes
        .iter()
        .map(|scope| format!("`{scope}`"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("GitHub denied {operation} because the active `gh` token is missing {scope_list}.")
}

fn looks_like_github_scope_error(details: &str) -> bool {
    details.contains("required scopes")
        || details.contains("required scope")
        || details.contains("requires one of the following scopes")
        || details.contains("not been granted")
        || details.contains("Resource not accessible by personal access token")
}

fn gh_auth_refresh_command(scopes: &[String]) -> String {
    let scope_args = scopes
        .iter()
        .map(|scope| format!("-s {scope}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!("gh auth refresh {scope_args}")
}

async fn repair_error<B: IssueBackend>(app: &mut App, backend: &B) {
    let Some(remediation) = app
        .error_detail
        .as_ref()
        .and_then(|error| error.remediation.clone())
    else {
        return;
    };

    app.begin_action(
        PendingAction::RepairAuth,
        format!("Running {}", remediation.command),
    );
    match backend.refresh_auth_scopes(&remediation.scopes).await {
        Ok(()) => {
            app.finish_action();
            app.clear_error();
            app.mode = UiMode::Browsing;
            app.flash = Some(FlashKind::Success);
            app.set_status(format!(
                "{} complete; retry the failed action",
                remediation.label
            ));
        }
        Err(err) => {
            app.finish_action();
            app.show_error(
                "Auth repair failed",
                format!("{err:#}"),
                Some(format!("Run `{}` in your terminal.", remediation.command)),
            );
        }
    }
}

async fn move_selected_project_issue<B: IssueBackend>(
    app: &mut App,
    backend: &B,
    direction: BoardMoveDirection,
) {
    let Some(issue_number) = app.selected_issue().map(|issue| issue.number) else {
        return;
    };
    let Some(board) = app.project_board.clone() else {
        app.flash = Some(FlashKind::Error);
        app.set_status("Load a GitHub project board before moving board items");
        return;
    };
    let Some(project_id) = board.project_id.clone() else {
        app.flash = Some(FlashKind::Error);
        app.set_status("This board view is not connected to a GitHub project");
        return;
    };
    let Some(field_id) = board.status_field_id.clone() else {
        app.flash = Some(FlashKind::Error);
        app.set_status("This GitHub project does not expose a writable status field");
        return;
    };
    let Some(item) = board
        .item_statuses
        .iter()
        .find(|item| item.issue_number == issue_number)
        .cloned()
    else {
        app.flash = Some(FlashKind::Error);
        app.set_status(format!(
            "Issue #{issue_number} is not on this GitHub project board"
        ));
        return;
    };
    let Some(current_index) = board
        .status_options
        .iter()
        .position(|option| option.name == item.status_name)
    else {
        app.flash = Some(FlashKind::Error);
        app.set_status(format!(
            "Issue #{issue_number} is not in a known board state"
        ));
        return;
    };

    let target_index = match direction {
        BoardMoveDirection::Previous if current_index == 0 => {
            app.set_status(format!(
                "Issue #{issue_number} is already in the first board state"
            ));
            return;
        }
        BoardMoveDirection::Previous => current_index - 1,
        BoardMoveDirection::Next if current_index + 1 >= board.status_options.len() => {
            app.set_status(format!(
                "Issue #{issue_number} is already in the last board state"
            ));
            return;
        }
        BoardMoveDirection::Next => current_index + 1,
    };
    let target = board.status_options[target_index].clone();

    app.begin_action(
        PendingAction::UpdateProjectItem,
        format!("Moving issue #{issue_number} to {}", target.name),
    );
    match backend
        .update_project_item_status(&project_id, &item.item_id, &field_id, &target.id)
        .await
    {
        Ok(()) => {
            app.finish_action();
            load_project_board(app, backend).await;
            if app.mode != UiMode::Error {
                app.flash = Some(FlashKind::Success);
                app.set_status(format!("Moved issue #{issue_number} to {}", target.name));
            }
        }
        Err(err) => {
            app.finish_action();
            let details = format!("{err:#}");
            app.show_error_with_remediation(
                "Move board item failed",
                github_scope_error_details(&details, &["project"], "project board edits"),
                Some(project_auth_hint()),
                github_scope_remediation(
                    &details,
                    &["project"],
                    "Refresh GitHub project write access",
                ),
            );
        }
    }
}

fn command_suggestions(input: &str) -> Vec<String> {
    let command = normalized_command(input);
    let mut suggestions = [
        "fs",
        "fa",
        "s ",
        "me",
        "unassigned",
        "@me",
        "@none",
        "view mine",
        "view untriaged",
        "view bugs",
        "triage",
        "list",
        "board",
        "boards",
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
    ]
    .into_iter()
    .enumerate()
    .filter_map(|(index, candidate)| {
        command_match_score(candidate, &command).map(|score| (score, index, candidate))
    })
    .collect::<Vec<_>>();

    suggestions.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.len().cmp(&right.2.len()))
            .then_with(|| left.2.cmp(right.2))
    });

    suggestions
        .into_iter()
        .take(5)
        .map(|(_, _, candidate)| candidate.to_owned())
        .collect()
}

fn command_match_score(candidate: &str, command: &str) -> Option<usize> {
    if command.is_empty() {
        return Some(0);
    }
    if candidate.starts_with(command) {
        return Some(0);
    }
    if !fuzzy_subsequence(candidate, command) {
        return None;
    }
    Some(1 + candidate.len().saturating_sub(command.len()))
}

fn fuzzy_subsequence(candidate: &str, query: &str) -> bool {
    let mut remaining = query.chars();
    let Some(mut needle) = remaining.next() else {
        return true;
    };

    for character in candidate.chars() {
        if character == needle {
            let Some(next) = remaining.next() else {
                return true;
            };
            needle = next;
        }
    }

    false
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

async fn handle_project_board_picker_key<B: IssueBackend>(
    app: &mut App,
    backend: &B,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => cancel_active_screen(app),
        KeyCode::Down | KeyCode::Char('j') => {
            app.select_next_picker_item(app.project_board_choices.len());
        }
        KeyCode::Up | KeyCode::Char('k') => app.select_previous_picker_item(),
        KeyCode::Enter => select_project_board_choice(app, backend).await,
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
            save_cached_issues(app);
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
            app.finish_action();
            app.flash = Some(FlashKind::Refresh);
            app.set_status(loaded_status);
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
            save_cached_issues(app);
            if let Some(number) = selected_issue_number {
                app.select_issue_number(number);
            }
            app.highlight_new_issues(new_issue_numbers.clone());
            app.highlight_mentioned_issues(mention_issue_numbers.clone());

            app.finish_action();
            app.flash = None;
            if let Some(status) = new_issue_status {
                app.set_status(status);
            } else if let Some(status) = mention_status {
                app.set_status(status);
            } else {
                app.set_status("Auto-refreshed; no new issues");
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
            save_cached_issues(app);
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
            if app
                .selected_detail
                .as_ref()
                .is_some_and(|detail| Some(detail.summary.number) == selected_issue_number)
            {
                let _ = load_selected_detail(app, backend).await;
            }
            if let (Some(number), Some(comment)) = (selected_issue_number, optimistic_comment) {
                keep_optimistic_comment_visible(app, number, comment);
            }
            app.finish_action();
            app.mode = UiMode::Success;
            app.flash = Some(FlashKind::Success);
            app.set_status(success_status);
        }
        Err(err) => {
            app.finish_action();
            app.mode = UiMode::Browsing;
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("{success_status}; refresh failed: {err:#}"));
        }
    }
}

fn load_cached_issues(app: &mut App) {
    let Some(cache) = IssueCache::for_repo(&app.repo) else {
        return;
    };
    let Ok(Some(issues)) = cache.load_issues(&app.filters) else {
        return;
    };

    app.set_issues(issues);
    app.set_status("Loaded cached issues; refreshing");
}

fn save_cached_issues(app: &App) {
    let Some(cache) = IssueCache::for_repo(&app.repo) else {
        return;
    };
    let _ = cache.save_issues(&app.filters, &app.issues);
}

fn load_cached_selected_detail(app: &mut App, number: u64) -> bool {
    let Some(cache) = IssueCache::for_repo(&app.repo) else {
        return false;
    };
    let Ok(Some(detail)) = cache.load_detail(number) else {
        return false;
    };

    app.set_selected_detail(detail);
    true
}

fn save_cached_detail(app: &App) {
    let Some(detail) = app.selected_detail.as_ref() else {
        return;
    };
    let Some(cache) = IssueCache::for_repo(&app.repo) else {
        return;
    };
    let _ = cache.save_detail(detail);
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

fn local_comment(app: &App, body: &str) -> IssueComment {
    IssueComment {
        author: app.viewer_login.as_ref().map(|login| User {
            login: login.clone(),
        }),
        body: body.to_string(),
        created_at: Some(Utc::now()),
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

    let previous_issues = app.issues.clone();
    let previous_detail = app.selected_detail.clone();
    let optimistic_comment = local_comment(app, &body);
    app.apply_optimistic_comment(number, optimistic_comment.clone());
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
            app.issues = previous_issues;
            app.selected_detail = previous_detail;
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

    let previous_issues = app.issues.clone();
    let previous_detail = app.selected_detail.clone();
    let optimistic_comment = local_comment(app, &body);
    app.apply_optimistic_comment(number, optimistic_comment);
    app.apply_optimistic_state(number, IssueState::Closed);
    app.begin_action(
        PendingAction::CloseIssue,
        format!("Closing issue #{number}"),
    );
    match backend.add_comment(&app.repo, number, &body).await {
        Ok(comment) => match backend
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
                    Some(comment),
                )
                .await;
            }
            Err(err) => {
                app.issues = previous_issues;
                app.selected_detail = previous_detail;
                app.finish_action();
                app.mode = UiMode::Browsing;
                app.flash = Some(FlashKind::Error);
                app.set_status(format!("Close failed after comment: {err:#}"));
            }
        },
        Err(err) => {
            app.issues = previous_issues;
            app.selected_detail = previous_detail;
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
            let details = format!("{err:#}");
            app.show_error_with_remediation(
                "Create issue failed",
                github_scope_error_details(&details, &["repo"], "issue writes"),
                Some(issue_write_auth_hint()),
                github_scope_remediation(&details, &["repo"], "Refresh GitHub repository access"),
            );
        }
    }
}

fn issue_write_auth_hint() -> String {
    "Run `gh auth status` to inspect scopes. For private repositories, run `gh auth refresh -s repo`. Fine-grained tokens need Issues: read/write for this repository.".to_string()
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
    app.active_view = None;
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

    let previous_issues = app.issues.clone();
    let previous_detail = app.selected_detail.clone();
    app.apply_optimistic_assignees(number, assignees.clone());
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
            app.issues = previous_issues;
            app.selected_detail = previous_detail;
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

    let previous_issues = app.issues.clone();
    let previous_detail = app.selected_detail.clone();
    app.apply_optimistic_labels(number, labels.clone());
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
            app.issues = previous_issues;
            app.selected_detail = previous_detail;
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

    let previous_issues = app.issues.clone();
    let previous_detail = app.selected_detail.clone();
    app.apply_optimistic_state(number, next_state.clone());
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
            app.issues = previous_issues;
            app.selected_detail = previous_detail;
            app.finish_action();
            app.mode = UiMode::Browsing;
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("Update failed: {err:#}"));
        }
    }
}

async fn assign_selected_issue_to_viewer<B: IssueBackend>(app: &mut App, backend: &B) {
    let Some(number) = app.selected_issue().map(|issue| issue.number) else {
        app.mode = UiMode::Browsing;
        return;
    };
    let Some(login) = app.viewer_login.clone() else {
        app.flash = Some(FlashKind::Error);
        app.set_status("Authenticated GitHub login is unavailable");
        return;
    };

    let assignees = vec![login];
    let previous_issues = app.issues.clone();
    let previous_detail = app.selected_detail.clone();
    app.apply_optimistic_assignees(number, assignees.clone());
    app.begin_action(
        PendingAction::UpdateAssignees,
        format!("Assigning issue #{number}"),
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
                format!("Assigned issue #{number}"),
                None,
                None,
            )
            .await;
        }
        Err(err) => {
            app.issues = previous_issues;
            app.selected_detail = previous_detail;
            app.finish_action();
            app.mode = UiMode::Browsing;
            app.flash = Some(FlashKind::Error);
            app.set_status(format!("Assign failed: {err:#}"));
        }
    }
}

async fn open_selected_detail<B: IssueBackend>(app: &mut App, backend: &B) {
    let Some(number) = app.selected_issue().map(|issue| issue.number) else {
        app.clear_selected_detail();
        app.mode = UiMode::Browsing;
        return;
    };

    if app
        .selected_detail
        .as_ref()
        .is_some_and(|detail| detail.summary.number == number)
    {
        app.mode = UiMode::IssueDetail;
        app.flash = Some(FlashKind::DetailOpen);
        app.set_status(format!("Viewing issue #{number}; Esc returns to list"));
        return;
    }

    let had_cached_detail = load_cached_selected_detail(app, number);
    app.begin_action(PendingAction::Refresh, format!("Loading issue #{number}"));
    match load_selected_detail(app, backend).await {
        Ok(()) => {
            app.finish_action();
            app.mode = UiMode::IssueDetail;
            app.flash = Some(FlashKind::DetailOpen);
            app.set_status(format!("Viewing issue #{number}; Esc returns to list"));
        }
        Err(err) => {
            app.finish_action();
            app.flash = Some(FlashKind::Error);
            if had_cached_detail {
                app.mode = UiMode::IssueDetail;
                app.set_status(format!(
                    "Showing cached issue #{number}; refresh failed: {err:#}"
                ));
            } else {
                app.mode = UiMode::Browsing;
                app.set_status(format!("Detail load failed: {err:#}"));
            }
        }
    }
}

fn close_issue_detail(app: &mut App) {
    app.mode = UiMode::IssueDetailClosing;
    app.flash = Some(FlashKind::DetailClose);
    app.set_status("Returned to issue list");
}

async fn load_selected_detail<B: IssueBackend>(app: &mut App, backend: &B) -> Result<()> {
    let Some(number) = app.selected_issue().map(|issue| issue.number) else {
        app.clear_selected_detail();
        return Ok(());
    };

    let detail = backend.get_issue(&app.repo, number).await?;
    app.set_selected_detail(detail);
    save_cached_detail(app);
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
        create_error: Mutex<Option<String>>,
        project_error: Mutex<Option<String>>,
        project_boards: Mutex<Vec<crate::domain::ProjectBoardSummary>>,
        project_board: Mutex<Option<crate::domain::ProjectBoard>>,
        project_moves: Mutex<Vec<(String, String, String, String)>>,
        auth_refreshes: Mutex<Vec<Vec<String>>>,
        auth_refresh_error: Mutex<Option<String>>,
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
            create_error: Mutex::new(None),
            project_error: Mutex::new(None),
            project_boards: Mutex::new(Vec::new()),
            project_board: Mutex::new(None),
            project_moves: Mutex::new(Vec::new()),
            auth_refreshes: Mutex::new(Vec::new()),
            auth_refresh_error: Mutex::new(None),
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

        async fn list_project_board(
            &self,
            _repo: &Repository,
            _config: &crate::config::ProjectBoardConfig,
        ) -> Result<Option<crate::domain::ProjectBoard>> {
            if let Some(message) = self.project_error.lock().unwrap().clone() {
                return Err(color_eyre::eyre::eyre!(message));
            }
            Ok(self.project_board.lock().unwrap().clone())
        }

        async fn list_project_boards(
            &self,
            _repo: &Repository,
        ) -> Result<Vec<crate::domain::ProjectBoardSummary>> {
            Ok(self.project_boards.lock().unwrap().clone())
        }

        async fn update_project_item_status(
            &self,
            project_id: &str,
            item_id: &str,
            field_id: &str,
            option_id: &str,
        ) -> Result<()> {
            self.project_moves.lock().unwrap().push((
                project_id.to_string(),
                item_id.to_string(),
                field_id.to_string(),
                option_id.to_string(),
            ));
            Ok(())
        }

        async fn refresh_auth_scopes(&self, scopes: &[String]) -> Result<()> {
            if let Some(message) = self.auth_refresh_error.lock().unwrap().clone() {
                return Err(color_eyre::eyre::eyre!(message));
            }
            self.auth_refreshes.lock().unwrap().push(scopes.to_vec());
            Ok(())
        }

        async fn create_issue(
            &self,
            _repo: &Repository,
            _title: &str,
            body: &str,
            labels: &[String],
        ) -> Result<IssueSummary> {
            if let Some(message) = self.create_error.lock().unwrap().clone() {
                return Err(color_eyre::eyre::eyre!(message));
            }
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        assert_eq!(*backend.detail_calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn submit_new_issue_refreshes_and_selects_created_issue() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/tissues".parse().unwrap());
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
    async fn create_issue_failure_opens_standard_error_details() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        *backend.create_error.lock().unwrap() =
            Some("Resource not accessible by personal access token".to_string());
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.start_new_issue();
        app.input = "New task".to_string();

        submit_new_issue(&mut app, &backend, true).await;

        assert_eq!(app.mode, UiMode::Error);
        assert_eq!(app.status, "Create issue failed");
        let error = app.error_detail.as_ref().expect("error detail");
        assert_eq!(error.title, "Create issue failed");
        assert!(error.details.contains("missing `repo`"));
        assert!(error.hint.as_ref().unwrap().contains("gh auth refresh"));
        assert_eq!(
            error.remediation.as_ref().unwrap().scopes,
            vec!["repo".to_string()]
        );
    }

    #[tokio::test]
    async fn assignee_filter_picker_applies_collaborator_filter() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/tissues".parse().unwrap());

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
        let mut app = App::new("owner/tissues".parse().unwrap());

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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());

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
    async fn ctrl_c_quits_from_any_mode() {
        let backend = backend_with_issues(Vec::new());
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.mode = UiMode::CommentComposer;

        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        )
        .await;

        assert!(app.should_quit);
    }

    #[tokio::test]
    async fn q_quits_only_in_browsing_mode() {
        let backend = backend_with_issues(Vec::new());
        let mut app = App::new("owner/tissues".parse().unwrap());

        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        )
        .await;
        assert!(app.should_quit);

        let mut non_browsing = App::new("owner/tissues".parse().unwrap());
        non_browsing.mode = UiMode::Command;
        handle_key(
            &mut non_browsing,
            &backend,
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        )
        .await;

        assert!(!non_browsing.should_quit);
        assert_eq!(non_browsing.input, "q");
    }

    #[tokio::test]
    async fn tab_completes_command_prefix() {
        let backend = backend_with_issues(Vec::new());
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
        refresh(&mut app, &backend).await;
        app.highlight_mentioned_issues(vec![2]);
        app.input = ":ping".to_string();
        app.mode = UiMode::Command;

        run_command(&mut app, &backend).await;

        assert_eq!(app.selected_issue().unwrap().number, 2);
        assert_eq!(app.selected_detail.as_ref().unwrap().summary.number, 2);
        assert_eq!(app.mode, UiMode::IssueDetail);
    }

    #[test]
    fn command_palette_uses_fuzzy_suggestions() {
        assert_eq!(
            command_suggestions("sco").first().map(String::as_str),
            Some("sort comments")
        );
        assert_eq!(
            command_suggestions("cm").first().map(String::as_str),
            Some("comment")
        );
    }

    #[tokio::test]
    async fn colon_command_issue_number_jumps_to_detail() {
        let backend = backend_with_issues(vec![
            issue(1, "Fix redraw", IssueState::Open, 0),
            issue(2, "Needs review", IssueState::Open, 0),
        ]);
        let mut app = App::new("owner/tissues".parse().unwrap());
        refresh(&mut app, &backend).await;
        app.mode = UiMode::Command;
        app.input = "#2".to_string();

        run_command(&mut app, &backend).await;

        assert_eq!(app.selected_issue().unwrap().number, 2);
        assert_eq!(app.selected_detail.as_ref().unwrap().summary.number, 2);
        assert_eq!(app.mode, UiMode::IssueDetail);
    }

    #[tokio::test]
    async fn colon_command_applies_saved_view() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 0)]);
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.set_saved_views(vec![crate::config::SavedView {
            name: "mine".to_string(),
            filters: IssueFilters {
                state: IssueStateFilter::Open,
                assignee: AssigneeFilter::Me,
                labels: Vec::new(),
                query: String::new(),
                sort: IssueSort::Updated,
            },
        }]);
        app.mode = UiMode::Command;
        app.input = "view mine".to_string();

        run_command(&mut app, &backend).await;

        assert_eq!(app.active_view.as_deref(), Some("mine"));
        assert_eq!(app.filters.assignee, AssigneeFilter::Me);
        assert_eq!(app.mode, UiMode::Browsing);
        assert_eq!(*backend.list_calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn triage_shortcuts_skip_assign_label_and_comment() {
        let backend = backend_with_issues(vec![
            issue(1, "Fix redraw", IssueState::Open, 0),
            issue(2, "Needs review", IssueState::Open, 0),
        ]);
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.set_viewer_login("kpowel");
        refresh(&mut app, &backend).await;

        handle_browsing_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE),
        )
        .await;
        handle_browsing_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        )
        .await;
        handle_browsing_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
        )
        .await;

        assert!(app.triage_mode);
        assert_eq!(app.selected_issue().unwrap().number, 2);
        assert_eq!(
            *backend.assignee_updates.lock().unwrap(),
            vec![(2, vec!["kpowel".to_string()])]
        );
        assert_eq!(app.selected_issue().unwrap().assignees[0].login, "kpowel");
        assert_eq!(app.mode, UiMode::Success);
    }

    #[tokio::test]
    async fn colon_command_accepts_optional_leading_colon() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/tissues".parse().unwrap());

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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
    async fn browsing_shortcut_toggles_between_list_and_board_views() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/tissues".parse().unwrap());

        handle_browsing_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
        )
        .await;
        assert_eq!(app.issue_view, IssueView::Board);

        handle_browsing_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
        )
        .await;
        assert_eq!(app.issue_view, IssueView::List);
    }

    #[tokio::test]
    async fn board_and_list_commands_switch_issue_views() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.mode = UiMode::Command;
        app.input = "board".to_string();

        run_command(&mut app, &backend).await;
        assert_eq!(app.issue_view, IssueView::Board);
        assert_eq!(app.mode, UiMode::Browsing);

        app.mode = UiMode::Command;
        app.input = "list".to_string();
        run_command(&mut app, &backend).await;
        assert_eq!(app.issue_view, IssueView::List);
        assert_eq!(app.mode, UiMode::Browsing);
    }

    #[tokio::test]
    async fn board_command_loads_project_board_from_backend() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        *backend.project_board.lock().unwrap() = Some(crate::domain::ProjectBoard {
            title: "Roadmap".to_string(),
            project_id: None,
            status_field_id: None,
            status_options: Vec::new(),
            item_statuses: Vec::new(),
            columns: vec![crate::domain::ProjectColumn {
                name: "Todo".to_string(),
                issues: vec![issue(1, "Fix redraw", IssueState::Open, 1)],
            }],
        });
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.mode = UiMode::Command;
        app.input = "board".to_string();

        run_command(&mut app, &backend).await;

        assert_eq!(app.issue_view, IssueView::Board);
        assert_eq!(app.project_board.as_ref().unwrap().title, "Roadmap");
        assert_eq!(app.status, "Loaded GitHub project board");
    }

    #[tokio::test]
    async fn board_command_opens_picker_when_multiple_project_boards_exist() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        *backend.project_boards.lock().unwrap() = vec![
            crate::domain::ProjectBoardSummary {
                id: "PVT_backlog".to_string(),
                owner: "owner".to_string(),
                number: 1,
                title: "Backlog".to_string(),
                item_count: 3,
            },
            crate::domain::ProjectBoardSummary {
                id: "PVT_roadmap".to_string(),
                owner: "owner".to_string(),
                number: 2,
                title: "Roadmap".to_string(),
                item_count: 5,
            },
        ];
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.mode = UiMode::Command;
        app.input = "board".to_string();

        run_command(&mut app, &backend).await;

        assert_eq!(app.issue_view, IssueView::Board);
        assert_eq!(app.mode, UiMode::ProjectBoardPicker);
        assert_eq!(app.project_board_choices.len(), 2);
        assert_eq!(app.status, "Choose a GitHub project board");
    }

    #[tokio::test]
    async fn board_picker_enter_loads_selected_project_board() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        *backend.project_board.lock().unwrap() = Some(crate::domain::ProjectBoard {
            title: "Roadmap".to_string(),
            project_id: None,
            status_field_id: None,
            status_options: Vec::new(),
            item_statuses: Vec::new(),
            columns: Vec::new(),
        });
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.set_project_board_choices(vec![
            crate::domain::ProjectBoardSummary {
                id: "PVT_backlog".to_string(),
                owner: "owner".to_string(),
                number: 1,
                title: "Backlog".to_string(),
                item_count: 3,
            },
            crate::domain::ProjectBoardSummary {
                id: "PVT_roadmap".to_string(),
                owner: "owner".to_string(),
                number: 2,
                title: "Roadmap".to_string(),
                item_count: 5,
            },
        ]);
        app.mode = UiMode::ProjectBoardPicker;
        app.picker_index = 1;

        handle_project_board_picker_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.issue_view, IssueView::Board);
        assert_eq!(app.project_board_config.owner.as_deref(), Some("owner"));
        assert_eq!(app.project_board_config.number, Some(2));
        assert_eq!(app.mode, UiMode::Browsing);
        assert_eq!(app.project_board.as_ref().unwrap().title, "Roadmap");
    }

    #[tokio::test]
    async fn right_arrow_moves_selected_project_issue_to_next_board_state() {
        let first_issue = issue(1, "Fix redraw", IssueState::Open, 1);
        let backend = backend_with_issues(vec![first_issue.clone()]);
        *backend.project_board.lock().unwrap() = Some(crate::domain::ProjectBoard {
            title: "Roadmap".to_string(),
            project_id: Some("project-id".to_string()),
            status_field_id: Some("status-field-id".to_string()),
            status_options: vec![
                crate::domain::ProjectStatusOption {
                    id: "todo-option".to_string(),
                    name: "Todo".to_string(),
                },
                crate::domain::ProjectStatusOption {
                    id: "doing-option".to_string(),
                    name: "Doing".to_string(),
                },
            ],
            item_statuses: vec![crate::domain::ProjectItemStatus {
                issue_number: 1,
                item_id: "item-id".to_string(),
                status_name: "Todo".to_string(),
            }],
            columns: vec![crate::domain::ProjectColumn {
                name: "Doing".to_string(),
                issues: vec![first_issue],
            }],
        });
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.set_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        app.set_project_board(crate::domain::ProjectBoard {
            title: "Roadmap".to_string(),
            project_id: Some("project-id".to_string()),
            status_field_id: Some("status-field-id".to_string()),
            status_options: vec![
                crate::domain::ProjectStatusOption {
                    id: "todo-option".to_string(),
                    name: "Todo".to_string(),
                },
                crate::domain::ProjectStatusOption {
                    id: "doing-option".to_string(),
                    name: "Doing".to_string(),
                },
            ],
            item_statuses: vec![crate::domain::ProjectItemStatus {
                issue_number: 1,
                item_id: "item-id".to_string(),
                status_name: "Todo".to_string(),
            }],
            columns: vec![crate::domain::ProjectColumn {
                name: "Todo".to_string(),
                issues: vec![issue(1, "Fix redraw", IssueState::Open, 1)],
            }],
        });
        app.set_issue_view(IssueView::Board);

        handle_browsing_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(
            *backend.project_moves.lock().unwrap(),
            vec![(
                "project-id".to_string(),
                "item-id".to_string(),
                "status-field-id".to_string(),
                "doing-option".to_string(),
            )]
        );
        assert_eq!(app.status, "Moved issue #1 to Doing");
        assert_eq!(app.project_board.as_ref().unwrap().columns[0].name, "Doing");
    }

    #[tokio::test]
    async fn left_arrow_on_first_project_state_does_not_mutate() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.set_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        app.set_project_board(crate::domain::ProjectBoard {
            title: "Roadmap".to_string(),
            project_id: Some("project-id".to_string()),
            status_field_id: Some("status-field-id".to_string()),
            status_options: vec![
                crate::domain::ProjectStatusOption {
                    id: "todo-option".to_string(),
                    name: "Todo".to_string(),
                },
                crate::domain::ProjectStatusOption {
                    id: "doing-option".to_string(),
                    name: "Doing".to_string(),
                },
            ],
            item_statuses: vec![crate::domain::ProjectItemStatus {
                issue_number: 1,
                item_id: "item-id".to_string(),
                status_name: "Todo".to_string(),
            }],
            columns: vec![crate::domain::ProjectColumn {
                name: "Todo".to_string(),
                issues: vec![issue(1, "Fix redraw", IssueState::Open, 1)],
            }],
        });
        app.set_issue_view(IssueView::Board);

        handle_browsing_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
        )
        .await;

        assert!(backend.project_moves.lock().unwrap().is_empty());
        assert_eq!(app.status, "Issue #1 is already in the first board state");
    }

    #[tokio::test]
    async fn board_load_failure_opens_standard_error_details() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        *backend.project_error.lock().unwrap() =
            Some("projectsV2 field requires one of the following scopes: ['read:project']".into());
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.mode = UiMode::Command;
        app.input = "board".to_string();

        run_command(&mut app, &backend).await;

        assert_eq!(app.issue_view, IssueView::Board);
        assert_eq!(app.mode, UiMode::Error);
        assert_eq!(app.status, "Project board load failed");
        let error = app.error_detail.as_ref().expect("error detail");
        assert_eq!(error.title, "Project board load failed");
        assert_eq!(
            error.details,
            "GitHub denied project board access because the active `gh` token is missing `read:project`."
        );
        assert!(!error.details.contains("line 4"));
        assert!(error.hint.as_ref().unwrap().contains("read:project"));
        let remediation = error.remediation.as_ref().expect("auth remediation");
        assert_eq!(remediation.command, "gh auth refresh -s read:project");
        assert_eq!(remediation.scopes, vec!["read:project".to_string()]);
    }

    #[tokio::test]
    async fn error_repair_key_refreshes_auth_scopes_and_returns_to_browsing() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.show_error_with_remediation(
            "Project board load failed",
            "GitHub denied project board access because the token is missing `read:project`.",
            Some("Run `gh auth refresh -s read:project`.".to_string()),
            Some(ErrorRemediation {
                label: "Refresh GitHub project access".to_string(),
                command: "gh auth refresh -s read:project".to_string(),
                scopes: vec!["read:project".to_string()],
            }),
        );

        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
        )
        .await;

        assert_eq!(
            *backend.auth_refreshes.lock().unwrap(),
            vec![vec!["read:project".to_string()]]
        );
        assert_eq!(app.mode, UiMode::Browsing);
        assert!(app.error_detail.is_none());
        assert_eq!(
            app.status,
            "Refresh GitHub project access complete; retry the failed action"
        );
    }

    #[tokio::test]
    async fn short_board_command_is_not_supported() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.mode = UiMode::Command;
        app.input = "b".to_string();

        run_command(&mut app, &backend).await;

        assert_eq!(app.issue_view, IssueView::List);
        assert_eq!(app.mode, UiMode::Browsing);
        assert_eq!(app.flash, Some(FlashKind::Error));
        assert_eq!(app.status, "Unknown command: b");
    }

    #[test]
    fn startup_status_does_not_make_later_refresh_effects_full_screen() {
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.begin_action(PendingAction::Refresh, "Starting tissues");
        let area = ratatui::layout::Rect::new(0, 0, 120, 40);

        let target = draw_effect_area(&app, area, false);

        assert_ne!(target, area);
        assert_eq!(target, ui::effect_area(&app, area));
    }

    #[tokio::test]
    async fn assignee_editor_assigns_issue_to_collaborator() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
    async fn mouse_click_selects_issue_without_loading_detail() {
        let backend = backend_with_issues(vec![
            issue(1, "Fix redraw", IssueState::Open, 1),
            issue(2, "Add mouse", IssueState::Open, 0),
        ]);
        let mut app = App::new("owner/tissues".parse().unwrap());
        refresh(&mut app, &backend).await;

        handle_mouse_target(&mut app, &backend, ui::MouseTarget::IssueRow(1)).await;

        assert_eq!(app.selected_issue().unwrap().number, 2);
        assert_eq!(*backend.detail_calls.lock().unwrap(), 0);
        assert!(app.selected_detail.is_none());
    }

    #[tokio::test]
    async fn mouse_wheel_over_detail_scrolls_detail_without_changing_issue() {
        let backend = backend_with_issues(vec![
            issue(1, "Fix redraw", IssueState::Open, 1),
            issue(2, "Add mouse", IssueState::Open, 0),
        ]);
        let mut app = App::new("owner/tissues".parse().unwrap());
        refresh(&mut app, &backend).await;
        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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

        app.set_issue_view(IssueView::Board);
        assert_eq!(
            loading_preview(&app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
            Some((
                PendingAction::UpdateProjectItem,
                "Moving issue #1".to_string()
            ))
        );
        app.set_issue_view(IssueView::List);

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
    async fn refresh_loads_issue_list_without_selected_detail() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/tissues".parse().unwrap());

        refresh(&mut app, &backend).await;

        assert_eq!(*backend.detail_calls.lock().unwrap(), 0);
        assert!(app.selected_detail.is_none());
        assert_eq!(app.mode, UiMode::Browsing);
    }

    #[tokio::test]
    async fn auto_refresh_reports_new_issues_and_preserves_selection() {
        let backend = backend_with_issues(vec![
            issue(1, "Fix redraw", IssueState::Open, 1),
            issue(2, "Add tree", IssueState::Open, 0),
        ]);
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        assert_eq!(app.flash, None);
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
        refresh(&mut app, &backend).await;

        let outcome = auto_refresh(&mut app, &backend).await;

        assert_eq!(outcome, AutoRefreshOutcome::default());
        assert_eq!(app.status, "Auto-refreshed; no new issues");
        assert_eq!(app.flash, None);
        assert_eq!(app.mode, UiMode::Browsing);
    }

    #[test]
    fn auto_refresh_only_runs_while_browsing() {
        let mut app = App::new("owner/tissues".parse().unwrap());
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
    async fn navigating_issue_list_does_not_load_detail() {
        let backend = backend_with_issues(vec![
            issue(1, "Fix redraw", IssueState::Open, 1),
            issue(2, "Add tree", IssueState::Open, 0),
        ]);
        let mut app = App::new("owner/tissues".parse().unwrap());
        refresh(&mut app, &backend).await;

        handle_browsing_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(*backend.detail_calls.lock().unwrap(), 0);
        assert_eq!(app.selected_issue().unwrap().number, 2);
        assert!(app.selected_detail.is_none());
    }

    #[tokio::test]
    async fn enter_loads_detail_and_escape_starts_return_to_issue_list() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/tissues".parse().unwrap());
        refresh(&mut app, &backend).await;

        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(*backend.detail_calls.lock().unwrap(), 1);
        assert_eq!(app.mode, UiMode::IssueDetail);
        assert_eq!(app.flash, Some(FlashKind::DetailOpen));
        assert_eq!(
            app.selected_detail.as_ref().unwrap().body,
            "## Body for issue 1"
        );

        handle_key(
            &mut app,
            &backend,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        )
        .await;

        assert_eq!(app.mode, UiMode::IssueDetailClosing);
        assert_eq!(app.flash, Some(FlashKind::DetailClose));
    }

    #[tokio::test]
    async fn enter_toggles_comment_tree_in_detail_mode() {
        let backend = backend_with_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        let mut app = App::new("owner/tissues".parse().unwrap());
        app.set_issues(vec![issue(1, "Fix redraw", IssueState::Open, 1)]);
        app.mode = UiMode::IssueDetail;
        app.set_selected_detail(IssueDetail {
            summary: issue(1, "Fix redraw", IssueState::Open, 1),
            body: "Body".to_string(),
            comments: Vec::new(),
        });

        handle_key(
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());

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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
        let mut app = App::new("owner/tissues".parse().unwrap());
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
