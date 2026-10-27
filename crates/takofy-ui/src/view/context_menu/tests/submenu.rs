use super::super::*;
use super::test_support::*;
use crate::layout::search_menu::row_rect;
use crate::menu::{Device, Gate, Item, Kind, Menu, Verb};
use crate::view::test_support::click_events;
use egui::{Event, Key, Modifiers, Rect};

/// The raised cards one frame painted, in paint order: the menu first, the
/// submenu beside it.
fn cards(output: &egui::FullOutput) -> Vec<Rect> {
    fills_of(output, Tokens::DARK.palette.surface_raised)
}

/// A menu of a submenu row, two verbs and two lists to offer.
fn with_lists(device: Device) -> Menu {
    let mut request = request(device, items());
    request.playlists = vec![
        crate::menu::AddTarget {
            uri: "spotify:playlist:a".to_owned(),
            name: "Night Drive".to_owned(),
            revision: takofy_model::Revision::from_wire(&[0x07]).expect("a revision"),
            length: 3,
        },
        crate::menu::AddTarget {
            uri: "spotify:playlist:b".to_owned(),
            name: "Piano".to_owned(),
            revision: takofy_model::Revision::from_wire(&[0x08]).expect("a revision"),
            length: 5,
        },
    ];
    request.items.insert(
        0,
        Item {
            label: "Add to playlist".to_owned(),
            kind: Kind::Plain,
            key: None,
            submenu: true,
            verb: Verb::AddToPlaylist,
        },
    );
    let mut menu = Menu::default();
    assert!(menu.open(request, Gate::default()));
    menu
}

#[test]
fn a_pointer_on_the_submenu_row_opens_the_list_and_the_frame_draws_its_card() {
    let ctx = context();
    let mut menu = with_lists(Device::Keyboard);
    warm(&ctx, &mut menu);
    // The menu's card first, so the row's own rect is known.
    let (first, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
    let card = card(&first.output);
    let row = row_rect(card, 0);
    let at = row.center();

    let (second, _) = draw(
        &ctx,
        &mut menu,
        true,
        vec![Event::PointerMoved(at)],
        |_, _| (),
    );
    assert_eq!(menu.submenu(), Some(0), "the pointer did not open the list");
    let cards = cards(&second.output);
    assert_eq!(cards.len(), 2, "the submenu's own card was not drawn");
    // This menu is right-aligned near the window's edge, so there is no room
    // beside it and the card takes the sheet's other side.
    assert!(
        cards[1].right() <= cards[0].left(),
        "the submenu at {:?} is neither beside nor left of {:?}",
        cards[1],
        cards[0]
    );
    assert_eq!(
        cards[1].top(),
        row.top(),
        "the card is not at the row's top"
    );
}

#[test]
fn enter_on_the_submenu_row_opens_its_list_without_closing_the_menu() {
    let ctx = context();
    let mut menu = with_lists(Device::Keyboard);
    warm(&ctx, &mut menu);
    // The keyboard lit the first row: the one that opens the list.
    assert_eq!(menu.lit(), Some(0));

    let (drawn, _) = draw(
        &ctx,
        &mut menu,
        true,
        vec![key_event(Key::Enter, Modifiers::NONE)],
        |_, _| (),
    );
    assert!(
        drawn.closed.is_none(),
        "Enter chose a row that opens a list"
    );
    assert!(menu.is_open());
    assert_eq!(menu.submenu(), Some(0));
}

#[test]
fn a_click_on_a_list_closes_the_menu_on_that_add() {
    let ctx = context();
    let mut menu = with_lists(Device::Pointer);
    warm(&ctx, &mut menu);
    assert!(menu.open_submenu(0));
    let (first, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
    let submenu = *cards(&first.output).last().expect("two cards");
    let row = geometry::submenu_row(submenu, 1);
    let at = row.center();

    let (closed, _) = draw(&ctx, &mut menu, true, click_events(at).to_vec(), |_, _| ());
    let closed = closed.closed.expect("the click closed the menu");
    assert_eq!(closed.chose, Some(Verb::AddToPlaylist));
    assert_eq!(
        menu.chosen_target().map(|target| target.uri.as_str()),
        Some("spotify:playlist:b"),
        "the second list is the one under the pointer"
    );
}

#[test]
fn escape_steps_back_out_of_the_submenu_before_it_closes_the_menu() {
    let ctx = context();
    let mut menu = with_lists(Device::Keyboard);
    warm(&ctx, &mut menu);
    assert!(menu.open_submenu(0));

    let escape = Event::Key {
        key: Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    let (first, _) = draw(&ctx, &mut menu, true, vec![escape.clone()], |_, _| ());
    assert!(first.closed.is_none(), "Escape closed the whole menu");
    assert!(menu.is_open());
    assert_eq!(menu.submenu(), None, "Escape left the submenu up");

    let (second, _) = draw(&ctx, &mut menu, true, vec![escape], |_, _| ());
    assert!(
        second
            .closed
            .expect("the second Escape closes")
            .chose
            .is_none()
    );
    assert!(!menu.is_open());
}

#[test]
fn the_submenu_hides_its_field_behind_a_hairline_and_paints_its_lists() {
    let ctx = context();
    let mut menu = with_lists(Device::Pointer);
    warm(&ctx, &mut menu);
    assert!(menu.open_submenu(0));
    let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
    let submenu = *cards(&drawn.output).last().expect("two cards");

    let labels = texts(&drawn.output);
    assert!(
        labels.iter().any(|(text, _)| text == "Night Drive"),
        "the first list was not drawn: {labels:?}"
    );
    assert!(labels.iter().any(|(text, _)| text == "Piano"));
    assert!(
        labels.iter().any(|(text, _)| text == "Find a playlist"),
        "the field's hint is missing: {labels:?}"
    );

    // The rule between the field and the lists.
    let rule = geometry::submenu_rule(submenu);
    assert!(
        fills_of(&drawn.output, Tokens::DARK.palette.border)
            .iter()
            .any(|rect| rect.contains_rect(rule)),
        "no hairline separates the field from the lists"
    );
}

#[test]
fn a_field_that_matches_nothing_says_so_in_one_muted_line() {
    let ctx = context();
    let mut menu = with_lists(Device::Pointer);
    warm(&ctx, &mut menu);
    assert!(menu.open_submenu(0));
    menu.filter_edit().push_str("zzz");
    let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
    let labels = texts(&drawn.output);
    assert!(
        labels.iter().any(|(text, _)| text == "No playlists match."),
        "{labels:?}"
    );
    assert!(!labels.iter().any(|(text, _)| text == "Night Drive"));
}
