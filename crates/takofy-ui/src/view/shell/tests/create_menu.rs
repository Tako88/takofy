use super::super::*;
use super::test_support::*;
use crate::library::Load;
use crate::message::Update;
use crate::player::Transport;
use crate::queue::Queue;
use crate::route::Page;

#[test]
fn a_create_is_refused_while_a_block_is_carried() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    creatable_sidebar(&mut window);
    open_naming(&mut window, &mut transport, &mut queue);
    type_text(&mut window, &mut transport, &mut queue, "Night Drive");
    // Any payload says a block is held; the gate reads no more than that.
    egui::DragAndDrop::set_payload(&window.ctx, ());

    assert_eq!(
        window
            .frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::Enter, true)]
            )
            .0,
        None,
        "a create went out while a block was carried"
    );
    assert!(!window.library.write_in_flight());
    assert_eq!(
        window.library.naming.text(),
        "Night Drive",
        "the dropped submit lost the name"
    );
}

#[test]
fn the_row_count_updates() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    creatable_sidebar(&mut window);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        text_in(&frame, "2 SAVED", sidebar_rect(&window)).is_some(),
        "the field's resting line is missing"
    );

    window.library.naming.open();
    window.library.naming.edit().push_str("Night Drive");
    assert!(window.library.create_playlist().is_some());
    window.library.apply(&Update::PlaylistCreated {
        entry: new_playlist("Night Drive", page_access(true)),
        revision: revision("root-2"),
    });
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

    assert!(
        text_in(&frame, "3 SAVED", sidebar_rect(&window)).is_some(),
        "the head did not count the new row"
    );
}

// ---- the menu's new-playlist row ----

/// Opens the `⋯` menu of the page row titled `title`, and answers the verbs
/// it offers.
fn open_row_menu(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    title: &str,
) -> Vec<Verb> {
    let (_, frame) = window.frame(transport, queue, Vec::new());
    let more = more_button(&frame, view_rect(window), title);
    let _ = window.click(transport, queue, more);
    settle_menu(window, transport, queue);
    assert!(window.menu.is_open(), "the `⋯` opened no menu");
    window.menu.items().iter().map(|item| item.verb).collect()
}

#[test]
fn the_menu_row_opens_the_field_and_sends_no_command() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    sidebar_playlists(&mut window, &[("a", Some(0))], Some(rootlist_access(true)));

    let offered = open_row_menu(&mut window, &mut transport, &mut queue, "p0");
    assert!(
        offered.contains(&Verb::NewPlaylist),
        "the row was stripped from a menu that could offer it: {offered:?}"
    );
    // The pointer opened the menu, so it lit nothing: the row is clicked where
    // it is painted.
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = painted_at(&frame, "New playlist…").expect("the row is painted");
    let command = window.click(
        &mut transport,
        &mut queue,
        egui::pos2(at.x + 20.0, at.y + 6.0),
    );

    assert_eq!(command, None, "the new-playlist row sent a command");
    assert!(!window.menu.is_open(), "the menu stayed open");
    assert!(
        window.library.naming.is_open(),
        "the row opened no naming field"
    );
    assert!(!window.library.write_in_flight(), "the row started a write");
}

#[test]
fn a_created_list_takes_the_pick_the_menu_held() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    page_of_tracks(&mut window, &[TRACK_A]);
    sidebar_playlists(&mut window, &[("a", Some(0))], Some(rootlist_access(true)));
    let offered = open_row_menu(&mut window, &mut transport, &mut queue, TRACK_A);
    assert!(offered.contains(&Verb::NewPlaylist), "{offered:?}");

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = painted_at(&frame, "New playlist…").expect("the row is painted");
    let _ = window.click(
        &mut transport,
        &mut queue,
        egui::pos2(at.x + 20.0, at.y + 6.0),
    );
    assert!(
        window.library.naming.is_open(),
        "the row opened no naming field"
    );
    assert_eq!(
        window.library.naming.pick().to_vec(),
        vec![TRACK_A.to_owned()],
        "the field did not take the pick the menu held"
    );

    type_text(&mut window, &mut transport, &mut queue, "Night Drive");
    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);

    assert_eq!(
        command,
        Some(Command::CreatePlaylist {
            name: "Night Drive".to_owned(),
            revision: revision("root-1"),
            tracks: vec![TRACK_A.to_owned()],
        }),
        "the pick rides the create"
    );
}

#[test]
fn the_menu_row_is_absent_in_the_rail_layout() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(800.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    sidebar_playlists(&mut window, &[("a", Some(0))], Some(rootlist_access(true)));
    assert!(window.library.creatable());

    let offered = open_row_menu(&mut window, &mut transport, &mut queue, "p0");

    assert!(
        !offered.contains(&Verb::NewPlaylist),
        "the rail offered a field it has no column for: {offered:?}"
    );
}

#[test]
fn the_menu_row_is_absent_while_the_playlists_load() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    window.library.playlists = Load::Loading;
    assert!(!window.library.creatable());

    let offered = open_row_menu(&mut window, &mut transport, &mut queue, "p0");

    assert!(
        !offered.contains(&Verb::NewPlaylist),
        "a list that is still loading offered a create: {offered:?}"
    );
}

#[test]
fn a_chosen_row_is_dropped_while_the_drawer_is_open_or_a_block_is_carried() {
    // The keyboard door, because a press that reaches a menu row closes the
    // drawer first: this is the only way to make the choice the gate guards.
    for drawer_open in [true, false] {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        sidebar_playlists(&mut window, &[("a", Some(0))], Some(rootlist_access(true)));
        let _ = click_page_row(&mut window, &mut transport, &mut queue, "p0");
        let _ = chord(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::F10,
            egui::Modifiers::SHIFT,
        );
        assert!(window.menu.is_open(), "the keyboard opened no menu");
        let offered: Vec<Verb> = window.menu.items().iter().map(|item| item.verb).collect();
        let row = offered
            .iter()
            .position(|verb| *verb == Verb::NewPlaylist)
            .expect("the row");
        for _ in 0..row {
            let _ = press(
                &mut window,
                &mut transport,
                &mut queue,
                egui::Key::ArrowDown,
            );
        }
        if drawer_open {
            window.drawer = Drawer::Open;
        } else {
            // Any payload says a block is held; the gate reads no more.
            egui::DragAndDrop::set_payload(&window.ctx, ());
        }

        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);

        assert!(
            !window.library.naming.is_open(),
            "a chosen row opened the field (drawer open: {drawer_open})"
        );
    }
}

#[test]
fn a_submit_is_dropped_when_another_region_took_the_command() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0"]);
    sidebar_playlists(&mut window, &[("a", Some(0))], Some(rootlist_access(true)));
    // An album on top, so Back returns to a playlist whose page was dropped
    // and has to be asked for again.
    window.library.open(Page::Album(ALBUM.to_owned()));
    window.library.tracks = Load::Idle;
    open_naming(&mut window, &mut transport, &mut queue);
    type_text(&mut window, &mut transport, &mut queue, "Night Drive");

    let command = window
        .frame(
            &mut transport,
            &mut queue,
            vec![
                key(egui::Key::Enter, true),
                key(egui::Key::BrowserBack, true),
            ],
        )
        .0;

    assert!(
        matches!(command, Some(Command::LoadTracks { .. })),
        "the frame's one command is the nav's: {command:?}"
    );
    assert!(
        !window.library.naming.writing(),
        "a dropped submit started a write"
    );
    assert!(!window.library.write_in_flight());
    assert_eq!(
        window.library.naming.text(),
        "Night Drive",
        "a dropped submit lost the name"
    );
}
