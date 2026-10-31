use super::super::*;
use super::test_support::*;
use crate::layout::search_menu::row_rect;
use crate::menu::{Device, Verb};
use egui::{Key, Modifiers};

#[test]
fn down_lights_the_next_row() {
    let ctx = context();
    let mut menu = opened(Device::Keyboard);
    warm(&ctx, &mut menu);
    press(&ctx, &mut menu, Key::ArrowDown);
    assert_eq!(menu.lit(), Some(1));

    let drawn = frame(&ctx, &mut menu, true, Vec::new());
    let card = card(&drawn.output);
    assert_eq!(
        fills_of(&drawn.output, Tokens::DARK.palette.row_selected),
        vec![row_rect(card, 1)],
        "the wash did not follow the lit row"
    );
}

#[test]
fn enter_chooses_it() {
    let ctx = context();
    let mut menu = opened(Device::Keyboard);
    warm(&ctx, &mut menu);
    press(&ctx, &mut menu, Key::ArrowDown);
    let (drawn, focused) = draw(
        &ctx,
        &mut menu,
        true,
        vec![key_event(Key::Enter, Modifiers::NONE)],
        |ui, _| ui.memory(|memory| memory.has_focus(opener())),
    );
    assert_eq!(
        drawn.closed,
        Some(Closed {
            opener: Some(opener()),
            chose: Some(Verb::StartRadio),
        })
    );
    assert!(!menu.is_open());
    assert!(focused, "the keyboard did not go back to the button");
}

#[test]
fn the_enter_that_opens_leaves_the_menu_open() {
    let ctx = context();
    // The first open sizes the area. The second is the one a user makes, and
    // egui fakes a primary click for Enter, so the same keystroke that
    // opened the menu must not fire row 0.
    let mut menu = opened(Device::Keyboard);
    warm(&ctx, &mut menu);
    let _ = menu.close(Reason::Escape);

    let mut menu = opened(Device::Keyboard);
    // The opening Enter comes with its release: egui marks a press with no
    // release between it and the one before as an auto-repeat.
    let drawn = press(&ctx, &mut menu, Key::Enter);
    assert_eq!(drawn.closed, None, "the opening Enter fired a verb");
    assert!(menu.is_open(), "the opening Enter closed the menu");

    // The next Enter is the user's, and fires.
    let drawn = press(&ctx, &mut menu, Key::Enter);
    assert_eq!(
        drawn.closed.map(|closed| closed.chose),
        Some(Some(Verb::AddToQueue))
    );
}

#[test]
fn a_repeated_enter_chooses_nothing_and_leaks_nowhere() {
    let ctx = context();
    // The user opens the menu with Enter and holds it: egui marks every
    // press with no release between it and the one before as a repeat.
    let mut menu = opened(Device::Keyboard);
    warm(&ctx, &mut menu);
    let _ = menu.close(Reason::Escape);

    let mut menu = opened(Device::Keyboard);
    let opening = frame(
        &ctx,
        &mut menu,
        true,
        vec![key_event(Key::Enter, Modifiers::NONE)],
    );
    assert_eq!(opening.closed, None, "the opening Enter fired a row");

    let (drawn, leaked) = draw(&ctx, &mut menu, true, vec![repeat(Key::Enter)], |ui, _| {
        ui.input(|input| input.key_pressed(Key::Enter))
    });
    assert_eq!(drawn.closed, None, "a held Enter fired a row");
    assert!(menu.is_open(), "a held Enter closed the menu");
    assert!(!leaked, "the repeat reached the `⋯` or the drawer");
}

#[test]
fn escape_closes_and_the_drawer_never_sees_it() {
    let ctx = context();
    let mut menu = opened(Device::Keyboard);
    warm(&ctx, &mut menu);

    let mut escaped = None;
    let (drawn, focused) = draw(
        &ctx,
        &mut menu,
        true,
        vec![key_event(Key::Escape, Modifiers::NONE)],
        |ui, _| {
            escaped = Some(ui.input(|input| input.key_pressed(Key::Escape)));
            ui.memory(|memory| memory.has_focus(opener()))
        },
    );
    assert_eq!(
        drawn.closed,
        Some(Closed {
            opener: Some(opener()),
            chose: None,
        })
    );
    assert_eq!(escaped, Some(false), "the Escape reached the drawer");
    assert!(focused, "Escape did not return the keyboard");
}

#[test]
fn tab_closes_and_focus_returns_to_the_opener() {
    let ctx = context();
    let mut menu = opened(Device::Keyboard);
    warm(&ctx, &mut menu);
    // One more frame, so the focus lock filter is in place.
    frame(&ctx, &mut menu, true, Vec::new());

    let (drawn, focused) = draw(
        &ctx,
        &mut menu,
        true,
        vec![key_event(Key::Tab, Modifiers::NONE)],
        |ui, _| ui.memory(|memory| memory.has_focus(opener())),
    );
    assert_eq!(
        drawn.closed,
        Some(Closed {
            opener: Some(opener()),
            chose: None,
        })
    );
    assert!(focused, "Tab did not return the keyboard");
}
