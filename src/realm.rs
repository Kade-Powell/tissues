use std::time::Duration;

use crossterm::event::{
    KeyCode, KeyEvent as CrosstermKeyEvent, KeyModifiers as CrosstermKeyModifiers, MouseButton,
    MouseEvent as CrosstermMouseEvent, MouseEventKind as CrosstermMouseEventKind,
};
use tuirealm::{
    application::{Application, ApplicationResult, PollStrategy},
    command::{Cmd, CmdResult},
    component::{AppComponent, Component},
    event::{
        Event, Key, KeyEvent, KeyModifiers, MouseButton as RealmMouseButton, MouseEvent,
        MouseEventKind, NoUserEvent,
    },
    listener::EventListenerCfg,
    props::{AttrValue, Attribute, QueryResult},
    ratatui::{Frame, layout::Rect},
    state::State,
};

use crate::{app::App, message::TissueMsg, ui};

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(20);
const INPUT_MAX_POLL: usize = 8;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RealmId {
    Page,
    Header,
    Filters,
    Body,
    CommandBar,
    Footer,
    Overlay,
    Input,
}

pub struct TissueRealm {
    app: Application<RealmId, TissueMsg, NoUserEvent>,
}

impl TissueRealm {
    pub fn new(app_state: &App) -> ApplicationResult<Self> {
        let mut app = Application::init(
            EventListenerCfg::default()
                .crossterm_input_listener(INPUT_POLL_INTERVAL, INPUT_MAX_POLL),
        );
        app.mount(
            RealmId::Page,
            Box::new(PageComponent::new(app_state.clone())),
            Vec::new(),
        )?;
        app.mount(
            RealmId::Header,
            Box::new(HeaderComponent::new(app_state.clone())),
            Vec::new(),
        )?;
        app.mount(
            RealmId::Filters,
            Box::new(FiltersComponent::new(app_state.clone())),
            Vec::new(),
        )?;
        app.mount(
            RealmId::Body,
            Box::new(BodyComponent::new(app_state.clone())),
            Vec::new(),
        )?;
        app.mount(
            RealmId::CommandBar,
            Box::new(CommandBarComponent::new(app_state.clone())),
            Vec::new(),
        )?;
        app.mount(
            RealmId::Footer,
            Box::new(FooterComponent::new(app_state.clone())),
            Vec::new(),
        )?;
        app.mount(
            RealmId::Overlay,
            Box::new(OverlayComponent::new(app_state.clone())),
            Vec::new(),
        )?;
        app.mount(RealmId::Input, Box::<InputBridge>::default(), Vec::new())?;
        app.active(&RealmId::Input)?;
        Ok(Self { app })
    }

    pub fn tick(&mut self, timeout: Duration) -> ApplicationResult<Vec<TissueMsg>> {
        self.app.tick(PollStrategy::Once(timeout))
    }

    pub fn render(&mut self, app_state: &App, frame: &mut Frame, area: Rect) {
        self.sync_surfaces(app_state);

        let areas = ui::screen_areas(app_state, area);
        self.app.view(&RealmId::Page, frame, area);
        self.app.view(&RealmId::Header, frame, areas.header);
        self.app.view(&RealmId::Filters, frame, areas.filters);
        self.app.view(&RealmId::Body, frame, areas.body);
        if let Some(command_bar) = areas.command_bar {
            self.app.view(&RealmId::CommandBar, frame, command_bar);
        }
        self.app.view(&RealmId::Footer, frame, areas.footer);
        self.app.view(&RealmId::Overlay, frame, area);
    }

    fn sync_surfaces(&mut self, app_state: &App) {
        sync_component::<PageComponent>(&mut self.app, RealmId::Page, app_state);
        sync_component::<HeaderComponent>(&mut self.app, RealmId::Header, app_state);
        sync_component::<FiltersComponent>(&mut self.app, RealmId::Filters, app_state);
        sync_component::<BodyComponent>(&mut self.app, RealmId::Body, app_state);
        sync_component::<CommandBarComponent>(&mut self.app, RealmId::CommandBar, app_state);
        sync_component::<FooterComponent>(&mut self.app, RealmId::Footer, app_state);
        sync_component::<OverlayComponent>(&mut self.app, RealmId::Overlay, app_state);
    }
}

