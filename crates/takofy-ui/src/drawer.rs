//! The rail's drawer as a small state machine.
//!
//! The view feeds it what happened this frame and draws the state it returns.
//! The drawer exists only while the sidebar is a rail, and picking a playlist is
//! a close: the row's job is to put a playlist on screen, and the drawer is
//! covering the screen.

/// Whether the rail's overflow panel is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Drawer {
    #[default]
    Closed,
    Open,
}

/// What reached the drawer this frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Input {
    /// Whether the sidebar is currently the rail, and so has a toggle.
    pub rail: bool,
    /// The toggle was activated, by pointer or keyboard.
    pub toggle: bool,
    /// Escape was pressed.
    pub escape: bool,
    /// A click landed outside the drawer.
    pub clicked_outside: bool,
    /// The user picked a playlist.
    pub selected: bool,
}

impl Drawer {
    /// The next state.
    ///
    /// The toggle wins over everything else in the same frame, because a
    /// keyboard activation can reach it while the drawer covers the pointer.
    pub fn step(self, input: Input) -> Self {
        if !input.rail {
            return Self::Closed;
        }
        if input.toggle {
            return match self {
                Self::Closed => Self::Open,
                Self::Open => Self::Closed,
            };
        }
        if self == Self::Open && (input.escape || input.clicked_outside || input.selected) {
            return Self::Closed;
        }
        self
    }
}

#[cfg(test)]
mod tests;
