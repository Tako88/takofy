use super::super::*;
use super::test_support::*;

/// The page's listing, under the id its rows are picked by.
fn playlist_listing() -> Listing {
    Listing::Playlist {
        uri: PLAYLIST.to_owned(),
        list: egui::Id::unique(("playlist-tracks", PLAYLIST)),
    }
}

#[test]
fn a_chosen_remove_on_a_playlist_issues_the_command_and_forgets_the_pick() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    // A frame first, so the page's own list is the one the picks are kept
    // against.
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    let listing = playlist_listing();
    let Listing::Playlist { list, .. } = listing.clone() else {
        panic!("the page is a playlist");
    };
    carry::select_rows(&window.ctx, list, 0..2);
    let targets = page_entries(&window, &[0]);
    open_menu(&mut window, Verb::Remove, targets, listing, vec![0]);
    settle_menu(&mut window, &mut transport, &mut queue);

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);

    assert_eq!(
        command,
        Some(Command::RemoveTracks {
            playlist: PLAYLIST.to_owned(),
            revision: revision("rev-1"),
            removals: vec![Removal { from: 0, length: 1 }],
        })
    );
    assert_eq!(page_rows(&window), vec!["list:p1", "list:p2"]);
    let list = carry::List::open(&window.ctx, list, 2);
    assert!(
        list.look(0).selected,
        "the pick did not shift with the rows"
    );
    assert!(
        !list.look(1).selected,
        "the removed row's pick survived the removal"
    );
}

#[test]
fn a_chosen_remove_on_the_queue_removes_the_rows_and_refocuses() {
    let mut transport = Transport::default();
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let targets = queue.entries_at(&[1]);
    open_menu(&mut window, Verb::Remove, targets, Listing::Queue, vec![1]);
    settle_menu(&mut window, &mut transport, &mut queue);

    // One press, so the row that takes the keyboard back still names it.
    let (command, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::Enter, true)],
    );

    assert_eq!(command, None, "the queue removes locally");
    assert_eq!(entries(&queue), vec!["t0", "t2", "t3"]);
    assert_eq!(queue.refocus(), Some(1));
    assert!(!window.menu.is_open());
    assert!(
        window
            .ctx
            .memory(|memory| memory.has_focus(egui::Id::unique("row-more"))),
        "the keyboard did not go back to the button"
    );
}

#[test]
fn delete_in_the_open_menu_chooses_the_removal() {
    let mut transport = Transport::default();
    let mut queue = queue_of(3, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let targets = queue.entries_at(&[1]);
    open_menu(&mut window, Verb::Remove, targets, Listing::Queue, vec![1]);
    settle_menu(&mut window, &mut transport, &mut queue);

    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

    assert!(!window.menu.is_open(), "Del left the menu open");
    assert_eq!(entries(&queue), vec!["t0", "t2"]);
}

#[test]
fn a_stale_queue_cut_removes_nothing_and_leaves_the_rows() {
    let mut transport = Transport::default();
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let targets = queue.entries_at(&[1]);
    let _ = window.frame(&mut transport, &mut queue, Vec::new());
    // The cut was made while row 1 was upcoming, and the queue moved on.
    let _ = queue.next(&mut transport);
    context_menu::post_cut(
        &window.ctx,
        Cut {
            listing: Listing::Queue,
            rows: vec![1],
            entries: targets,
        },
    );

    let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());

    assert_eq!(command, None, "a refused queue cut reported a command");
    assert_eq!(entries(&queue), vec!["t0", "t1", "t2", "t3"]);
    assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("t1"));
    assert!(!window.menu.is_open());
}

