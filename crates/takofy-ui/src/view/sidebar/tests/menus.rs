use super::super::*;
use super::test_support::*;

use egui::Event;

use crate::menu::{Anchor, Device, LibraryItem, Listing};
use crate::message::Update;
use crate::view::context_menu;
use crate::view::test_support::click_events;

/// One press of the context-menu key.
fn menu_key() -> Event {
    Event::Key {
        key: egui::Key::Menu,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

/// Right-clicks `at` in the column form, one pass per event.
fn right_click(
    ctx: &egui::Context,
    at: Pos2,
    library: &mut Library,
    art: &mut Art,
) -> Option<Command> {
    let mut command = None;
    for event in crate::view::test_support::secondary_click_events(at) {
        let (_, next) = run_column(ctx, vec![event], library, art);
        if next.is_some() {
            command = next;
        }
    }
    command
}

#[test]
fn a_right_click_on_each_kind_posts_its_library_request() {
    let cases = [
        (SidebarKind::Playlists, "A", "spotify:playlist:A", "A"),
        (SidebarKind::Podcasts, "A show", "spotify:show:s", "A show"),
        (
            SidebarKind::Artists,
            "An artist",
            "spotify:artist:a",
            "An artist",
        ),
        (
            SidebarKind::Albums,
            "An album",
            "spotify:album:r",
            "An album",
        ),
    ];
    for (kind, label, uri, name) in cases {
        let ctx = context();
        let mut library = four_kinds();
        assert!(library.choose_kind(kind));
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let at = name_line(&output, label);

        let command = right_click(&ctx, at, &mut library, &mut art);

        let request =
            context_menu::take_request(&ctx).unwrap_or_else(|| panic!("{kind:?} posted no menu"));
        assert_eq!(
            request.listing,
            Listing::Library(LibraryItem {
                kind,
                uri: uri.to_owned(),
                name: name.to_owned(),
            })
        );
        let expected = match kind {
            SidebarKind::Playlists => {
                vec!["Rename", "Copy link", "Start radio", "Remove from library"]
            }
            SidebarKind::Albums | SidebarKind::Artists => {
                vec!["Copy link", "Start radio", "Remove from library"]
            }
            SidebarKind::Podcasts => vec!["Copy link", "Remove from library"],
        };
        assert_eq!(
            request
                .items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            expected,
            "{kind:?}"
        );
        assert!(
            matches!(request.door.anchor, Anchor::Pointer(_)),
            "{kind:?}"
        );
        assert_eq!(request.door.device, Device::Pointer, "{kind:?}");
        assert!(request.targets.is_empty(), "{kind:?}");
        assert!(request.rows.is_empty(), "{kind:?}");
        assert_eq!(request.page, None, "{kind:?}");

        assert_eq!(command, None, "{kind:?} opened its page");
        assert!(carry::carried(&ctx).is_none(), "{kind:?} lifted its row");
    }
}

#[test]
fn a_row_field_centres_its_text_in_the_box() {
    let ctx = context();
    let mut library = movable_playlists(&["A"], 7, true, None);
    let mut art = Art::default();
    let (plain, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    library
        .renaming
        .open("spotify:playlist:A", "A", crate::naming::Surface::Row);
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    let (pos, size) = painted_text(&output, "A");
    let middle = pos.y + size.y / 2.0;
    let row_middle = row_top(&plain, "A", "Playlist") + ROW_H / 2.0;
    assert!(
        (middle - row_middle).abs() < 1.0,
        "the field's text sits at {middle}, not the row's middle {row_middle}"
    );
}

#[test]
fn a_rename_field_is_as_tall_as_both_of_the_row_lines() {
    let field = super::super::rename_field(10.0, 4.0, 120.0, 17.0, 13.0);
    assert_eq!(field.min, Pos2::new(10.0, 4.0));
    assert_eq!(field.width(), 120.0);
    assert_eq!(
        field.height(),
        30.0,
        "a field on the name line alone clips its own text"
    );
    assert!(
        field.height() > 17.0,
        "the field has no room past the name line"
    );
}

#[test]
fn a_row_the_user_cannot_write_offers_no_rename() {
    for access in [
        Some(takofy_model::PlaylistAccess {
            revision: takofy_model::Revision::from_wire(&[7]).expect("a revision"),
            editable: false,
        }),
        None,
    ] {
        let ctx = context();
        let mut library = movable_playlists(&["A"], 7, true, None);
        if let Load::Ready(playlists) = &mut library.playlists {
            playlists[0].access = access;
        }
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let at = name_line(&output, "A");
        let _ = right_click(&ctx, at, &mut library, &mut art);

        let request = context_menu::take_request(&ctx).expect("the row posted a menu");
        assert!(
            request
                .items
                .iter()
                .all(|item| item.verb != crate::menu::Verb::Rename),
            "a row the listing does not call editable offered Rename"
        );
    }

    let ctx = context();
    let mut library = movable_playlists(&["A"], 7, true, None);
    if let Load::Ready(playlists) = &mut library.playlists {
        playlists[0].access = None;
    }
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    let at = name_line(&output, "A");
    let _ = right_click(&ctx, at, &mut library, &mut art);
    let request = context_menu::take_request(&ctx).expect("the row posted a menu");
    assert!(
        request
            .items
            .iter()
            .all(|item| item.verb != crate::menu::Verb::Rename),
        "a row with no access yet offered Rename"
    );

    if let Load::Ready(playlists) = &mut library.playlists {
        playlists[0].access = Some(takofy_model::PlaylistAccess {
            revision: takofy_model::Revision::from_wire(&[7]).expect("a revision"),
            editable: true,
        });
    }
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    let at = name_line(&output, "A");
    let _ = right_click(&ctx, at, &mut library, &mut art);
    let request = context_menu::take_request(&ctx).expect("the row posted a menu");
    assert!(
        request
            .items
            .iter()
            .any(|item| item.verb == crate::menu::Verb::Rename),
        "the arrived access still offered no Rename"
    );
}

#[test]
fn the_two_rows_of_one_list_are_two_openers() {
    let ctx = context();
    let mut library = movable_playlists(&["A", "B"], 7, true, None);
    let mut art = Art::default();
    let mut openers = Vec::new();
    for label in ["A", "B"] {
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let at = name_line(&output, label);
        let _ = right_click(&ctx, at, &mut library, &mut art);
        let request = context_menu::take_request(&ctx).expect("a menu");
        openers.push(request.door.opener);
    }

    assert_ne!(openers[0], openers[1], "both rows opened the same widget");
}

#[test]
fn the_context_menu_key_opens_on_a_focused_row_only() {
    let ctx = context();
    let mut library = movable_playlists(&["A", "B"], 7, true, None);
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    let at = name_line(&output, "A");

    let _ = run_column(&ctx, vec![menu_key()], &mut library, &mut art);
    assert!(
        context_menu::take_request(&ctx).is_none(),
        "a key with no focused row opened a menu"
    );

    for event in click_events(at) {
        let _ = run_column(&ctx, vec![event], &mut library, &mut art);
    }
    let _ = run_column(&ctx, vec![menu_key()], &mut library, &mut art);

    let request = context_menu::take_request(&ctx).expect("the focused row opened its menu");
    assert!(matches!(request.door.anchor, Anchor::Button(_)));
    assert_eq!(request.door.device, Device::Keyboard);
    assert_eq!(
        request.listing,
        Listing::Library(LibraryItem {
            kind: SidebarKind::Playlists,
            uri: "spotify:playlist:A".to_owned(),
            name: "A".to_owned(),
        })
    );
}

#[test]
fn a_carry_or_the_drawer_form_posts_no_menu() {
    // A block is held.
    let ctx = context();
    let mut library = movable_playlists(&["A"], 7, true, None);
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    let at = name_line(&output, "A");
    egui::DragAndDrop::set_payload(&ctx, ());
    let _ = right_click(&ctx, at, &mut library, &mut art);
    assert!(
        context_menu::take_request(&ctx).is_none(),
        "a held block opened a menu"
    );

    // The drawer's form opens no menu, so it posts nothing.
    let ctx = context();
    let mut library = movable_playlists(&["A"], 7, true, None);
    let (output, _) = run_with(&ctx, Vec::new(), &mut library, &mut art, false);
    let at = name_line(&output, "A");
    for event in crate::view::test_support::secondary_click_events(at) {
        let _ = run_with(&ctx, vec![event], &mut library, &mut art, false);
    }
    assert!(context_menu::take_request(&ctx).is_none());
}

#[test]
fn a_playlist_row_that_cannot_be_unlinked_offers_only_its_link() {
    let ctx = context();
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist(
        "spotify:playlist:A",
        "A",
    )]));
    assert!(!library.rootlist_removable());
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    let at = name_line(&output, "A");

    let _ = right_click(&ctx, at, &mut library, &mut art);

    let request = context_menu::take_request(&ctx).expect("a menu");
    assert_eq!(
        request
            .items
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>(),
        ["Copy link", "Start radio"],
        "a row the library cannot write offered a removal"
    );
}

#[test]
fn an_unowned_playlist_still_offers_its_removal() {
    let ctx = context();
    // The rootlist is the user's own, but this playlist's tracks are not
    // theirs to edit; the unlink names the rootlist row and not the tracks.
    let mut library = movable_playlists(&["A"], 7, false, None);
    library.apply(&Update::RootlistAccess(takofy_model::PlaylistAccess {
        revision: takofy_model::Revision::from_wire(&[7]).expect("a revision"),
        editable: true,
    }));
    assert!(library.rootlist_removable());
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    let at = name_line(&output, "A");

    let _ = right_click(&ctx, at, &mut library, &mut art);

    let request = context_menu::take_request(&ctx).expect("a menu");
    assert_eq!(
        request
            .items
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>(),
        ["Copy link", "Start radio", "Remove from library"],
        "an unowned playlist was not offered its removal"
    );
}
