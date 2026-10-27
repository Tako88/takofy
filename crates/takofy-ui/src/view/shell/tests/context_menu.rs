use super::super::*;
use super::test_support::*;

#[test]
fn selection_resets_when_the_page_changes() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = row_line(&frame, view_rect(&window), "p1");
    let _ = window.click(&mut transport, &mut queue, at);

    let id = egui::Id::unique(("playlist-tracks", PLAYLIST));
    assert!(
        carry::List::open(&window.ctx, id, 3).look(1).selected,
        "the click is stored under the list's own id"
    );

    window.library.open(Page::Album(ALBUM.to_owned()));
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        !carry::List::open(&window.ctx, id, 3).look(1).selected,
        "a page change forgets every list's pick"
    );
}

#[test]
fn clicking_the_open_more_closes_its_menu_and_keeps_the_keyboard() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let more = more_button(&frame, view_rect(&window), "p0");

    let _ = window.click(&mut transport, &mut queue, more);
    settle_menu(&mut window, &mut transport, &mut queue);
    assert!(window.menu.is_open(), "the `⋯` opened no menu");

    // The same `⋯` again closes it instead of resetting it.
    let _ = window.click(&mut transport, &mut queue, more);
    assert!(
        !window.menu.is_open(),
        "the `⋯` reset its own menu instead of closing it"
    );

    // And the keyboard is back on the `⋯`, so Enter opens the menu again.
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::Enter, true)],
    );
    assert!(
        window.menu.is_open(),
        "the `⋯` did not take the keyboard back"
    );
}

#[test]
fn a_held_enter_that_opened_the_menu_fires_nothing() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let more = more_button(&frame, view_rect(&window), "p0");

    // Open from the `⋯` and close, so the button holds the keyboard.
    let _ = window.click(&mut transport, &mut queue, more);
    settle_menu(&mut window, &mut transport, &mut queue);
    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);
    assert!(!window.menu.is_open(), "Escape closed the menu");

    // Enter opens it and is held: no release comes before the repeats.
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::Enter, true)],
    );
    assert!(window.menu.is_open(), "Enter opened no menu");
    for _ in 0..3 {
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key_repeat(egui::Key::Enter)],
        );
    }

    assert!(window.menu.is_open(), "a held Enter closed the menu");
    assert_eq!(window.menu.lit(), Some(0), "a held Enter moved the lit row");
    assert_eq!(
        queue.upcoming().count(),
        0,
        "a held Enter fired Add to queue"
    );
}

#[test]
fn a_held_enter_after_choosing_does_not_reopen_the_menu() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let more = more_button(&frame, view_rect(&window), "p0");

    let _ = window.click(&mut transport, &mut queue, more);
    settle_menu(&mut window, &mut transport, &mut queue);
    let _ = press(
        &mut window,
        &mut transport,
        &mut queue,
        egui::Key::ArrowDown,
    );
    assert_eq!(window.menu.lit(), Some(0));

    // Enter chooses, closes and hands the keyboard back to the `⋯`.
    let chosen = window
        .frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Enter, true)],
        )
        .0;
    assert_eq!(chosen, None, "adding to the queue asks for no command");
    assert!(!window.menu.is_open(), "the choice left the menu open");
    assert_eq!(queue.upcoming().count(), 1, "the choice queued nothing");

    // Held, the repeats land on the `⋯` that now holds the keyboard.
    for _ in 0..3 {
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key_repeat(egui::Key::Enter)],
        );
    }
    assert!(
        !window.menu.is_open(),
        "a held Enter reopened the menu it chose from"
    );
    assert_eq!(
        queue.upcoming().count(),
        1,
        "a held Enter queued the row again"
    );
}

#[test]
fn the_menu_survives_idle_frames() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = row_line(&frame, view_rect(&window), "p1");
    for event in crate::view::test_support::secondary_click_events(at) {
        let _ = window.frame(&mut transport, &mut queue, vec![event]);
    }
    assert!(window.menu.is_open(), "the right-click opened no menu");
    for index in 0..10 {
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            window.menu.is_open(),
            "the menu closed on idle frame {index}"
        );
    }
    // The window losing the OS keyboard is not the menu's business until the
    // user acts.
    window.unfocused = true;
    for index in 0..3 {
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            window.menu.is_open(),
            "the menu closed on unfocused frame {index}"
        );
    }
}