#[test]
fn a_queue_removal_is_not_blocked_by_a_playlist_write() {
    let mut transport = Transport::default();
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let carried = page_entries(&window, &[0]);
    assert!(
        window
            .library
            .remove_tracks(PLAYLIST, &[0], &carried)
            .is_some(),
        "the page started no write"
    );
    assert!(window.library.write_in_flight());
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = row_line(&frame, pane_rect_when_shown(&window), "t2");
    for event in [
        egui::Event::PointerMoved(at),
        side(at, egui::PointerButton::Secondary, true),
        side(at, egui::PointerButton::Secondary, false),
    ] {
        let _ = window.frame(&mut transport, &mut queue, vec![event]);
    }
    settle_menu(&mut window, &mut transport, &mut queue);

    assert!(
        window.menu.is_open(),
        "a playlist write refused the queue's menu"
    );
    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

    assert_eq!(command, None);
    assert_eq!(entries(&queue), vec!["t0", "t1", "t3"]);
    assert!(
        window.library.write_in_flight(),
        "the queue's removal settled the playlist write"
    );
}

#[test]
fn a_queue_delete_while_a_playlist_write_is_pending_still_removes() {
    let mut transport = Transport::default();
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let carried = page_entries(&window, &[0]);
    assert!(
        window
            .library
            .remove_tracks(PLAYLIST, &[0], &carried)
            .is_some(),
        "the page started no write"
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = row_line(&frame, pane_rect_when_shown(&window), "t2");
    let _ = window.click(&mut transport, &mut queue, at);

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

    assert_eq!(command, None, "the queue's Del reported a command");
    assert_eq!(entries(&queue), vec!["t0", "t1", "t3"]);
    assert!(
        window.library.write_in_flight(),
        "the queue's Del settled the playlist write"
    );
}

#[test]
fn a_menu_without_a_removal_ignores_delete() {
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

    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

    assert!(
        window.menu.is_open(),
        "a menu with no removal closed on Del"
    );
    assert_eq!(queue.upcoming().count(), 0, "the Del fired a verb");
}

#[test]
fn a_refused_cut_leaves_the_rows_alone() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let targets = page_entries(&window, &[0]);
    open_menu(
        &mut window,
        Verb::Remove,
        targets,
        playlist_listing(),
        vec![0],
    );
    settle_menu(&mut window, &mut transport, &mut queue);

    // The drawer owns the frame, so the shell drops the cut rather than
    // writing behind it.
    window.drawer = Drawer::Open;
    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);

    assert_eq!(command, None);
    assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
    assert!(
        !window.library.write_in_flight(),
        "a dropped cut left a write in flight"
    );
}
#[test]
fn a_right_click_on_an_editable_playlist_offers_remove_last() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);

    right_click_page_row(&mut window, &mut transport, &mut queue, "p0");
    settle_menu(&mut window, &mut transport, &mut queue);

    assert!(window.menu.is_open(), "the right-click opened no menu");
    let last = window.menu.items().last().expect("a command");
    assert_eq!(last.verb, Verb::Remove, "the last row is not the removal");
    assert_eq!(last.label, "Remove from this playlist");
    assert_eq!(last.key, Some("Del"));
    assert!(
        window.menu.cut().is_some_and(|cut| cut.rows == vec![0]),
        "the menu is not about the row it was opened on"
    );
}

#[test]
fn a_right_click_on_a_read_only_page_offers_no_remove() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    read_only_playlist_page(&mut window, &["p0", "p1"]);

    right_click_page_row(&mut window, &mut transport, &mut queue, "p0");
    settle_menu(&mut window, &mut transport, &mut queue);

    assert!(window.menu.is_open(), "the right-click opened no menu");
    assert!(
        window
            .menu
            .items()
            .iter()
            .all(|item| item.verb != Verb::Remove),
        "a list that is not ours offered a removal"
    );
    assert_eq!(window.menu.cut(), None);
}

#[test]
fn a_focused_rows_delete_removes_the_row() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    let _ = click_page_row(&mut window, &mut transport, &mut queue, "p1");

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

    assert_eq!(
        command,
        Some(Command::RemoveTracks {
            playlist: PLAYLIST.to_owned(),
            revision: revision("rev-1"),
            removals: vec![Removal { from: 1, length: 1 }],
        })
    );
    assert_eq!(page_rows(&window), vec!["list:p0", "list:p2"]);
}

