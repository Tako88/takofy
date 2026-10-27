use egui::{Event, Modifiers};

use super::super::*;
use super::test_support::*;

#[test]
fn a_right_click_reports_the_rows_door() {
    let ctx = context();
    let queue = queue_of(6, 1);
    let at = row_at(&ctx, &queue, "t4");

    let reply = feed_doors(
        &ctx,
        PANE,
        &queue,
        crate::view::test_support::secondary_click_events(at).to_vec(),
        Reply::default(),
    );

    let (row, door) = reply.menu.expect("a right-click is a door");
    assert_eq!(row, 4);
    assert_eq!(door.device, crate::menu::Device::Pointer);
    assert!(
        matches!(door.anchor, Anchor::Pointer(_)),
        "the menu did not anchor where the click was"
    );
    assert_eq!(reply.press, None, "the door also picked the row");
}

#[test]
fn shift_f10_and_the_menu_key_are_the_focused_rows_door() {
    for event in [
        Event::Key {
            key: egui::Key::F10,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::SHIFT,
        },
        Event::Key {
            key: egui::Key::Menu,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        },
    ] {
        let ctx = context();
        let queue = queue_of(6, 1);
        let at = row_at(&ctx, &queue, "t4");
        let _ = click(&ctx, PANE, &queue, at);

        let reply = feed_doors(&ctx, PANE, &queue, vec![event], Reply::default());

        let (row, door) = reply.menu.expect("the key opened no door");
        assert_eq!(row, 4);
        assert_eq!(door.device, crate::menu::Device::Keyboard);
        assert!(
            matches!(door.anchor, Anchor::Button(_)),
            "the keyboard's menu floated at the pointer"
        );
    }
}

#[test]
fn a_key_door_without_focus_opens_nothing() {
    let ctx = context();
    let queue = queue_of(6, 1);

    let reply = feed_doors(
        &ctx,
        PANE,
        &queue,
        vec![Event::Key {
            key: egui::Key::Menu,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
        Reply::default(),
    );

    assert!(reply.menu.is_none());
}

#[test]
fn the_queues_own_rows_end_at_the_current_entry_so_it_has_no_door() {
    let ctx = context();
    let queue = queue_of(6, 2);
    let mut art = Art::default();
    let now = Instant::now();
    let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
    assert!(
        !painted_rows(&output).contains(&2),
        "the current entry is drawn as a row"
    );

    // A right-click on the caption band names no row.
    let at = painted_at(&output, "NEXT IN QUEUE").expect("the caption is painted");
    let reply = feed_doors(
        &ctx,
        PANE,
        &queue,
        crate::view::test_support::secondary_click_events(at + Vec2::new(4.0, 4.0)).to_vec(),
        Reply::default(),
    );
    assert!(reply.menu.is_none(), "the caption opened a menu");
}
