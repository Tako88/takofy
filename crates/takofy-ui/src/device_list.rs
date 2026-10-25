//! Whether the device list is open, above which button, and who gets the
//! keyboard when it closes.
//!
//! A row that can move the sound is the card's one control, so a pick closes it
//! with no focus of ours to hand back. What it lists is
//! [`crate::devices::Devices`]'.

use egui::Id;

use crate::menu::{Anchor, Reason};

/// The card a press on the devices button opens.
#[derive(Debug, Clone, Default)]
pub struct DeviceList {
    open: bool,
    anchor: Option<Anchor>,
    /// The button that opened it, which the keyboard goes back to.
    opener: Option<Id>,
    /// The frame the open card first painted on, so the click that opened it is
    /// not also a click outside it.
    first_drawn: Option<f64>,
}

impl DeviceList {
    /// A press on the devices button: opens the card, or closes it when the same
    /// button opened it. Reports whether it is now open.
    pub fn press(&mut self, anchor: Anchor, opener: Id) -> bool {
        if self.open && self.opener == Some(opener) {
            self.close(Reason::Outside);
            return false;
        }
        self.open = true;
        self.anchor = Some(anchor);
        self.opener = Some(opener);
        self.first_drawn = None;
        true
    }

    /// Closes the card. Escape reports the button the keyboard goes back to; a
    /// close the pointer caused has no focus of ours to return.
    pub fn close(&mut self, reason: Reason) -> Option<Id> {
        if !self.open {
            return None;
        }
        self.open = false;
        self.anchor = None;
        let opener = self.opener.take();
        match reason {
            Reason::Escape => opener,
            _ => None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether `now` is still the frame the card first drew on, where the press
    /// that opened it is not yet a press outside it.
    pub fn just_opened(&self, now: f64) -> bool {
        self.first_drawn.is_none_or(|drawn| now <= drawn)
    }

    /// Marks `now` as the frame the card drew on. The pass egui repeats within
    /// that frame shares the clock, so the opening is one frame however many
    /// times it is drawn.
    pub fn mark_drawn(&mut self, now: f64) {
        self.first_drawn.get_or_insert(now);
    }

    /// Where the card hangs from, while it is open.
    pub fn anchor(&self) -> Option<Anchor> {
        self.anchor
    }
}

#[cfg(test)]
mod tests {
    use egui::{Pos2, Rect, vec2};

    use super::*;

    fn anchor() -> Anchor {
        Anchor::Button(Rect::from_min_size(Pos2::ZERO, vec2(28.0, 28.0)))
    }

    fn opener() -> Id {
        Id::unique("devices")
    }

    #[test]
    fn a_press_opens_and_the_same_press_again_closes() {
        let mut list = DeviceList::default();
        assert!(!list.is_open());
        assert!(list.press(anchor(), opener()));
        assert!(list.is_open());
        assert_eq!(list.anchor(), Some(anchor()));
        assert!(!list.press(anchor(), opener()));
        assert!(!list.is_open());
    }

    #[test]
    fn escape_hands_the_keyboard_back_and_a_pointer_close_does_not() {
        let mut list = DeviceList::default();
        list.press(anchor(), opener());
        assert_eq!(list.close(Reason::Escape), Some(opener()));
        assert!(!list.is_open());

        list.press(anchor(), opener());
        assert_eq!(list.close(Reason::Outside), None);
        assert!(!list.is_open());
    }

    #[test]
    fn closing_a_closed_list_does_nothing() {
        let mut list = DeviceList::default();
        for reason in [Reason::Escape, Reason::Outside, Reason::Tab, Reason::Page] {
            assert_eq!(list.close(reason), None, "{reason:?}");
        }
        assert!(!list.is_open());
    }

    #[test]
    fn the_frame_that_opened_it_is_not_a_press_outside() {
        let mut list = DeviceList::default();
        list.press(anchor(), opener());
        assert!(list.just_opened(1.0), "nothing has drawn it yet");
        list.mark_drawn(1.0);
        assert!(list.just_opened(1.0), "a second pass of the same frame");
        assert!(
            !list.just_opened(1.016),
            "a later frame is not the opening, or its own press would close it"
        );

        list.close(Reason::Outside);
        list.press(anchor(), opener());
        assert!(list.just_opened(2.0), "a reopen is an opening of its own");
    }

    #[test]
    fn reopening_replaces_the_anchor() {
        let mut list = DeviceList::default();
        list.press(anchor(), opener());
        let other = Anchor::Button(Rect::from_min_size(Pos2::new(40.0, 60.0), vec2(28.0, 28.0)));
        let second = Id::unique("devices-elsewhere");
        assert!(list.press(other, second));
        assert_eq!(list.anchor(), Some(other));
        assert_eq!(list.close(Reason::Escape), Some(second));
    }
}