#[test]
fn a_focused_row_outside_the_pick_removes_only_itself() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    let _ = click_page_row(&mut window, &mut transport, &mut queue, "p0");
    carry::select_rows(
        &window.ctx,
        egui::Id::unique(("playlist-tracks", PLAYLIST)),
        1..3,
    );

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

    assert_eq!(
        command,
        Some(Command::RemoveTracks {
            playlist: PLAYLIST.to_owned(),
            revision: revision("rev-1"),
            removals: vec![Removal { from: 0, length: 1 }],
        }),
        "Del acted on a pick the focused row is not in"
    );
    assert_eq!(page_rows(&window), vec!["list:p1", "list:p2"]);
}

#[test]
fn delete_with_no_focused_row_removes_nothing() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

    assert_eq!(command, None);
    assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
    assert!(!window.library.write_in_flight());
}

#[test]
fn a_focused_more_button_does_not_answer_delete() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let more = more_button(&frame, view_rect(&window), "p0");
    let _ = window.click(&mut transport, &mut queue, more);

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

    assert_eq!(
        command, None,
        "the focused `⋯` answered Del as if it were the row"
    );
    assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
}

#[test]
fn shift_or_ctrl_delete_is_not_this_key() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let _ = click_page_row(&mut window, &mut transport, &mut queue, "p0");

    for modifiers in [egui::Modifiers::SHIFT, egui::Modifiers::COMMAND] {
        let command = chord(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::Delete,
            modifiers,
        );
        assert_eq!(command, None, "{modifiers:?}+Del removed a row");
    }
    assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
}

#[test]
fn a_held_delete_removes_once() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    let _ = click_page_row(&mut window, &mut transport, &mut queue, "p1");

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);
    assert!(command.is_some(), "the first Del removed nothing");
    for _ in 0..3 {
        let (held, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![key_repeat(egui::Key::Delete)],
        );
        assert_eq!(held, None, "a held Del removed a second row");
    }
    assert_eq!(page_rows(&window), vec!["list:p0", "list:p2"]);
}

#[test]
fn delete_on_a_filtered_selection_cuts_exactly_the_picked_visible_rows() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(
        &mut window,
        &[
            "p0", "p1", "keep2", "p3", "p4", "keep5", "p6", "p7", "p8", "keep9",
        ],
    );
    window.library.track_filter.edit(PLAYLIST).push_str("keep");
    let expected: Vec<String> = page_entries(&window, &[2, 5, 9])
        .into_iter()
        .map(|entry| entry.title)
        .collect();
    assert_eq!(
        expected,
        ["keep2", "keep5", "keep9"],
        "the page's true rows 2, 5 and 9 are the kept ones"
    );

    let _ = click_page_row(&mut window, &mut transport, &mut queue, "keep2");
    window.held = egui::Modifiers::COMMAND;
    let _ = click_page_row(&mut window, &mut transport, &mut queue, "keep5");
    let _ = click_page_row(&mut window, &mut transport, &mut queue, "keep9");
    window.held = egui::Modifiers::NONE;

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

    assert_eq!(
        command,
        Some(Command::RemoveTracks {
            playlist: PLAYLIST.to_owned(),
            revision: revision("rev-1"),
            removals: vec![
                Removal { from: 9, length: 1 },
                Removal { from: 5, length: 1 },
                Removal { from: 2, length: 1 },
            ],
        }),
        "one cut, highest from first, on the true indices"
    );
    assert_eq!(
        page_rows(&window),
        vec![
            "list:p0", "list:p1", "list:p3", "list:p4", "list:p6", "list:p7", "list:p8"
        ],
        "and exactly those rows left the page"
    );
}

