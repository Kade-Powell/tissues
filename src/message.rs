use crossterm::event::{KeyEvent, MouseEvent};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TissueMsg {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    Tick,
}
