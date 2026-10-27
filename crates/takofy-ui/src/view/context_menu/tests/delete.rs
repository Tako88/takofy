use super::super::*;
use super::test_support::*;
use crate::menu::{Device, Item, Kind, Listing, Verb};
use egui::{Event, Key, Modifiers};

/// How many Delete presses the frame still has after the menu read it.
fn deletes(ui: &egui::Ui) -> usize {
    ui.input(|input| {
        input
            .events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    Event::Key {
                        key: Key::Delete,
                        pressed: true,
                        ..
                    }
                )
            })
            .count()
    })
}

fn destructive_item(label: &str) -> Item {
    Item {
        label: label.to_owned(),
        kind: Kind::Destructive,
        key: Some("Del"),
        submenu: false,
        verb: Verb::Remove,
    }
}

/// The four page verbs, plus the destructive one last.
fn removable() -> Vec<Item> {
    let mut items = items();
    items.push(destructive_item("Remove from this playlist"));
    items
}

#[test]
fn delete_chooses_the_destructive_row_and_takes_the_key() {
    let ctx = context();
    let mut menu = opened_with(Device::Pointer, removable());
    warm(&ctx, &mut menu);
    let (drawn, left) = draw(
        &ctx,
        &mut menu,
        true,
        vec![key_event(Key::Delete, Modifiers::NONE)],
        |ui, _| deletes(ui),
    );
    assert_eq!(
        drawn.closed.and_then(|closed| closed.chose),
        Some(Verb::Remove),
        "Del did not choose the destructive row"
    );
    assert_eq!(left, 0, "the Del reached the region behind the menu");
    assert!(!menu.is_open());
}

#[test]
fn delete_in_a_menu_without_a_removal_does_nothing() {
    let ctx = context();
    let mut menu = opened(Device::Pointer);
    warm(&ctx, &mut menu);
    let (drawn, left) = draw(
        &ctx,
        &mut menu,
        true,
        vec![key_event(Key::Delete, Modifiers::NONE)],
        |ui, _| deletes(ui),
    );
    assert_eq!(drawn.closed, None, "a menu with no removal chose one");
    assert!(menu.is_open(), "a menu with no removal closed on Del");
    assert_eq!(left, 0, "the menu left the key for the page behind it");
}

#[test]
fn a_modified_delete_is_not_the_menus_and_a_repeat_chooses_nothing() {
    let ctx = context();
    let mut menu = opened_with(Device::Pointer, removable());
    warm(&ctx, &mut menu);
    let shifted = Event::Key {
        key: Key::Delete,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::SHIFT,
    };
    let (drawn, left) = draw(&ctx, &mut menu, true, vec![shifted], |ui, _| deletes(ui));
    assert_eq!(drawn.closed, None, "Shift+Del chose a row");
    assert!(menu.is_open());
    assert_eq!(left, 1, "a modified Del was taken from its own owner");

    let mut menu = opened_with(Device::Pointer, removable());
    warm(&ctx, &mut menu);
    let (drawn, left) = draw(&ctx, &mut menu, true, vec![repeat(Key::Delete)], |ui, _| {
        deletes(ui)
    });
    assert_eq!(drawn.closed, None, "a held Del chose twice");
    assert!(menu.is_open(), "a held Del closed the menu");
    assert_eq!(left, 0, "a held Del leaked past the menu");
}
#[test]
fn a_posted_cut_is_taken_once() {
    let ctx = context();
    let cut = Cut {
        listing: Listing::Queue,
        rows: vec![1],
        entries: vec![track()],
    };
    post_cut(&ctx, cut.clone());
    assert_eq!(take_cut(&ctx), Some(cut));
    assert_eq!(take_cut(&ctx), None, "a cut is read once");
}