#[test]
fn delete_in_the_filter_field_edits_text_and_removes_no_row() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let field = page_filter_field(&frame, view_rect(&window)).expect("the filter is painted");
    let _ = window.click(&mut transport, &mut queue, field.center());
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::Text("ab".to_owned())],
    );
    // Del forward-deletes, and the caret sits after the text, so one step
    // left puts a character in front of it.
    let _ = press(
        &mut window,
        &mut transport,
        &mut queue,
        egui::Key::ArrowLeft,
    );

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

    assert_eq!(command, None);
    assert_eq!(
        window.library.track_filter.edit(PLAYLIST).as_str(),
        "a",
        "the field kept the key"
    );
    assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
}

#[test]
fn a_filtered_drag_inside_the_page_moves_nothing() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["a", "keep0", "b", "keep1"]);
    window.library.track_filter.edit(PLAYLIST).push_str("keep");
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let view = view_rect(&window);
    let from = row_line(&frame, view, "keep0");
    let to = page_boundary(&frame, view, &["keep0", "keep1"], 2);

    assert_eq!(
        window.drag(&mut transport, &mut queue, from, to),
        None,
        "a filtered page reorders nothing"
    );
    let before = vec!["list:a", "list:keep0", "list:b", "list:keep1"];
    assert_eq!(page_rows(&window), before);
    assert!(
        window.library.movable(),
        "the refused drag left a write pending"
    );

    // The same gesture, once the query is gone, is the one that moves.
    window.library.track_filter.clear();
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view, "keep0");
    let to = page_boundary(&frame, view, &["keep0", "keep1"], 2);
    assert!(
        window.drag(&mut transport, &mut queue, from, to).is_some(),
        "clearing the query restores the reorder"
    );
    assert_ne!(
        page_rows(&window),
        before,
        "the same drag now moves the row"
    );
}

#[test]
fn alt_arrows_on_a_filtered_page_move_no_page_row() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let titles = ["a", "keep0", "b", "keep1"];
    playlist_page(&mut window, &titles);
    window.library.track_filter.edit(PLAYLIST).push_str("keep");
    let _ = click_page_row(&mut window, &mut transport, &mut queue, "keep0");

    // The page really is narrowed, so the guard below cannot pass on a page
    // that never filtered. `page_rows` reads the list itself, which keeps
    // all four rows; the painted titles are what the user sees.
    assert_eq!(
        page_rows(&window),
        titles.map(|title| format!("list:{title}")),
        "the page's own list is whole"
    );
    let shown = painted_titles(&mut window, &mut transport, &mut queue, &titles);
    assert_eq!(shown, vec!["keep0", "keep1"], "the query narrows the page");

    let command = chord(
        &mut window,
        &mut transport,
        &mut queue,
        egui::Key::ArrowDown,
        egui::Modifiers::ALT,
    );

    // There is no page-level Alt handler: `alt_step` moves the queue's rows
    // only, so this is a regression guard for the day one is added rather
    // than a test of a page feature.
    assert_eq!(command, None);
    assert_eq!(
        painted_titles(&mut window, &mut transport, &mut queue, &titles),
        shown,
        "Alt+↓ is the queue's, never a page row's"
    );
    assert_eq!(
        page_rows(&window),
        titles.map(|title| format!("list:{title}")),
        "Alt+↓ leaves the list's own order alone"
    );
    assert_eq!(
        window.library.track_filter.needle(PLAYLIST).as_deref(),
        Some("keep"),
        "the Alt chord does not clear the query either"
    );
}

#[test]
fn del_in_the_search_field_edits_text_and_removes_no_row() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let _ = click_page_row(&mut window, &mut transport, &mut queue, "p0");
    let at = field_box(&mut window, &mut transport, &mut queue).center();
    let _ = window.click(&mut transport, &mut queue, at);
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::Text("abc".to_owned())],
    );
    // Del forward-deletes, and the caret sits after the text, so one step
    // left puts a character in front of it.
    let _ = press(
        &mut window,
        &mut transport,
        &mut queue,
        egui::Key::ArrowLeft,
    );

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

    assert_eq!(command, None);
    assert_eq!(window.library.search.text, "ab", "the field lost the key");
    assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
}

