//! Pointer events shared by the views' test harnesses.
//!
//! One click is the same sequence everywhere — the pointer arrives, presses and
//! releases — and egui wants a pass per event, so this is the one place the
//! sequence is written down. A drag is the same sequence with travel in it.

use egui::{Event, Modifiers, PointerButton, Pos2};

/// How far a drag's pointer travels between one pass and the next, in points.
const DRAG_STEP: f32 = 8.0;

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

/// One press or release of the secondary button at `pos`.
pub fn secondary_button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: PointerButton::Secondary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

/// The passes one right-click takes, in order: the pointer arrives, presses and
/// releases the secondary button.
pub fn secondary_click_events(pos: Pos2) -> [Event; 3] {
    [
        Event::PointerMoved(pos),
        secondary_button(pos, true),
        secondary_button(pos, false),
    ]
}

/// The passes one drag takes, in order: the pointer arrives and presses, walks
/// to `to` in steps, and releases there.
///
/// The steps matter: a drag begins only once the pointer has moved further than
/// egui's click threshold, so a press and a release at two positions in a row
/// would read as a click.
pub fn drag_events(from: Pos2, to: Pos2) -> Vec<Event> {
    let mut events = vec![Event::PointerMoved(from), button(from, true)];
    events.extend(steps(from, to).map(Event::PointerMoved));
    events.push(button(to, false));
    events
}

/// The positions a drag visits between `from` and `to`, never repeating `from`
/// and always ending at `to`.
fn steps(from: Pos2, to: Pos2) -> impl Iterator<Item = Pos2> {
    let travel = to - from;
    let count = ((travel.length() / DRAG_STEP).ceil() as usize).max(1);
    (1..=count).map(move |step| from + travel * (step as f32 / count as f32))
}
