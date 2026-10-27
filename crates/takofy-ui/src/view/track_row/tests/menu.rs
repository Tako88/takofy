use egui::{Event, Key, Modifiers};

use super::super::*;
use super::test_support::*;
use crate::theme::Tokens;
use crate::view::test_support::{click_events, secondary_click_events};

/// A playlist row whose menu may open.
fn menu_row() -> Row<'static> {
    Row {
        menu: true,
        ..row_of(1)
    }
}

#[test]
fn the_more_button_opens_a_button_door() {
    let ctx = context();
    let row = menu_row();
    let answers = run(
        &ctx,
        &row,
        &click_events(actions().center()),
        Modifiers::NONE,
    );

    let door = door(&answers).expect("the more opened a menu");
    assert_eq!(door.anchor, Anchor::Button(actions()));
    assert_eq!(door.device, Device::Pointer);
    assert!(
        !answers
            .iter()
            .flatten()
            .any(|click| matches!(click, Click::Select(_) | Click::Play)),
        "the press on the more also picked or played the row: {answers:?}"
    );
}

#[test]
fn a_right_click_on_the_body_a_link_cell_and_the_index_opens_at_the_pointer() {
    let cells = columns().at(body_rect().min);
    let anchor = context();
    let anchor_row = menu_row();
    let artist = words(&anchor, &anchor_row, "Slow Tide");
    let album = words(&anchor, &anchor_row, "Record");
    let points = [
        body_point(),
        artist.center(),
        space_after(cells.artist, artist),
        album.center(),
        space_after(cells.album, album),
        number_box(cells.index).center(),
    ];
    for point in points {
        let ctx = context();
        let row = menu_row();
        let answers = run(&ctx, &row, &secondary_click_events(point), Modifiers::NONE);

        let door = door(&answers).unwrap_or_else(|| panic!("no menu at {point:?}"));
        let Anchor::Pointer(at) = door.anchor else {
            panic!(
                "a right-click at {point:?} anchored to the `⋯`: {:?}",
                door.anchor
            );
        };
        assert!(
            (at - point).length() < 0.01,
            "a right-click at {point:?} opened at {at:?}"
        );
        assert_eq!(door.device, Device::Pointer, "at {point:?}");
        assert!(
            !answers
                .iter()
                .flatten()
                .any(|click| matches!(click, Click::Select(_) | Click::Open(_) | Click::Play)),
            "a right-click at {point:?} did more than open the menu: {answers:?}"
        );
    }
}

#[test]
fn a_pointer_click_on_the_open_more_posts_no_door_and_keeps_the_keyboard() {
    let ctx = context();
    let row = menu_row();
    let key = (row.uri, row.number);
    let mut more_id = None;
    let mut output = ctx.run_ui(input_with(Vec::new()), |ui| {
        more_id = Some(controls::slot_id(ui, Control::TrackMore, key));
        let _ = show(ui, body_rect(), columns(), &row, &Tokens::DARK);
    });
    output.textures_delta.clear();
    let id = more_id.expect("a frame ran");

    // The menu is up, and this `⋯` is the one that opened it.
    context_menu::note_open(&ctx, Some(id));
    let answers = run(
        &ctx,
        &row,
        &click_events(actions().center()),
        Modifiers::NONE,
    );
    assert_eq!(
        door(&answers),
        None,
        "the open `⋯` posted a fresh request instead of closing"
    );

    let mut focused = false;
    let mut output = ctx.run_ui(input_with(Vec::new()), |ui| {
        let _ = show(ui, body_rect(), columns(), &row, &Tokens::DARK);
        focused = ui.memory(|memory| memory.has_focus(id));
    });
    output.textures_delta.clear();
    assert!(focused, "the keyboard did not come back to the `⋯`");
}

#[test]
fn a_held_enter_does_not_reopen_the_menu_from_the_more() {
    let ctx = context();
    let row = menu_row();
    // The click gives the `⋯` the keyboard, as a pointer does.
    let _ = run(
        &ctx,
        &row,
        &click_events(actions().center()),
        Modifiers::NONE,
    );

    let press = Event::Key {
        key: Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    let first = run(&ctx, &row, std::slice::from_ref(&press), Modifiers::NONE);
    assert!(
        door(&first).is_some(),
        "a fresh Enter on the `⋯` opened nothing"
    );

    // Still held: egui marks a press with no release before it a repeat.
    let held = run(&ctx, &row, &[press], Modifiers::NONE);
    assert_eq!(door(&held), None, "a held Enter reopened the menu");
    assert!(
        held.iter().flatten().next().is_none(),
        "a held Enter reported {held:?}"
    );
}

#[test]
fn shift_f10_and_the_menu_key_open_on_a_focused_row_but_plain_f10_does_not() {
    let ctx = context();
    let row = menu_row();
    // The click is what gives the row the keyboard.
    let first = run(&ctx, &row, &click_events(body_point()), Modifiers::NONE);
    assert!(
        first
            .iter()
            .flatten()
            .any(|click| matches!(click, Click::Select(_))),
        "the row never took the keyboard: {first:?}"
    );

    let press = |key, modifiers| Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };

    let plain = run(
        &ctx,
        &row,
        &[press(Key::F10, Modifiers::NONE)],
        Modifiers::NONE,
    );
    assert_eq!(door(&plain), None, "plain F10 opened the menu");

    let shift = run(
        &ctx,
        &row,
        &[press(Key::F10, Modifiers::SHIFT)],
        Modifiers::SHIFT,
    );
    let shift = door(&shift).expect("Shift+F10 opened no menu");
    assert_eq!(shift.device, Device::Keyboard);
    assert_eq!(shift.anchor, Anchor::Button(actions()));

    let menu_key = run(
        &ctx,
        &row,
        &[press(Key::Menu, Modifiers::NONE)],
        Modifiers::NONE,
    );
    assert_eq!(
        door(&menu_key).map(|door| door.device),
        Some(Device::Keyboard),
        "the context-menu key opened no menu"
    );
}

