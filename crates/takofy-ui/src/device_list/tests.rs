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