#[test]
fn a_right_click_on_a_playlist_row_opens_the_menu_the_shell_draws() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = row_line(&frame, view_rect(&window), "p1");

    // The row posts a request; the shell opens the menu on it in the same
    // frame. The area's first frame is its sizing pass, and its second is
    // the opening frame, which ignores keys.
    for event in crate::view::test_support::secondary_click_events(at) {
        let _ = window.frame(&mut transport, &mut queue, vec![event]);
    }
    assert!(window.menu.is_open(), "the right-click opened no menu");
    settle_menu(&mut window, &mut transport, &mut queue);

    // The pointer opened it, so nothing is lit until the keyboard asks.
    assert_eq!(window.menu.lit(), None);
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::ArrowDown, true)],
    );
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::ArrowDown, false)],
    );
    assert_eq!(window.menu.lit(), Some(0));

    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
        None,
        "adding to the queue asks for no command"
    );
    assert!(!window.menu.is_open());
    let queued: Vec<&str> = queue
        .upcoming()
        .map(|(_, entry)| entry.uri.as_str())
        .collect();
    assert_eq!(queued, vec!["list:p1"], "the row the menu was opened on");
}

#[test]
fn a_chosen_add_to_queue_appends_and_closes() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    open_menu(
        &mut window,
        Verb::AddToQueue,
        vec![menu_track(0)],
        Listing::Page,
        vec![0],
    );
    settle_menu(&mut window, &mut transport, &mut queue);

    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
        None,
        "adding to the queue asks for no command"
    );
    assert!(
        !window.menu.is_open(),
        "the menu stayed open after the choice"
    );
    let queued: Vec<&str> = queue
        .upcoming()
        .map(|(_, entry)| entry.uri.as_str())
        .collect();
    assert_eq!(queued, vec!["spotify:track:0"]);
}

#[test]
fn a_chosen_copy_link_emits_the_joined_urls() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    open_menu(
        &mut window,
        Verb::CopyLink,
        vec![menu_track(0), menu_track(1)],
        Listing::Page,
        vec![0, 1],
    );
    settle_menu(&mut window, &mut transport, &mut queue);

    let (_, frame) = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::Enter, true)],
    );
    let copied = frame
        .platform_output
        .commands
        .iter()
        .find_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.as_str()),
            _ => None,
        });
    assert_eq!(
        copied,
        Some("https://open.spotify.com/track/0\nhttps://open.spotify.com/track/1"),
        "one URL per line"
    );
}

#[test]
fn a_chosen_go_to_opens_the_page() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    open_menu(
        &mut window,
        Verb::GoToAlbum,
        vec![menu_track(0)],
        Listing::Page,
        vec![0],
    );
    settle_menu(&mut window, &mut transport, &mut queue);

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
    assert_eq!(
        window.library.page(),
        Some(&Page::Album(ALBUM.to_owned())),
        "the page did not open"
    );
    assert!(command.is_some(), "the page's read was not asked for");
    assert!(!window.menu.is_open());
}

#[test]
fn choosing_save_issues_one_command_for_the_pick_and_flips_every_effective_row() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    liked(&mut window, &[]);
    open_menu(
        &mut window,
        Verb::Save,
        vec![menu_track(0), menu_track(1)],
        Listing::Page,
        vec![0, 1],
    );
    settle_menu(&mut window, &mut transport, &mut queue);

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);

    assert_eq!(
        command,
        Some(Command::SetLiked {
            tracks: vec!["spotify:track:0".to_owned(), "spotify:track:1".to_owned()],
            liked: true,
        })
    );
    assert_eq!(window.library.likes.state("spotify:track:0"), Some(true));
    assert_eq!(window.library.likes.state("spotify:track:1"), Some(true));
    assert!(!window.menu.is_open());
}

#[test]
fn choosing_remove_from_your_library_unlikes() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    liked(&mut window, &["spotify:track:0"]);
    open_menu(
        &mut window,
        Verb::Unsave,
        vec![menu_track(0)],
        Listing::Page,
        vec![0],
    );
    settle_menu(&mut window, &mut transport, &mut queue);

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);

    assert_eq!(
        command,
        Some(Command::SetLiked {
            tracks: vec!["spotify:track:0".to_owned()],
            liked: false,
        })
    );
    assert_eq!(window.library.likes.state("spotify:track:0"), Some(false));
}

#[test]
fn a_save_chosen_while_a_like_is_pending_is_dropped() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    liked(&mut window, &[]);
    assert!(
        window
            .library
            .set_liked(vec!["spotify:track:9".to_owned()], true)
            .is_some()
    );
    open_menu(
        &mut window,
        Verb::Save,
        vec![menu_track(1)],
        Listing::Page,
        vec![0],
    );
    settle_menu(&mut window, &mut transport, &mut queue);

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);

    assert_eq!(command, None, "a dropped save sent a command");
    assert_eq!(window.library.likes.state("spotify:track:1"), Some(false));
}

#[test]
fn a_pending_like_does_not_hold_a_menu_shut() {
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    liked(&mut window, &[]);
    assert!(
        window
            .library
            .set_liked(vec!["spotify:track:9".to_owned()], true)
            .is_some()
    );
    assert!(
        !window.library.write_in_flight(),
        "a like is not a playlist write"
    );
    open_menu(
        &mut window,
        Verb::Save,
        vec![menu_track(1)],
        Listing::Page,
        vec![0],
    );
    assert!(window.menu.is_open(), "a like held the menu shut");
}