trait AppStateComponent {
    fn set_app_state(&mut self, app_state: App);
}

fn sync_component<T>(
    app: &mut Application<RealmId, TissueMsg, NoUserEvent>,
    id: RealmId,
    app_state: &App,
) where
    T: AppStateComponent + 'static,
{
    if let Some(component) = app.get_component_mut(&id)
        && let Some(surface) = component.as_any_mut().downcast_mut::<T>()
    {
        surface.set_app_state(app_state.clone());
    }
}

macro_rules! surface_component {
    ($name:ident, $render:path) => {
        struct $name {
            app_state: App,
        }

        impl $name {
            fn new(app_state: App) -> Self {
                Self { app_state }
            }
        }

        impl Component for $name {
            fn view(&mut self, frame: &mut Frame, area: Rect) {
                $render(&self.app_state, area, frame.buffer_mut());
            }

            fn query<'a>(&'a self, _attr: Attribute) -> Option<QueryResult<'a>> {
                None
            }

            fn attr(&mut self, _attr: Attribute, _value: AttrValue) {}

            fn state(&self) -> State {
                State::None
            }

            fn perform(&mut self, _cmd: Cmd) -> CmdResult {
                CmdResult::NoChange
            }
        }

        impl AppComponent<TissueMsg, NoUserEvent> for $name {
            fn on(&mut self, _ev: &Event<NoUserEvent>) -> Option<TissueMsg> {
                None
            }
        }

        impl AppStateComponent for $name {
            fn set_app_state(&mut self, app_state: App) {
                self.app_state = app_state;
            }
        }
    };
}

surface_component!(PageComponent, ui::render_page_background);
surface_component!(HeaderComponent, ui::render_header);
surface_component!(FiltersComponent, ui::render_filters);
surface_component!(BodyComponent, ui::render_body);
surface_component!(CommandBarComponent, ui::render_command_bar);
surface_component!(FooterComponent, ui::render_footer);
surface_component!(OverlayComponent, ui::render_overlay);

#[derive(Default)]
struct InputBridge;

impl Component for InputBridge {
    fn view(&mut self, _frame: &mut Frame, _area: Rect) {}

    fn query<'a>(&'a self, _attr: Attribute) -> Option<QueryResult<'a>> {
        None
    }

    fn attr(&mut self, _attr: Attribute, _value: AttrValue) {}

    fn state(&self) -> State {
        State::None
    }

    fn perform(&mut self, _cmd: Cmd) -> CmdResult {
        CmdResult::NoChange
    }
}

impl AppComponent<TissueMsg, NoUserEvent> for InputBridge {
    fn on(&mut self, ev: &Event<NoUserEvent>) -> Option<TissueMsg> {
        match ev {
            Event::Keyboard(key) => Some(TissueMsg::Key(crossterm_key_event(*key))),
            Event::Mouse(mouse) => Some(TissueMsg::Mouse(crossterm_mouse_event(*mouse))),
            Event::WindowResize(width, height) => Some(TissueMsg::Resize(*width, *height)),
            Event::Tick => Some(TissueMsg::Tick),
            Event::None
            | Event::FocusGained
            | Event::FocusLost
            | Event::Paste(_)
            | Event::User(_) => None,
        }
    }
}

fn crossterm_key_event(key: KeyEvent) -> CrosstermKeyEvent {
    CrosstermKeyEvent::new(
        crossterm_key_code(key.code),
        crossterm_key_modifiers(key.modifiers),
    )
}

fn crossterm_key_code(key: Key) -> KeyCode {
    match key {
        Key::Backspace => KeyCode::Backspace,
        Key::Enter => KeyCode::Enter,
        Key::Left | Key::ShiftLeft | Key::AltLeft | Key::CtrlLeft => KeyCode::Left,
        Key::Right | Key::ShiftRight | Key::AltRight | Key::CtrlRight => KeyCode::Right,
        Key::Up | Key::ShiftUp | Key::AltUp | Key::CtrlUp => KeyCode::Up,
        Key::Down | Key::ShiftDown | Key::AltDown | Key::CtrlDown => KeyCode::Down,
        Key::Home | Key::CtrlHome => KeyCode::Home,
        Key::End | Key::CtrlEnd => KeyCode::End,
        Key::PageUp => KeyCode::PageUp,
        Key::PageDown => KeyCode::PageDown,
        Key::Tab => KeyCode::Tab,
        Key::BackTab => KeyCode::BackTab,
        Key::Delete => KeyCode::Delete,
        Key::Insert => KeyCode::Insert,
        Key::Function(number) => KeyCode::F(number),
        Key::Char(character) => KeyCode::Char(character),
        Key::Esc => KeyCode::Esc,
        Key::CapsLock
        | Key::ScrollLock
        | Key::NumLock
        | Key::PrintScreen
        | Key::Pause
        | Key::Menu
        | Key::KeypadBegin
        | Key::Media(_)
        | Key::Null => KeyCode::Null,
    }
}