#[test]
fn a_cut_posted_while_a_field_is_typing_is_dropped() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let at = field_box(&mut window, &mut transport, &mut queue).center();
    let _ = window.click(&mut transport, &mut queue, at);

    context_menu::post_cut(&window.ctx, page_cut(&window, &[0]));
    let _ = window.frame(&mut transport, &mut queue, Vec::new());

    assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
    assert!(!window.library.write_in_flight(), "a write went out");
}

#[test]
fn a_cut_posted_with_the_drawer_open_removes_nothing() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(800.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    window.drawer = Drawer::Open;

    context_menu::post_cut(&window.ctx, page_cut(&window, &[0]));
    let _ = window.frame(&mut transport, &mut queue, Vec::new());

    assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
}

#[test]
fn a_cut_posted_while_carrying_removes_nothing() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    // Any payload says a block is held; the gate reads no more than that.
    egui::DragAndDrop::set_payload(&window.ctx, ());

    context_menu::post_cut(&window.ctx, page_cut(&window, &[0]));
    let _ = window.frame(&mut transport, &mut queue, Vec::new());

    assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
}

#[test]
fn a_cut_posted_while_a_write_is_pending_removes_nothing() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    assert!(
        window
            .library
            .remove_tracks(PLAYLIST, &[0], &page_entries(&window, &[0]))
            .is_some()
    );

    context_menu::post_cut(&window.ctx, page_cut(&window, &[0]));
    let _ = window.frame(&mut transport, &mut queue, Vec::new());

    assert_eq!(
        page_rows(&window),
        vec!["list:p1", "list:p2"],
        "a second removal went out behind the first"
    );
}

#[test]
fn delete_on_a_read_only_page_does_nothing() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    read_only_playlist_page(&mut window, &["p0", "p1"]);
    let _ = click_page_row(&mut window, &mut transport, &mut queue, "p0");

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

    assert_eq!(command, None);
    assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
}

#[test]
fn del_with_the_menu_open_chooses_the_menu_row_not_the_page() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    let _ = click_page_row(&mut window, &mut transport, &mut queue, "p0");
    let targets = page_entries(&window, &[2]);
    open_menu(
        &mut window,
        Verb::Remove,
        targets,
        playlist_listing(),
        vec![2],
    );
    settle_menu(&mut window, &mut transport, &mut queue);

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

    assert_eq!(
        command,
        Some(Command::RemoveTracks {
            playlist: PLAYLIST.to_owned(),
            revision: revision("rev-1"),
            removals: vec![Removal { from: 2, length: 1 }],
        }),
        "the page's focused row answered a Del the menu owned"
    );
    assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
}

#[test]
fn a_cut_on_the_frame_a_menu_opened_is_dropped() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let _ = window.frame(&mut transport, &mut queue, Vec::new());

    context_menu::post(&window.ctx, page_request(&window, &[1]));
    context_menu::post_cut(&window.ctx, page_cut(&window, &[0]));
    let _ = window.frame(&mut transport, &mut queue, Vec::new());

    assert!(window.menu.is_open(), "the request did not open a menu");
    assert_eq!(
        page_rows(&window),
        vec!["list:p0", "list:p1"],
        "a cut landed under a menu that opened over it"
    );
}

