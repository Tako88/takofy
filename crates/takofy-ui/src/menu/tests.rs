use egui::{Id, Pos2, Rect, vec2};
use takofy_model::TrackEntry;

use super::*;
use crate::route::Page;

fn opener() -> Id {
    Id::unique("row-more")
}

fn anchor() -> Anchor {
    Anchor::Button(Rect::from_min_size(Pos2::ZERO, vec2(28.0, 28.0)))
}

fn door(device: Device) -> Door {
    Door {
        anchor: anchor(),
        opener: opener(),
        device,
    }
}

fn item(index: usize) -> Item {
    let verbs = [
        Verb::AddToQueue,
        Verb::GoToAlbum,
        Verb::GoToArtist,
        Verb::CopyLink,
    ];
    Item {
        label: format!("Verb {index}"),
        kind: Kind::Plain,
        key: None,
        submenu: false,
        verb: verbs[index % verbs.len()],
    }
}

fn request(device: Device, rows: usize) -> Request {
    Request {
        door: door(device),
        items: (0..rows).map(item).collect(),
        playlists: Vec::new(),
        targets: Vec::new(),
        rows: Vec::new(),
        listing: Listing::default(),
        page: None,
    }
}

fn target(uri: &str, name: &str, byte: u8, length: usize) -> AddTarget {
    AddTarget {
        uri: uri.to_owned(),
        name: name.to_owned(),
        revision: Revision::from_wire(&[byte]).expect("a revision"),
        length,
    }
}

fn add_item() -> Item {
    Item {
        label: "Add to playlist".to_owned(),
        kind: Kind::Plain,
        key: None,
        submenu: true,
        verb: Verb::AddToPlaylist,
    }
}

fn playlist_listing() -> Listing {
    Listing::Playlist {
        uri: "spotify:playlist:a".to_owned(),
        list: Id::unique("playlist-tracks"),
    }
}