fn crossterm_key_modifiers(modifiers: KeyModifiers) -> CrosstermKeyModifiers {
    let mut crossterm_modifiers = CrosstermKeyModifiers::empty();
    if modifiers.contains(KeyModifiers::SHIFT) {
        crossterm_modifiers.insert(CrosstermKeyModifiers::SHIFT);
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        crossterm_modifiers.insert(CrosstermKeyModifiers::CONTROL);
    }
    if modifiers.contains(KeyModifiers::ALT) {
        crossterm_modifiers.insert(CrosstermKeyModifiers::ALT);
    }
    crossterm_modifiers
}

fn crossterm_mouse_event(mouse: MouseEvent) -> CrosstermMouseEvent {
    CrosstermMouseEvent {
        kind: crossterm_mouse_event_kind(mouse.kind),
        column: mouse.column,
        row: mouse.row,
        modifiers: crossterm_key_modifiers(mouse.modifiers),
    }
}

fn crossterm_mouse_event_kind(kind: MouseEventKind) -> CrosstermMouseEventKind {
    match kind {
        MouseEventKind::Down(button) => {
            CrosstermMouseEventKind::Down(crossterm_mouse_button(button))
        }
        MouseEventKind::Up(button) => CrosstermMouseEventKind::Up(crossterm_mouse_button(button)),
        MouseEventKind::Drag(button) => {
            CrosstermMouseEventKind::Drag(crossterm_mouse_button(button))
        }
        MouseEventKind::Moved => CrosstermMouseEventKind::Moved,
        MouseEventKind::ScrollDown => CrosstermMouseEventKind::ScrollDown,
        MouseEventKind::ScrollUp => CrosstermMouseEventKind::ScrollUp,
        MouseEventKind::ScrollLeft => CrosstermMouseEventKind::ScrollLeft,
        MouseEventKind::ScrollRight => CrosstermMouseEventKind::ScrollRight,
    }
}

fn crossterm_mouse_button(button: RealmMouseButton) -> MouseButton {
    match button {
        RealmMouseButton::Left => MouseButton::Left,
        RealmMouseButton::Right => MouseButton::Right,
        RealmMouseButton::Middle => MouseButton::Middle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_keyboard_events_into_existing_crossterm_handlers() {
        let event = crossterm_key_event(KeyEvent::new(
            Key::Char('s'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ));

        assert_eq!(event.code, KeyCode::Char('s'));
        assert!(event.modifiers.contains(CrosstermKeyModifiers::CONTROL));
        assert!(event.modifiers.contains(CrosstermKeyModifiers::SHIFT));
    }

    #[test]
    fn maps_mouse_events_into_existing_crossterm_handlers() {
        let event = crossterm_mouse_event(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            modifiers: KeyModifiers::ALT,
            column: 8,
            row: 13,
        });

        assert_eq!(event.kind, CrosstermMouseEventKind::ScrollDown);
        assert_eq!(event.column, 8);
        assert_eq!(event.row, 13);
        assert!(event.modifiers.contains(CrosstermKeyModifiers::ALT));
    }

    #[test]
    fn realm_mounts_named_components_without_generic_surface_bridge() {
        let realm_source = include_str!("realm.rs");
        let ui_source = include_str!("ui.rs");

        assert!(!realm_source.contains(concat!("Surface", "Bridge")));
        assert!(!ui_source.contains(concat!("pub fn ", "render(app: &App")));
        assert!(realm_source.contains("struct HeaderComponent"));
        assert!(realm_source.contains("struct BodyComponent"));
        assert!(realm_source.contains("struct OverlayComponent"));
    }
}
