//! Pointer events shared by the views' test harnesses.
//!
//! One click is the same sequence everywhere — the pointer arrives, presses and
//! releases — and egui wants a pass per event, so this is the one place the
//! sequence is written down.

use egui::{Event, Modifiers, PointerButton, Pos2};

/// One press or release of the primary button at `pos`.
pub fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

/// The passes one click takes, in order: the pointer arrives, presses, releases.
pub fn click_events(pos: Pos2) -> [Event; 3] {
    [
        Event::PointerMoved(pos),
        button(pos, true),
        button(pos, false),
    ]
}