#[test]
fn enter_on_a_focused_more_opens_with_the_keyboard_device() {
    let ctx = context();
    let row = menu_row();
    let at = actions().center();
    let first = run(&ctx, &row, &click_events(at), Modifiers::NONE);
    assert_eq!(
        door(&first).map(|door| door.device),
        Some(Device::Pointer),
        "the click did not open the menu"
    );

    let enter = |pressed| Event::Key {
        key: Key::Enter,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    let answers = run(&ctx, &row, &[enter(true), enter(false)], Modifiers::NONE);
    assert_eq!(
        door(&answers).map(|door| door.device),
        Some(Device::Keyboard),
        "Enter on the focused more did not open the menu: {answers:?}"
    );
}

#[test]
fn a_waiting_more_and_a_carried_row_open_nothing() {
    let ctx = context();
    // The page's gate is shut: the more is a waiting control.
    let waiting = row_of(1);
    for events in [
        click_events(actions().center()).to_vec(),
        secondary_click_events(actions().center()).to_vec(),
        secondary_click_events(body_point()).to_vec(),
    ] {
        let answers = run(&ctx, &waiting, &events, Modifiers::NONE);
        assert_eq!(door(&answers), None, "a waiting row opened a menu");
    }

    // A carried row is being dragged, so nothing it says is a door.
    let carried = Row {
        look: Look {
            carrying: true,
            ..Look::default()
        },
        ..menu_row()
    };
    let answers = run(
        &ctx,
        &carried,
        &secondary_click_events(body_point()),
        Modifiers::NONE,
    );
    assert_eq!(door(&answers), None, "a carried row opened a menu");
}

#[test]
fn a_right_click_under_another_layer_is_ignored() {
    let ctx = context();
    let row = menu_row();
    let point = body_point();
    let cover = Rect::from_min_size(point - vec2(40.0, 20.0), vec2(200.0, 40.0));

    let answer = |events: Vec<Event>| {
        let mut answer = None;
        let mut output = ctx.run_ui(input_with(events), |ui| {
            egui::Area::new(egui::Id::unique("cover"))
                .order(egui::Order::Middle)
                .fixed_pos(cover.min)
                .show(ui.ctx(), |ui| {
                    ui.expand_to_include_rect(cover);
                    ui.painter()
                        .rect_filled(cover, 0.0, Tokens::DARK.palette.surface);
                });
            answer = show(ui, body_rect(), columns(), &row, &Tokens::DARK);
        });
        output.textures_delta.clear();
        answer
    };

    // Two frames, so the cover's own rect is known to the layer test.
    let _ = answer(Vec::new());
    let _ = answer(Vec::new());
    let answers: Vec<Option<Click>> = secondary_click_events(point)
        .into_iter()
        .map(|event| answer(vec![event]))
        .collect();
    assert_eq!(
        door(&answers),
        None,
        "a right-click under another layer opened the row's menu"
    );
}

#[test]
fn the_open_more_button_is_lit_and_announces_expanded() {
    let ctx = context();
    // The node builders only exist with AccessKit on.
    ctx.enable_accesskit();
    let row = menu_row();
    let key = (row.uri, row.number);
    let mut more_id = None;
    let mut output = ctx.run_ui(
        input_with(vec![Event::PointerMoved(Pos2::new(899.0, 799.0))]),
        |ui| {
            more_id = Some(controls::slot_id(ui, Control::TrackMore, key));
            let _ = show(ui, body_rect(), columns(), &row, &Tokens::DARK);
        },
    );
    output.textures_delta.clear();
    let id = more_id.expect("a frame ran");
    assert!(
        !glyph_colors(&output, actions()).contains(&Tokens::DARK.palette.text),
        "a resting more was drawn open"
    );

    // The menu stores its opener; the row draws before the menu, so it reads
    // the frame before. The node is read in the frame that built it.
    context_menu::note_open(&ctx, Some(id));
    let mut nodes = None;
    let mut output = ctx.run_ui(input_with(Vec::new()), |ui| {
        let _ = show(ui, body_rect(), columns(), &row, &Tokens::DARK);
        nodes = ui
            .ctx()
            .accesskit_node_builder(id, |node| (node.is_expanded(), node.has_popup()));
    });
    output.textures_delta.clear();
    assert!(
        glyph_colors(&output, actions()).contains(&Tokens::DARK.palette.text),
        "the open more is not in full ink: {:?}",
        glyph_colors(&output, actions())
    );

    let (expanded, popup) = nodes.expect("the more has an AccessKit node");
    assert_eq!(expanded, Some(true), "the open more does not say so");
    assert_eq!(popup, Some(egui::accesskit::HasPopup::Menu));
}

#[test]
fn the_more_button_is_a_tab_stop_only_when_live() {
    for (menu, focusable) in [(true, true), (false, false)] {
        let ctx = context();
        let row = Row { menu, ..row_of(1) };
        let key = (row.uri, row.number);
        let mut sense = None;
        let mut output = ctx.run_ui(input_with(Vec::new()), |ui| {
            let _ = show(ui, body_rect(), columns(), &row, &Tokens::DARK);
            sense = ui
                .ctx()
                .read_response(controls::slot_id(ui, Control::TrackMore, key))
                .map(|response| response.sense);
        });
        output.textures_delta.clear();
        let sense = sense.expect("the more is drawn");
        assert_eq!(sense.is_focusable(), focusable, "menu: {menu}");
        assert_eq!(sense.senses_click(), focusable, "menu: {menu}");
    }
}