#[test]
fn a_playlist_write_does_not_stop_a_like() {
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    liked(&mut window, &[]);
    let carried = page_entries(&window, &[0]);
    assert!(
        window
            .library
            .remove_tracks(PLAYLIST, &[0], &carried)
            .is_some()
    );
    assert!(
        window
            .library
            .set_liked(vec!["spotify:track:0".to_owned()], true)
            .is_some(),
        "a playlist write stopped a like"
    );
}

#[test]
fn a_menu_opened_before_the_set_is_ready_has_no_library_row() {
    let window = Window::new(Vec2::new(1400.0, 900.0));
    let targets = vec![menu_track(0)];
    let uris: Vec<&str> = targets.iter().map(|entry| entry.uri.as_str()).collect();
    let standing = window.library.likes.standing(&uris);
    assert_eq!(standing, crate::likes::Standing::Unknown);
    let items = crate::verbs::track_items(&targets, None, &Listing::Page, standing);
    assert!(
        items
            .iter()
            .all(|item| !matches!(item.verb, Verb::Save | Verb::Unsave)),
        "a set nobody has read offered a library row"
    );
}

#[test]
fn an_applied_ask_issues_set_liked_and_flips_the_row() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    liked(&mut window, &[]);
    crate::view::heart::post(
        &window.ctx,
        crate::view::heart::Ask {
            tracks: vec!["spotify:track:a".to_owned()],
            liked: true,
        },
    );

    let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());

    assert_eq!(
        command,
        Some(Command::SetLiked {
            tracks: vec!["spotify:track:a".to_owned()],
            liked: true,
        })
    );
    assert_eq!(window.library.likes.state("spotify:track:a"), Some(true));
    assert!(
        crate::view::heart::take(&window.ctx).is_none(),
        "the ask was not taken"
    );
}

#[test]
fn an_ask_with_the_drawer_open_is_dropped() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    liked(&mut window, &[]);
    window.drawer = Drawer::Open;
    crate::view::heart::post(
        &window.ctx,
        crate::view::heart::Ask {
            tracks: vec!["spotify:track:a".to_owned()],
            liked: true,
        },
    );

    let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());

    assert_eq!(command, None, "a dropped ask sent a command");
    assert_eq!(window.library.likes.state("spotify:track:a"), Some(false));
    assert!(
        crate::view::heart::take(&window.ctx).is_none(),
        "a dropped ask outlived its frame"
    );
}

#[test]
fn a_page_change_closes_the_menu() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    open_menu(
        &mut window,
        Verb::AddToQueue,
        vec![menu_track(0)],
        Listing::Page,
        vec![0],
    );
    settle_menu(&mut window, &mut transport, &mut queue);
    assert!(window.menu.is_open());

    let _ = window.library.open(Page::Playlist(PLAYLIST.to_owned()));
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        !window.menu.is_open(),
        "the menu outlived the page its rows came from"
    );
}

#[test]
fn a_click_on_now_playings_more_opens_a_current_menu() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    queue.start(&[menu_track(0)], 0, None, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let _ = window.frame(&mut transport, &mut queue, Vec::new());

    let at = now_playing_more(&window);
    let _ = window.click(&mut transport, &mut queue, at);
    settle_menu(&mut window, &mut transport, &mut queue);

    assert!(window.menu.is_open(), "the more button opened no menu");
    assert!(
        window
            .menu
            .items()
            .iter()
            .all(|item| { item.verb != Verb::AddToQueue && item.verb != Verb::Remove }),
        "Now Playing offered a list command"
    );
    assert_eq!(window.menu.cut(), None, "the entry was removable");
    assert_eq!(
        window
            .menu
            .items()
            .iter()
            .map(|item| item.verb)
            .collect::<Vec<_>>(),
        [Verb::GoToAlbum, Verb::GoToArtist, Verb::CopyLink]
    );
}

#[test]
fn now_playings_more_opens_from_the_keyboard_after_a_click() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    queue.start(&[menu_track(0)], 0, None, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    let at = now_playing_more(&window);

    // The press opens the menu and leaves the keyboard on the button; the
    // Escape hands it back there, so the key door has something to work from.
    let _ = window.click(&mut transport, &mut queue, at);
    settle_menu(&mut window, &mut transport, &mut queue);
    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);
    assert!(!window.menu.is_open());

    let _ = chord(
        &mut window,
        &mut transport,
        &mut queue,
        egui::Key::F10,
        egui::Modifiers::SHIFT,
    );

    assert!(window.menu.is_open(), "Shift+F10 opened no menu");
}

#[test]
fn now_playings_more_waits_without_a_current_entry() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let _ = window.frame(&mut transport, &mut queue, Vec::new());

    let at = now_playing_more(&window);
    let _ = window.click(&mut transport, &mut queue, at);

    assert!(
        !window.menu.is_open(),
        "a waiting more button opened a menu"
    );
}

// ---- the library head's new-playlist field ----
