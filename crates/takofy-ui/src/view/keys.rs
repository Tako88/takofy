//! The keys a text field owns, and the filter that keeps egui's own handlers
//! off them.
//!
//! Two fields in the window are real text edits — the topbar's search and the
//! library head's name — and both need the same rules: an unmodified key is read
//! with the same match it is consumed with, and a held key does not act twice.

use egui::{Event, InputState, Key};

/// The text edit's own key filter: egui's defaults, plus Escape.
///
/// egui's memory surrenders focus on Escape before the view sees it, so without
/// this a field would blur before Escape could cancel anything. The two arrow
/// flags are the text edit's defaults and must stay: they keep arrow presses out
/// of spatial focus movement.
///
/// It is one constant because focus is taken more than one way — the text edit's
/// own `show`, and a hand-back that calls `request_focus` — and `request_focus`
/// stores a default filter, so every path has to store this one after it.
pub(crate) const FIELD_FILTER: egui::EventFilter = egui::EventFilter {
    tab: false,
    horizontal_arrows: true,
    vertical_arrows: true,
    escape: true,
};

/// Any press of `key` with no modifiers, auto-repeat included.
///
/// The field reads and consumes with the same rule: `consume_key` matches a key
/// whose modifiers match logically, which would eat an Alt+Down the read never
/// saw.
pub(crate) fn pressed(input: &InputState, key: Key) -> bool {
    input.events.iter().any(|event| {
        matches!(
            event,
            Event::Key {
                key: pressed,
                pressed: true,
                modifiers,
                ..
            } if *pressed == key && !modifiers.any()
        )
    })
}

/// A first press of `key` with no modifiers, never an auto-repeat.
///
/// A held Enter must not replay an action, and a held Escape must not step
/// through two: one press, one action.
pub(crate) fn pressed_once(input: &InputState, key: Key) -> bool {
    input.events.iter().any(|event| {
        matches!(
            event,
            Event::Key {
                key: pressed,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } if *pressed == key && !modifiers.any()
        )
    })
}

#[cfg(test)]
mod tests;
