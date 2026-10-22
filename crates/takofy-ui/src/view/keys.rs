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
mod tests {
    use super::{pressed, pressed_once};
    use egui::{Context, Event, Key, Modifiers, RawInput};

    /// One frame of `events` on `ctx`, answering what both readers say for
    /// `key`.
    fn frame(ctx: &Context, events: Vec<Event>, key: Key) -> (bool, bool) {
        let input = RawInput {
            events,
            ..Default::default()
        };
        let mut answers = (false, false);
        let mut output = ctx.run_ui(input, |ui| {
            ui.input(|state| answers = (pressed(state, key), pressed_once(state, key)));
        });
        output.textures_delta.clear();
        answers
    }

    fn press(key: Key, repeat: bool, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat,
            modifiers,
        }
    }

    #[test]
    fn a_repeat_counts_as_a_press_but_not_as_a_first_one() {
        let ctx = Context::default();
        assert_eq!(
            frame(
                &ctx,
                vec![press(Key::Enter, false, Modifiers::NONE)],
                Key::Enter
            ),
            (true, true),
            "the first press is a first press"
        );
        // egui decides a repeat against the pass before it, so the held key is
        // only a repeat once the key is already down.
        assert_eq!(
            frame(
                &ctx,
                vec![press(Key::Enter, true, Modifiers::NONE)],
                Key::Enter
            ),
            (true, false),
            "a held key acts once"
        );
    }

    #[test]
    fn a_modified_key_is_somebody_elses() {
        let ctx = Context::default();
        assert_eq!(
            frame(
                &ctx,
                vec![press(Key::Escape, false, Modifiers::ALT)],
                Key::Escape
            ),
            (false, false)
        );
    }

    #[test]
    fn a_release_is_not_a_press() {
        let ctx = Context::default();
        let release = Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        assert_eq!(frame(&ctx, vec![release], Key::Enter), (false, false));
    }
}