fn opened_on(request: Request) -> Menu {
    let mut menu = Menu::default();
    assert!(menu.open(request, Gate::default()), "the menu opened");
    menu
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

fn opened(device: Device, rows: usize) -> Menu {
    let mut menu = Menu::default();
    assert!(
        menu.open(request(device, rows), Gate::default()),
        "the menu opened"
    );
    menu
}

fn entry(uri: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: "A Title".to_owned(),
        artists: "A Band".to_owned(),
        album: Some("A Record".to_owned()),
        album_uri: Some("spotify:album:a".to_owned()),
        artist_uris: vec!["spotify:artist:b".to_owned()],
        artist_names: vec!["A Band".to_owned()],
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

#[test]
fn a_pointer_open_lights_nothing() {
    let menu = opened(Device::Pointer, 3);
    assert_eq!(menu.lit(), None, "the pointer lit a row it never reached");
    assert_eq!(menu.device(), Device::Pointer);
    for row in 0..3 {
        assert_eq!(menu.tier(row, None), Tier::None, "row {row}");
    }
}

#[test]
fn a_keyboard_open_lights_the_first_row() {
    let menu = opened(Device::Keyboard, 3);
    assert_eq!(menu.lit(), Some(0));
    assert_eq!(menu.tier(0, None), Tier::Keyboard);
    assert_eq!(menu.tier(1, None), Tier::None);
}

#[test]
fn opening_is_refused_while_carrying_or_writing() {
    for gate in [
        Gate {
            carrying: true,
            writing: false,
        },
        Gate {
            carrying: false,
            writing: true,
        },
    ] {
        let mut menu = Menu::default();
        assert!(!menu.open(request(Device::Pointer, 2), gate), "{gate:?}");
        assert!(!menu.is_open(), "{gate:?} opened the menu");
    }
    let mut menu = Menu::default();
    assert!(menu.open(request(Device::Pointer, 2), Gate::default()));
    assert!(menu.is_open());
}

#[test]
fn an_empty_menu_does_not_open() {
    let mut menu = Menu::default();
    assert!(!menu.open(request(Device::Pointer, 0), Gate::default()));
    assert!(!menu.is_open());
}

#[test]
fn down_from_nothing_lights_the_first_row_and_up_the_last() {
    let mut down = opened(Device::Pointer, 4);
    down.step(Step::Down);
    assert_eq!(down.lit(), Some(0));
    assert_eq!(down.device(), Device::Keyboard, "a step is the keyboard's");

    let mut up = opened(Device::Pointer, 4);
    up.step(Step::Up);
    assert_eq!(up.lit(), Some(3));
}

#[test]
fn the_arrows_clamp_at_the_ends() {
    let mut menu = opened(Device::Keyboard, 3);
    for _ in 0..8 {
        menu.step(Step::Down);
    }
    assert_eq!(menu.lit(), Some(2), "down wrapped past the last row");
    for _ in 0..8 {
        menu.step(Step::Up);
    }
    assert_eq!(menu.lit(), Some(0), "up wrapped past the first row");
}

#[test]
fn home_and_end_jump() {
    let mut menu = opened(Device::Pointer, 5);
    menu.step(Step::Last);
    assert_eq!(menu.lit(), Some(4));
    menu.step(Step::First);
    assert_eq!(menu.lit(), Some(0));
}

#[test]
fn a_pointer_over_a_row_swaps_the_tier() {
    let mut menu = opened(Device::Keyboard, 4);
    assert_eq!(menu.lit(), Some(0));

    menu.pointer_over(Some(2));
    assert_eq!(
        menu.lit(),
        None,
        "the keyboard kept its row under the pointer"
    );
    assert_eq!(menu.device(), Device::Pointer);
    assert_eq!(menu.tier(2, Some(2)), Tier::Pointer);
    assert_eq!(menu.tier(0, Some(2)), Tier::None);
    assert_eq!(menu.tier(2, None), Tier::None);
}

#[test]
fn only_one_tier_is_ever_lit() {
    for device in [Device::Keyboard, Device::Pointer] {
        for rows in 1..=4 {
            for pointer in [None, Some(0), Some(2)] {
                let mut menu = opened(device, rows);
                if pointer.is_some() {
                    menu.pointer_over(pointer);
                }
                for step in [None, Some(Step::Down), Some(Step::Up), Some(Step::Last)] {
                    let mut menu = menu.clone();
                    if let Some(step) = step {
                        menu.step(step);
                    }
                    let lit = (0..rows)
                        .filter(|row| menu.tier(*row, pointer) != Tier::None)
                        .count();
                    assert!(lit <= 1, "{lit} rows lit for {device:?} at {pointer:?}");
                }
            }
        }
    }
}

#[test]
fn enter_chooses_only_a_row_the_keyboard_lit() {
    let mut pointer = opened(Device::Pointer, 3);
    assert_eq!(pointer.choose_lit(), None, "Enter fired a row nobody lit");
    assert!(
        pointer.is_open(),
        "the menu closed on a choice it never made"
    );

    let mut keyboard = opened(Device::Keyboard, 3);
    keyboard.step(Step::Down);
    assert_eq!(
        keyboard.choose_lit(),
        Some(Closed {
            opener: Some(opener()),
            chose: Some(Verb::GoToAlbum),
        })
    );
    assert!(!keyboard.is_open());
}

#[test]
fn a_click_chooses_the_row_and_closes() {
    let mut menu = opened(Device::Pointer, 3);
    assert_eq!(
        menu.choose(1),
        Some(Closed {
            opener: Some(opener()),
            chose: Some(Verb::GoToAlbum),
        })
    );
    assert!(!menu.is_open());
}

#[test]
fn choosing_a_row_that_is_not_there_does_nothing() {
    let mut menu = opened(Device::Pointer, 2);
    assert_eq!(menu.choose(2), None);
    assert!(menu.is_open(), "a miss closed the menu");
    assert_eq!(menu.choose(99), None);
    assert!(menu.is_open());
}

#[test]
fn the_first_drawn_frame_is_the_opening_and_the_pass_after_it_is_not() {
    let mut menu = opened(Device::Pointer, 3);
    assert!(menu.just_opened(1.0), "nothing has drawn it yet");
    menu.mark_drawn(1.0);
    assert!(menu.just_opened(1.0), "a second pass of the same frame");
    assert!(
        !menu.just_opened(1.016),
        "a later frame is not the opening, or the door's click would dismiss it"
    );
}

#[test]
fn escape_tab_outside_and_focus_all_return_the_opener() {
    for reason in [Reason::Escape, Reason::Tab, Reason::Outside] {
        let mut menu = opened(Device::Keyboard, 3);
        assert_eq!(
            menu.close(reason),
            Some(Closed {
                opener: Some(opener()),
                chose: None,
            }),
            "{reason:?}"
        );
        assert!(!menu.is_open(), "{reason:?}");
    }
}

#[test]
fn a_page_change_closes_without_an_opener() {
    let mut menu = Menu::default();
    let mut request = request(Device::Pointer, 2);
    request.page = Some(Page::Playlist("a".to_owned()));
    assert!(menu.open(request, Gate::default()));

    assert_eq!(
        menu.follow_page(Some(&Page::Playlist("a".to_owned()))),
        None
    );
    assert!(menu.is_open(), "the page it opened on closed it");

    assert_eq!(
        menu.follow_page(Some(&Page::Playlist("b".to_owned()))),
        Some(Closed {
            opener: None,
            chose: None,
        }),
        "the row the menu was about is gone, so there is nothing to focus"
    );
    assert!(!menu.is_open());
}

#[test]
fn reopening_replaces_the_menu() {
    let mut menu = opened(Device::Pointer, 2);
    let mut request = request(Device::Keyboard, 3);
    request.door.opener = Id::unique("another-more");
    assert!(menu.open(request, Gate::default()));

    assert_eq!(menu.items().len(), 3);
    assert_eq!(menu.opener(), Some(Id::unique("another-more")));
    assert_eq!(menu.lit(), Some(0));
}

#[test]
fn the_destructive_row_is_always_last() {
    let mut menu = Menu::default();
    let mut request = request(Device::Pointer, 3);
    request.items[0].kind = Kind::Destructive;
    request.items[0].label = "Remove".to_owned();
    assert!(menu.open(request, Gate::default()));

    let kinds: Vec<Kind> = menu.items().iter().map(|item| item.kind).collect();
    assert_eq!(kinds, vec![Kind::Plain, Kind::Plain, Kind::Destructive]);
    assert_eq!(
        menu.items().last().map(|item| item.label.as_str()),
        Some("Remove")
    );
}

#[test]
fn the_remove_row_is_drawn_last_and_lit_first_by_the_keyboard() {
    let mut request = request(Device::Keyboard, 2);
    request.items = vec![item(0), destructive_item("Remove from this playlist")];
    let menu = opened_on(request);

    assert_eq!(
        menu.items().last().map(|item| item.verb),
        Some(Verb::Remove),
        "the destructive row is not last after the sort"
    );
    assert_eq!(menu.lit(), Some(0), "the keyboard opens on the first row");
}

#[test]
fn cut_is_built_from_the_snapshot_and_only_for_a_removing_listing() {
    let entries = vec![entry("spotify:track:a"), entry("spotify:track:b")];
    for listing in [
        Listing::Page,
        Listing::Current,
        Listing::Queue,
        playlist_listing(),
    ] {
        let mut request = request(Device::Pointer, 2);
        request.listing = listing.clone();
        request.rows = vec![1, 3];
        request.targets = entries.clone();
        let menu = opened_on(request);

        let removing = matches!(listing, Listing::Queue | Listing::Playlist { .. });
        match menu.cut() {
            Some(cut) => {
                assert!(removing, "{listing:?} offered a removal");
                assert_eq!(cut.listing, listing, "{listing:?}");
                assert_eq!(cut.rows, vec![1, 3], "{listing:?}");
                assert_eq!(cut.entries, entries, "{listing:?}");
            }
            None => assert!(!removing, "{listing:?} has no cut"),
        }
    }
}

#[test]
fn cut_survives_the_close() {
    let mut request = request(Device::Keyboard, 2);
    request.listing = Listing::Queue;
    request.rows = vec![2];
    request.targets = vec![entry("spotify:track:a")];
    let mut menu = opened_on(request);

    let cut = menu.cut().expect("a removal");
    assert!(menu.close(Reason::Escape).is_some());
    assert_eq!(
        menu.cut(),
        Some(cut),
        "the shell reads the cut after the menu closed on it"
    );
}

#[test]
fn a_submenu_row_is_not_a_command() {
    let mut menu = Menu::default();
    let mut request = request(Device::Pointer, 2);
    request.items = vec![add_item(), item(1)];
    request.playlists = vec![target("spotify:playlist:a", "A", 0x07, 3)];
    assert!(menu.open(request, Gate::default()));

    assert_eq!(menu.choose(0), None, "a submenu row fired a command");
    assert!(menu.is_open(), "the menu closed on a row that opens a list");
}

#[test]
fn a_submenu_opens_only_on_a_submenu_row_with_candidates() {
    let mut menu = Menu::default();
    let mut offered = request(Device::Pointer, 2);
    offered.items = vec![add_item(), item(1)];
    offered.playlists = vec![target("spotify:playlist:a", "A", 0x07, 3)];
    assert!(menu.open(offered, Gate::default()));

    assert!(menu.open_submenu(0));
    assert_eq!(menu.submenu(), Some(0));
    menu.close_submenu();
    assert_eq!(menu.submenu(), None);
    assert!(!menu.open_submenu(1), "a plain row opens no list");

    let mut empty = Menu::default();
    let mut bare = request(Device::Pointer, 1);
    bare.items = vec![add_item()];
    assert!(empty.open(bare, Gate::default()));
    assert!(!empty.open_submenu(0), "a list with nothing in it opened");
    assert_eq!(empty.submenu(), None);
}

#[test]
fn choose_target_reports_the_add_and_refuses_without_an_open_submenu() {
    let playlists = vec![
        target("spotify:playlist:a", "A", 0x07, 3),
        target("spotify:playlist:b", "B", 0x08, 5),
    ];
    let mut menu = Menu::default();
    let mut request = request(Device::Pointer, 2);
    request.items = vec![add_item(), item(1)];
    request.playlists = playlists.clone();
    assert!(menu.open(request, Gate::default()));

    assert_eq!(menu.choose_target(0), None, "no submenu was open");
    assert!(menu.is_open());

    assert!(menu.open_submenu(0));
    assert_eq!(menu.choose_target(2), None, "a list nobody offered");
    assert!(menu.is_open(), "a miss closed the menu");

    assert_eq!(
        menu.choose_target(1),
        Some(Closed {
            opener: Some(opener()),
            chose: Some(Verb::AddToPlaylist),
        })
    );
    assert!(!menu.is_open());
    assert_eq!(menu.submenu(), None);
    assert_eq!(
        menu.chosen_target(),
        Some(&playlists[1]),
        "the choice survives the close"
    );
}

#[test]
fn opening_a_menu_forgets_the_last_choice() {
    let playlists = vec![target("spotify:playlist:a", "A", 0x07, 3)];
    let mut menu = Menu::default();
    let mut first = request(Device::Pointer, 1);
    first.items = vec![add_item()];
    first.playlists = playlists.clone();
    assert!(menu.open(first, Gate::default()));
    assert!(menu.open_submenu(0));
    assert!(menu.choose_target(0).is_some());
    assert!(menu.chosen_target().is_some());

    let mut again = request(Device::Pointer, 1);
    again.items = vec![add_item()];
    again.playlists = playlists;
    assert!(menu.open(again, Gate::default()));

    assert_eq!(menu.chosen_target(), None, "the last menu's choice leaked");
    assert_eq!(menu.submenu(), None);
}

#[test]
fn the_field_finds_a_list_by_any_part_of_its_name() {
    assert!(matches("Night Drive", ""));
    assert!(matches("Night Drive", "   "));
    assert!(matches("Night Drive", "night"));
    assert!(matches("Night Drive", "DRIVE"));
    assert!(matches("Night Drive", "ht dr"));
    assert!(!matches("Night Drive", "mix"));
    assert!(!matches("Night Drive", "night drive 2"));
    // Trimmed at both ends, so a stray space does not empty the list.
    assert!(matches("Night Drive", " drive "));
}

#[test]
fn the_submenu_shows_what_the_field_matches_in_the_sidebar_order() {
    let mut menu = Menu::default();
    let mut request = request(Device::Pointer, 1);
    request.items = vec![add_item()];
    request.playlists = vec![
        target("spotify:playlist:a", "Night Drive", 0x07, 3),
        target("spotify:playlist:b", "Piano", 0x08, 5),
        target("spotify:playlist:c", "Night Shift", 0x09, 1),
    ];
    assert!(menu.open(request, Gate::default()));
    assert!(menu.open_submenu(0));
    assert_eq!(menu.shown(), vec![0, 1, 2], "an empty field hid a list");

    menu.filter_edit().push_str("night");
    assert_eq!(menu.shown(), vec![0, 2]);

    menu.filter_edit().clear();
    menu.filter_edit().push_str("zzz");
    assert_eq!(menu.shown(), Vec::<usize>::new());
}

#[test]
fn the_submenu_stops_at_the_sheets_eight_rows() {
    let mut menu = Menu::default();
    let mut request = request(Device::Pointer, 1);
    request.items = vec![add_item()];
    request.playlists = (0..12)
        .map(|index| {
            target(
                &format!("spotify:playlist:{index}"),
                &format!("List {index}"),
                0x07,
                index,
            )
        })
        .collect();
    assert!(menu.open(request, Gate::default()));
    assert!(menu.open_submenu(0));

    let shown = menu.shown();
    assert_eq!(shown.len(), extent::SUBMENU_ROWS);
    assert_eq!(shown.first(), Some(&0));
    assert_eq!(shown.last(), Some(&(extent::SUBMENU_ROWS - 1)));

    // A filter that names the twelfth list finds it, even though the empty
    // field's eight rows never reached it.
    menu.filter_edit().push_str("List 11");
    assert_eq!(menu.shown(), vec![11]);
}

#[test]
fn the_submenu_cursor_walks_the_shown_rows_and_chooses_the_list_it_names() {
    let playlists = vec![
        target("spotify:playlist:a", "A", 0x07, 3),
        target("spotify:playlist:b", "B", 0x08, 5),
    ];
    let mut menu = Menu::default();
    let mut request = request(Device::Pointer, 1);
    request.items = vec![add_item()];
    request.playlists = playlists.clone();
    assert!(menu.open(request, Gate::default()));
    assert!(menu.open_submenu(0));

    assert_eq!(menu.submenu_lit(), None, "a pointer opened with a lit row");
    menu.step_submenu(Step::Down);
    assert_eq!(menu.submenu_lit(), Some(0));
    menu.step_submenu(Step::Last);
    assert_eq!(menu.submenu_lit(), Some(1));
    menu.step_submenu(Step::Down);
    assert_eq!(
        menu.submenu_lit(),
        Some(1),
        "the cursor wrapped past the end"
    );
    assert_eq!(menu.submenu_tier(1, None), Tier::Keyboard);

    let closed = menu.choose_submenu_lit().expect("a choice");
    assert_eq!(closed.chose, Some(Verb::AddToPlaylist));
    assert_eq!(menu.chosen_target(), Some(&playlists[1]));
}

#[test]
fn a_filter_that_shortens_the_list_unlights_a_row_that_moved() {
    let playlists = vec![
        target("spotify:playlist:a", "A", 0x07, 3),
        target("spotify:playlist:b", "B", 0x08, 5),
    ];
    let mut menu = Menu::default();
    let mut request = request(Device::Keyboard, 1);
    request.items = vec![add_item()];
    request.playlists = playlists;
    assert!(menu.open(request, Gate::default()));
    assert!(menu.open_submenu(0));
    menu.step_submenu(Step::Last);
    assert_eq!(menu.submenu_lit(), Some(1));

    menu.filter_edit().push('A');
    assert_eq!(menu.shown(), vec![0]);
    assert_eq!(
        menu.submenu_lit(),
        None,
        "Enter would have fired a row the keyboard never lit"
    );
    assert_eq!(menu.choose_submenu_lit(), None);
}

#[test]
fn another_rows_submenu_starts_with_an_empty_field() {
    let mut menu = Menu::default();
    let mut request = request(Device::Pointer, 3);
    request.items = vec![add_item(), item(1), add_item()];
    request.playlists = vec![target("spotify:playlist:a", "Night Drive", 0x07, 3)];
    assert!(menu.open(request, Gate::default()));

    assert!(menu.open_submenu(0));
    menu.filter_edit().push_str("night");
    menu.step_submenu(Step::Down);

    assert!(menu.open_submenu(2));
    assert_eq!(menu.filter(), "", "the last submenu's field leaked");
    assert_eq!(menu.submenu_lit(), None);
    assert_eq!(menu.shown(), vec![0]);

    menu.filter_edit().push_str("night");
    menu.close_submenu();
    assert_eq!(menu.filter(), "");
}

#[test]
fn allow_add_strips_the_row_when_nothing_is_offered() {
    let mut request = request(Device::Pointer, 2);
    request.items = vec![add_item(), item(1)];
    let offered = request.items.len();

    request.allow_add(Vec::new());
    assert_eq!(request.items.len(), offered - 1);
    assert!(
        request
            .items
            .iter()
            .all(|item| item.verb != Verb::AddToPlaylist)
    );

    request.allow_add(vec![target("spotify:playlist:a", "A", 0x07, 3)]);
    assert_eq!(
        request.items.len(),
        offered - 1,
        "allowing a stripped row put it back"
    );
    assert_eq!(request.playlists.len(), 1);
}

#[test]
fn choose_destructive_picks_the_last_row_only_if_destructive() {
    let mut request = request(Device::Keyboard, 2);
    request.items = vec![item(0), destructive_item("Remove from this playlist")];
    let mut menu = opened_on(request);
    assert_eq!(
        menu.choose_destructive(),
        Some(Closed {
            opener: Some(opener()),
            chose: Some(Verb::Remove),
        })
    );
    assert!(!menu.is_open());

    let mut plain = opened(Device::Keyboard, 2);
    assert_eq!(
        plain.choose_destructive(),
        None,
        "a menu with no removal chose one"
    );
    assert!(plain.is_open(), "a menu with no removal closed");
    assert_eq!(plain.lit(), Some(0), "the lit row moved");
}

fn library_row(kind: SidebarKind, uri: &str, name: &str) -> LibraryItem {
    LibraryItem {
        kind,
        uri: uri.to_owned(),
        name: name.to_owned(),
    }
}

/// A menu opened on one sidebar row, the way its door posts it.
fn library_menu(row: LibraryItem) -> Menu {
    let mut request = request(Device::Keyboard, 0);
    request.items = crate::verbs::library_items(
        &row,
        crate::verbs::LibraryRights {
            renamable: true,
            removable: true,
            saved: Some(true),
        },
    );
    request.listing = Listing::Library(row);
    request.targets = Vec::new();
    request.rows = Vec::new();
    opened_on(request)
}

#[test]
fn a_library_listing_is_never_a_cut_and_keeps_its_item() {
    let row = library_row(SidebarKind::Playlists, "spotify:playlist:a", "Deep Focus");
    let mut menu = library_menu(row.clone());

    assert_eq!(menu.cut(), None, "a sidebar row is not a pick");
    assert_eq!(menu.library_item(), Some(&row));

    assert!(menu.close(Reason::Escape).is_some());
    assert_eq!(
        menu.library_item(),
        Some(&row),
        "the shell reads the item after the menu closed on it"
    );
}

#[test]
fn del_chooses_the_library_rows_removal() {
    let mut menu = library_menu(library_row(
        SidebarKind::Albums,
        "spotify:album:r",
        "A Record",
    ));

    assert_eq!(
        menu.choose_destructive(),
        Some(Closed {
            opener: Some(opener()),
            chose: Some(Verb::RemoveFromLibrary),
        })
    );
    assert!(!menu.is_open());
}

#[test]
fn a_later_track_menu_forgets_the_library_item() {
    let mut menu = library_menu(library_row(
        SidebarKind::Podcasts,
        "spotify:show:s",
        "A Show",
    ));
    assert!(menu.close(Reason::Escape).is_some());

    assert!(menu.open(request(Device::Pointer, 2), Gate::default()));

    assert_eq!(menu.library_item(), None, "the last row's item leaked");
}
