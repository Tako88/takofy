//! Whether focus is visible: the accent ring follows the keyboard, not the pointer.
//!
//! egui draws no `:focus-visible`, so one context-wide value remembers whether
//! the last input was a key and whether the ring was showing. Every row and
//! control asks the same question, and only the widget that holds focus answers
//! it, so a list of rows costs one comparison each.

use egui::{Context, Event, Id};

/// The frame's last input that decides whether the keyboard is in use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Input {
    /// A pointer button went down: the pointer is in use.
    Pointer,
    /// A key went down, or assistive technology asked for an action.
    Key,
}

/// Which widget holds focus, what last moved it, and whether its ring is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Seen {
    focused: Option<Id>,
    keyboard: bool,
    visible: bool,
}

impl Default for Seen {
    fn default() -> Self {
        // A fresh window has had no pointer input, so programmatic focus shows
        // its ring the way a browser does.
        Self {
            focused: None,
            keyboard: true,
            visible: false,
        }
    }
}

impl Seen {
    /// Folds one frame's input and focus into the remembered state.
    ///
    /// A press hides the ring at once, even on the widget already focused. A key
    /// only says the keyboard is in use; it never reveals a ring on its own. The
    /// ring follows the focus move that comes after it.
    pub(crate) fn advance(&mut self, input: Option<Input>, focused: Option<Id>) {
        match input {
            Some(Input::Pointer) => {
                self.keyboard = false;
                self.visible = false;
            }
            Some(Input::Key) => self.keyboard = true,
            None => {}
        }
        if focused != self.focused {
            self.focused = focused;
            self.visible = self.keyboard && focused.is_some();
        }
    }
}

/// The frame's last input that matters, or `None` when it had none.
///
/// Releases, moves, scrolls and modifier changes are not input: they neither
/// hide a ring nor reveal one.
pub(crate) fn last_input(events: &[Event]) -> Option<Input> {
    events.iter().rev().find_map(|event| match event {
        Event::PointerButton { pressed: true, .. } => Some(Input::Pointer),
        Event::Key { pressed: true, .. } | Event::AccessKitActionRequest(_) => Some(Input::Key),
        _ => None,
    })
}

/// The id the remembered state lives under.
fn state_id() -> Id {
    Id::unique("focus-visible")
}

/// Whether `id`'s ring is showing this frame.
///
/// An unfocused widget is answered by one comparison and reads no input, so the
/// rows a frame did not focus cost nothing. Only the focused widget advances the
/// state, which is what lets a view drawn without [`begin_frame`] stay in step.
pub(crate) fn visible(ctx: &Context, id: Id) -> bool {
    if ctx.memory(|memory| memory.focused()) != Some(id) {
        return false;
    }
    let input = ctx.input(|input| last_input(&input.events));
    ctx.data_mut(|data| {
        let seen = data.get_temp_mut_or_default::<Seen>(state_id());
        seen.advance(input, Some(id));
        seen.visible
    })
}

/// Reads the frame's input and focus before any view consumes the events.
///
/// A key can be taken out of `input.events` mid-frame, so the frame's own key
/// press has to be seen here. The advance is idempotent, which is why both draw
/// passes call it.
pub(crate) fn begin_frame(ctx: &Context) {
    let focused = ctx.memory(|memory| memory.focused());
    let input = ctx.input(|input| last_input(&input.events));
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Seen>(state_id())
            .advance(input, focused);
    });
}

/// The state a frame left, so a test can prove an unfocused widget wrote nothing.
#[cfg(test)]
fn peek(ctx: &Context) -> Option<Seen> {
    ctx.data(|data| data.get_temp::<Seen>(state_id()))
}

#[cfg(test)]
mod tests;