#[test]
fn a_cut_is_dropped_when_another_region_took_the_command() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 3);
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::ArrowDown, true)],
    );
    context_menu::post_cut(&window.ctx, page_cut(&window, &[0]));

    // The search menu's Enter is the frame's command, so the cut is spent
    // with it rather than applied behind it.
    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "t0".to_owned()
        }),
        "the search menu did not take the frame"
    );
    assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
    assert!(!window.library.write_in_flight());
}
#[test]
fn a_right_click_on_a_queue_row_opens_the_queue_menu_and_removes_it() {
    let mut transport = Transport::default();
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = row_line(&frame, pane_rect_when_shown(&window), "t2");

    for event in [
        egui::Event::PointerMoved(at),
        side(at, egui::PointerButton::Secondary, true),
        side(at, egui::PointerButton::Secondary, false),
    ] {
        let _ = window.frame(&mut transport, &mut queue, vec![event]);
    }

    settle_menu(&mut window, &mut transport, &mut queue);
    assert!(window.menu.is_open(), "the right-click opened no menu");
    assert!(
        window
            .menu
            .items()
            .iter()
            .all(|item| item.verb != Verb::AddToQueue),
        "the queue offered to add what is already there"
    );
    let last = window.menu.items().last().expect("a command");
    assert_eq!(last.verb, Verb::Remove);
    assert_eq!(last.label, "Remove from the queue");

    // A pointer-opened menu lights nothing, so Enter is not its to fire;
    // Del is the destructive row's key.
    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

    assert_eq!(entries(&queue), vec!["t0", "t1", "t3"]);
    assert!(!window.menu.is_open(), "the menu outlived its choice");
}

#[test]
fn shift_f10_on_a_focused_queue_row_opens_the_queue_menu() {
    let mut transport = Transport::default();
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = row_line(&frame, pane_rect_when_shown(&window), "t2");
    let _ = window.click(&mut transport, &mut queue, at);
    assert!(!window.menu.is_open(), "a plain click opened a menu");

    let _ = chord(
        &mut window,
        &mut transport,
        &mut queue,
        egui::Key::F10,
        egui::Modifiers::SHIFT,
    );

    assert!(window.menu.is_open(), "Shift+F10 opened no menu");
    assert_eq!(
        window.menu.cut().map(|cut| cut.rows),
        Some(vec![2]),
        "the menu is not about the focused row"
    );
    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);
    assert!(!window.menu.is_open());
}

/// The row titles of `titles` that the page painted, in `titles`' order.
///
/// `page_rows` reads the whole list; this reads what the user sees, so it
/// can tell a narrowed page from an unfiltered one.
fn painted_titles(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    titles: &[&str],
) -> Vec<String> {
    let (_, frame) = window.frame(transport, queue, Vec::new());
    let area = view_rect(window);
    titles
        .iter()
        .filter(|title| text_in(&frame, title, area).is_some())
        .map(|title| (*title).to_owned())
        .collect()
}

/// Right-clicks the playlist page's row titled `title`, the way a mouse
/// does: move, press, release.
fn right_click_page_row(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    title: &str,
) {
    let (_, frame) = window.frame(transport, queue, Vec::new());
    let at = row_line(&frame, view_rect(window), title);
    for event in [
        egui::Event::PointerMoved(at),
        side(at, egui::PointerButton::Secondary, true),
        side(at, egui::PointerButton::Secondary, false),
    ] {
        let _ = window.frame(transport, queue, vec![event]);
    }
}

/// A cut the page would post for `rows`, built from the page's own entries.
fn page_cut(window: &Window, rows: &[usize]) -> Cut {
    Cut {
        listing: playlist_listing(),
        rows: rows.to_vec(),
        entries: page_entries(window, rows),
    }
}

/// A request to open the menu on the page's rows, as a door posts it.
fn page_request(window: &Window, rows: &[usize]) -> crate::menu::Request {
    let targets = page_entries(window, rows);
    crate::menu::Request {
        door: crate::menu::Door {
            anchor: crate::menu::Anchor::Button(Rect::from_min_size(
                egui::Pos2::new(600.0, 300.0),
                Vec2::new(28.0, 28.0),
            )),
            opener: egui::Id::unique("row-more"),
            device: crate::menu::Device::Keyboard,
        },
        items: crate::verbs::track_items(
            &targets,
            None,
            &playlist_listing(),
            crate::likes::Standing::Unknown,
        ),
        playlists: Vec::new(),
        targets,
        rows: rows.to_vec(),
        listing: playlist_listing(),
        page: window.library.page().cloned(),
    }
}
