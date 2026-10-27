use super::super::*;
use super::test_support::*;
use crate::player::{NowPlaying, Transport};
use crate::queue::Queue;
use crate::route::Page;
use crate::theme::extent;
use takofy_model::{Move, TrackEntry};

pub(super) const QUEUE_ALBUM: &str = "spotify:album:q";
pub(super) const QUEUE_ARTIST: &str = "spotify:artist:q";

#[test]
fn a_queue_link_opens_the_page() {
    let mut transport = playing(cover("abc"));
    let mut queue = linked_queue(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = on_the_words(&frame, pane_rect_when_shown(&window), "t2");

    assert_eq!(
        click_words(&mut window, &mut transport, &mut queue, at),
        Some(Command::LoadAlbum {
            album: QUEUE_ALBUM.to_owned()
        }),
        "a click on a queue row's name opens its album"
    );
    assert_eq!(
        window.library.page(),
        Some(&Page::Album(QUEUE_ALBUM.to_owned())),
        "and the page is the album it named"
    );
}

#[test]
fn a_now_playing_link_opens_the_page() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    transport.now_playing = Some(NowPlaying {
        album_uri: Some(ALBUM.to_owned()),
        ..NowPlaying::named("spotify:track:x", "A Title", "A Band")
    });
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = on_the_words(&frame, pane_rect_when_shown(&window), "A Title");

    assert_eq!(
        click_words(&mut window, &mut transport, &mut queue, at),
        Some(Command::LoadAlbum {
            album: ALBUM.to_owned()
        }),
        "a click on the panel's title opens its album"
    );
    assert_eq!(
        window.library.page(),
        Some(&Page::Album(ALBUM.to_owned())),
        "and the page is the album it named"
    );
}

#[test]
fn a_now_playing_artist_link_opens_the_page() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    transport.now_playing = Some(NowPlaying {
        artist_uri: Some(ARTIST.to_owned()),
        ..NowPlaying::named("spotify:track:x", "A Title", "A Band")
    });
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = on_the_words(&frame, pane_rect_when_shown(&window), "A Band");

    assert_eq!(
        click_words(&mut window, &mut transport, &mut queue, at),
        Some(Command::LoadArtist {
            artist: ARTIST.to_owned()
        }),
        "a click on the panel's artist opens their page"
    );
    assert_eq!(
        window.library.page(),
        Some(&Page::Artist(ARTIST.to_owned())),
        "and the page is the artist it named"
    );
}

#[test]
fn a_queue_artist_link_opens_the_page() {
    let mut transport = playing(cover("abc"));
    let mut queue = linked_queue(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = on_the_words(&frame, pane_rect_when_shown(&window), "Band 2");

    assert_eq!(
        click_words(&mut window, &mut transport, &mut queue, at),
        Some(Command::LoadArtist {
            artist: QUEUE_ARTIST.to_owned()
        }),
        "a click on a queue row's artist opens their page"
    );
    assert_eq!(
        window.library.page(),
        Some(&Page::Artist(QUEUE_ARTIST.to_owned())),
        "and the page is the artist it named"
    );
}

#[test]
fn a_playbar_link_opens_the_page() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    transport.now_playing = Some(NowPlaying {
        artist_uri: Some(ARTIST.to_owned()),
        ..NowPlaying::named("spotify:track:x", "A Title", "A Band")
    });
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (bar, _) = playbar_parts(&window);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = on_the_words(&frame, bar, "A Band");

    assert_eq!(
        click_words(&mut window, &mut transport, &mut queue, at),
        Some(Command::LoadArtist {
            artist: ARTIST.to_owned()
        }),
        "a click on the playbar's artist opens their page"
    );
    assert_eq!(
        window.library.page(),
        Some(&Page::Artist(ARTIST.to_owned())),
        "and the page is the artist it named"
    );
}

#[test]
fn a_link_click_is_dropped_when_the_frame_already_has_a_command() {
    let mut transport = playing(cover("abc"));
    let mut queue = linked_queue(4, &mut transport);

    // The control: the same click in a frame whose only command is its own
    // opens the queue's album, so the frame below has an open to drop.
    let mut control = Window::new(Vec2::new(1400.0, 900.0));
    control.library.open(Page::Album(ALBUM.to_owned()));
    control.library.open(Page::Album(ALBUM_B.to_owned()));
    let (_, frame) = control.frame(&mut transport, &mut queue, Vec::new());
    let at = on_the_words(&frame, pane_rect_when_shown(&control), "t2");

    assert_eq!(
        click_words(&mut control, &mut transport, &mut queue, at),
        Some(Command::LoadAlbum {
            album: QUEUE_ALBUM.to_owned()
        }),
        "the same click with no nav opens the queue's album"
    );
    assert_eq!(
        control.library.page(),
        Some(&Page::Album(QUEUE_ALBUM.to_owned())),
        "and the control's page is the album it named"
    );

    // Two albums behind each other, so Back has one to land on and must ask
    // for it: the nav's command is the frame's.
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    window.library.open(Page::Album(ALBUM.to_owned()));
    window.library.open(Page::Album(ALBUM_B.to_owned()));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = on_the_words(&frame, pane_rect_when_shown(&window), "t2");

    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![egui::Event::PointerMoved(at)],
    );
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![crate::view::test_support::button(at, true)],
    );
    let command = window
        .frame(
            &mut transport,
            &mut queue,
            vec![
                key(egui::Key::BrowserBack, true),
                crate::view::test_support::button(at, false),
            ],
        )
        .0;

    assert_eq!(
        command,
        Some(Command::LoadAlbum {
            album: ALBUM.to_owned()
        }),
        "the nav's command is the frame's one"
    );
    assert_eq!(
        window.library.page(),
        Some(&Page::Album(ALBUM.to_owned())),
        "the nav landed on the album behind and the link's page was dropped"
    );
}

#[test]
fn a_link_click_is_dropped_while_a_block_is_carried() {
    let mut transport = playing(cover("abc"));
    let mut queue = linked_queue(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let pane = pane_rect_when_shown(&window);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, pane, "t2");
    let title = on_the_words(&frame, pane, "t0");
    let _ = window.hold_drag(&mut transport, &mut queue, from, below(from, 0.75));
    assert!(carry::carrying(&window.ctx), "a block is in flight");
    // The gesture that holds the block is a drag, and a drag cannot also
    // click, so the frame the click arrives in is told a block is away.
    let _ = window.frame(&mut transport, &mut queue, vec![window.release(from)]);
    egui::DragAndDrop::set_payload(&window.ctx, ());

    assert_eq!(
        click_words(&mut window, &mut transport, &mut queue, title),
        None,
        "a carried block opened a page"
    );
    assert_ne!(
        window.library.page(),
        Some(&Page::Album(QUEUE_ALBUM.to_owned())),
        "the carried frame moved the library"
    );
}

#[test]
fn a_queue_open_and_a_now_playing_open_in_one_frame_apply_once() {
    // One pointer clicks one link a frame, so the frame that names two pages
    // is written by hand: the panel's page is the one taken, and the queue's
    // is spent with it, so the library can only move once.
    let mut panel = now_playing::Reply {
        open: Some(Page::Album(ALBUM.to_owned())),
        ..now_playing::Reply::default()
    };
    let mut queue = queue_pane::Reply {
        open: Some(Page::Album(QUEUE_ALBUM.to_owned())),
        ..queue_pane::Reply::default()
    };

    assert_eq!(
        take_open(&mut panel.open, &mut queue.open),
        Some(Page::Album(ALBUM.to_owned())),
        "the panel's page is the frame's one page"
    );
    assert_eq!(
        (panel.open, queue.open),
        (None, None),
        "both opens are spent with the frame's one"
    );
}

#[test]
fn a_click_on_a_queue_row_selects_it_and_plays_nothing() {
    // A single click is a press. Scanning the queue with the pointer must
    // never move the transport.
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = row_line(&frame, pane_rect_when_shown(&window), "t2");

    assert_eq!(window.click(&mut transport, &mut queue, at), None);
    assert!(queue.selected(2), "the click picks the row");
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.uri.as_str()),
        Some("t0"),
        "a single click must not move the transport"
    );
}

#[test]
fn a_click_on_a_queue_row_keeps_the_keyboard_without_a_ring() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = row_line(&frame, pane_rect_when_shown(&window), "t2");
    assert!(
        ringed_row(&frame).is_none(),
        "nothing holds the keyboard yet"
    );

    let _ = window.click(&mut transport, &mut queue, at);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let id = window
        .ctx
        .memory(|memory| memory.focused())
        .expect("the clicked row holds the keyboard");
    let row = window
        .ctx
        .read_response(id)
        .expect("the row was laid out")
        .rect;
    assert!(
        row.contains(at),
        "the keyboard went to another row: {row:?}"
    );
    assert_eq!(row.height(), QUEUE_ROW_H, "the keyboard is on a row");
    assert!(ringed_row(&frame).is_none(), "a clicked row wears no ring");
}

#[test]
fn a_clicked_queue_row_wears_no_ring() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = row_line(&frame, pane_rect_when_shown(&window), "t2");

    let _ = window.click(&mut transport, &mut queue, at);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        window.ctx.memory(|memory| memory.focused()).is_some(),
        "the click took the keyboard"
    );
    assert!(
        ringed_row(&frame).is_none(),
        "a clicked queue row wears no ring"
    );
}

#[test]
fn tab_rings_the_row_inside_the_list() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(5, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let pane = pane_rect_when_shown(&window);
    clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");

    // Tab walks out of the row's own controls and onto the rows below it.
    let mut row = None;
    for _ in 0..8 {
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Tab);
        let focused = window
            .ctx
            .memory(|memory| memory.focused())
            .expect("Tab keeps the keyboard");
        let rect = window
            .ctx
            .read_response(focused)
            .expect("the row was laid out")
            .rect;
        if (rect.height() - QUEUE_ROW_H).abs() < 0.6 {
            row = Some(rect);
            break;
        }
    }
    let row = row.expect("Tab reached a queue row");

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let rings = crate::view::test_support::rings(&frame, &Tokens::DARK);
    assert_eq!(rings.len(), 1, "the tabbed row wears the ring: {rings:?}");
    assert!(
        row.contains_rect(rings[0]),
        "the ring left the row: {:?} in {row:?}",
        rings[0]
    );
    assert!(pane.contains_rect(row), "the row left the list: {row:?}");
}

#[test]
fn a_click_on_empty_space_then_tab_rings_the_next_widget() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let _ = window.frame(&mut transport, &mut queue, Vec::new());

    let empty = view_rect(&window).center();
    let _ = window.click(&mut transport, &mut queue, empty);
    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Tab);

    let focused = window
        .ctx
        .memory(|memory| memory.focused())
        .expect("Tab took the keyboard");
    let rect = window
        .ctx
        .read_response(focused)
        .expect("the widget was laid out")
        .rect;
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let rings = crate::view::test_support::rings(&frame, &Tokens::DARK);
    assert_eq!(rings.len(), 1, "one widget wears the ring: {rings:?}");
    let control_ring = rect.expand(extent::FOCUS_OFFSET + stroke::HAIRLINE);
    assert!(
        rings[0] == rect || rings[0] == control_ring,
        "the ring is not the widget Tab reached: {rings:?} for {rect:?}"
    );
}

#[test]
fn a_click_then_space_wears_no_ring_on_the_clicked_widget() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = row_line(&frame, pane_rect_when_shown(&window), "t2");
    let _ = window.click(&mut transport, &mut queue, at);

    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Space);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        ringed_row(&frame).is_none(),
        "a key after a click reveals no ring on the clicked widget"
    );
    let focused = window
        .ctx
        .memory(|memory| memory.focused())
        .expect("the click took the keyboard");
    let row = window
        .ctx
        .read_response(focused)
        .expect("the row was laid out")
        .rect;
    assert!(row.contains(at), "the keyboard left the clicked row");
}

#[test]
fn enter_on_a_focused_queue_row_plays_it() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = row_line(&frame, pane_rect_when_shown(&window), "t2");
    let _ = window.click(&mut transport, &mut queue, at);

    assert_eq!(
        chord(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::Enter,
            egui::Modifiers::NONE
        ),
        Some(Command::Play {
            uri: "t2".to_owned()
        })
    );
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.uri.as_str()),
        Some("t2")
    );
}

#[test]
fn shift_click_selects_a_span_and_ctrl_click_toggles_in_the_queue() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(7, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let pane = pane_rect_when_shown(&window);
    let at = |title: &str| row_line(&frame, pane, title);

    let _ = window.click(&mut transport, &mut queue, at("t2"));
    window.held = egui::Modifiers::SHIFT;
    let _ = window.click(&mut transport, &mut queue, at("t4"));
    assert!(
        (2..=4).all(|row| queue.selected(row)),
        "shift extends from the anchor"
    );

    window.held = egui::Modifiers::COMMAND;
    let _ = window.click(&mut transport, &mut queue, at("t3"));
    window.held = egui::Modifiers::NONE;
    assert!(queue.selected(2) && queue.selected(4));
    assert!(!queue.selected(3), "ctrl takes one row out of the span");
}

#[test]
fn dragging_a_queue_row_below_another_reorders_without_a_command() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(5, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, pane_rect_when_shown(&window), "t2");
    let to = below(from, 1.75);

    assert_eq!(
        window.drag(&mut transport, &mut queue, from, to),
        None,
        "a reorder is not the transport's to play"
    );
    assert_eq!(entries(&queue), ["t0", "t1", "t3", "t2", "t4"]);
    assert!(queue.selected(3), "a move picks the block it landed");
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.uri.as_str()),
        Some("t0"),
        "the playing entry keeps its place"
    );
}

#[test]
fn dragging_a_selected_block_moves_the_block() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(6, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let pane = pane_rect_when_shown(&window);
    let _ = window.click(&mut transport, &mut queue, row_line(&frame, pane, "t2"));
    window.held = egui::Modifiers::COMMAND;
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let _ = window.click(&mut transport, &mut queue, row_line(&frame, pane, "t3"));
    window.held = egui::Modifiers::NONE;
    assert!(queue.selected(2) && queue.selected(3));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, pane, "t3");

    assert_eq!(
        window.drag(&mut transport, &mut queue, from, below(from, 2.75)),
        None
    );
    assert_eq!(
        entries(&queue),
        ["t0", "t1", "t4", "t5", "t2", "t3"],
        "the whole block moves, in its own order"
    );
}

#[test]
fn dropping_above_the_first_upcoming_row_cannot_displace_the_playing_entry() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let pane = pane_rect_when_shown(&window);
    let from = row_line(&frame, pane, "t3");
    // The caption band above the first row is part of the target, and means
    // the first boundary the queue will take a drop at.
    let to = text_in(&frame, "NEXT IN QUEUE", pane)
        .expect("the caption is painted")
        .center();

    assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
    assert_eq!(
        entries(&queue),
        ["t0", "t3", "t1", "t2"],
        "a drop at the caption is the first upcoming boundary"
    );
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.uri.as_str()),
        Some("t0"),
        "the playing entry cannot be displaced"
    );
}

#[test]
fn a_release_outside_the_queue_drops_nothing() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, pane_rect_when_shown(&window), "t2");
    let before = entries(&queue);

    assert_eq!(
        window.drag(
            &mut transport,
            &mut queue,
            from,
            view_rect(&window).center()
        ),
        None
    );
    assert_eq!(entries(&queue), before, "a release nowhere changes nothing");
}

#[test]
fn an_empty_queue_takes_the_first_row_and_stays_armed() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let from = row_line(&frame, view_rect(&window), "p1");
    // The caption band is part of the pane's target and sits inside the pane
    // whatever the art box above reserves; the panel's own centre may not.
    let to = text_in(&frame, "NEXT IN QUEUE", pane_rect_when_shown(&window))
        .expect("the caption is painted")
        .center();

    assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
    assert_eq!(entries(&queue), ["list:p1"]);
    assert!(
        !queue.can_previous(),
        "a drop queues the row, it does not start it"
    );
    assert_eq!(queue.first_upcoming(), 0, "the row waits for a play");

    let (command, _) = tap(
        &mut window,
        &mut transport,
        &mut queue,
        key(egui::Key::Space, true),
    );
    assert_eq!(
        command,
        Some(Command::Play {
            uri: "list:p1".to_owned()
        }),
        "a space starts the waiting row"
    );
    assert_eq!(queue.first_upcoming(), 1, "and it is the current entry now");
}

#[test]
fn a_playlist_row_dropped_on_the_queue_is_still_queued() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let pane = pane_rect_when_shown(&window);
    let from = row_line(&frame, view_rect(&window), "p1");
    let to = below(row_line(&frame, pane, "t3"), -0.25);

    assert_eq!(
        window.drag(&mut transport, &mut queue, from, to),
        None,
        "dropping a track in the queue must not start it"
    );
    assert_eq!(entries(&queue), ["t0", "t1", "t2", "list:p1", "t3"]);
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.uri.as_str()),
        Some("t0")
    );
    assert_eq!(
        page_rows(&window),
        ["list:p0", "list:p1", "list:p2"],
        "a drop in the queue is no write to the page it came from"
    );
}

#[test]
fn dragging_a_playlist_row_below_the_next_moves_it_and_sends_one_write() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    playlist_page(&mut window, &["p0", "p1", "p2"]);
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let view = view_rect(&window);
    let from = row_line(&frame, view, "p0");
    let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 2);

    assert_eq!(
        window.drag(&mut transport, &mut queue, from, to),
        Some(Command::MoveTrack {
            playlist: PLAYLIST.to_owned(),
            revision: revision("rev-1"),
            moves: vec![Move {
                from: 0,
                length: 1,
                to: 2,
            }],
        }),
        "the page asks for one write on the revision it was read at"
    );
    assert_eq!(
        page_rows(&window),
        ["list:p1", "list:p0", "list:p2"],
        "the row has already moved on screen"
    );
    assert!(!window.library.movable(), "the write is in flight");
    assert_eq!(
        window.frame(&mut transport, &mut queue, Vec::new()).0,
        None,
        "the gesture sends one write, not one a frame"
    );
}

#[test]
fn a_block_dropped_in_the_queue_lands_as_a_block_in_row_order() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let carry = Carry {
        entries: vec![track(8, None), track(9, None)],
        ..carried_rows(egui::Id::unique("list"), vec![0, 2])
    };

    land(
        &mut queue,
        &carry,
        Target::At(Landing {
            boundary: 2,
            marker: Rect::ZERO,
        }),
    );

    assert_eq!(entries(&queue), ["t0", "t1", "t8", "t9", "t2", "t3"]);
}

#[test]
fn a_block_appended_by_the_button_joins_the_end_whole() {
    let mut transport = playing(cover("abc"));
    let mut queue = queue_of(4, &mut transport);
    let carry = Carry {
        entries: vec![track(8, None), track(9, None)],
        ..carried_rows(egui::Id::unique("list"), vec![0, 2])
    };

    land(&mut queue, &carry, Target::End);

    assert_eq!(entries(&queue), ["t0", "t1", "t2", "t3", "t8", "t9"]);
}

/// A queue whose every row names one album and one credit, each row's words
/// its own so a test can aim at one.
fn linked_queue(count: usize, transport: &mut Transport) -> Queue {
    let tracks: Vec<TrackEntry> = (0..count)
        .map(|index| TrackEntry {
            artists: format!("Band {index}"),
            album_uri: Some(QUEUE_ALBUM.to_owned()),
            artist_uris: vec![QUEUE_ARTIST.to_owned()],
            ..track(index, None)
        })
        .collect();
    let mut queue = Queue::default();
    queue.start(&tracks, 0, None, transport);
    queue
}

/// The point a click on the words of `label`, painted in `area`, lands on.
fn on_the_words(output: &egui::FullOutput, area: Rect, label: &str) -> egui::Pos2 {
    row_line(output, area, label) + Vec2::new(0.0, 4.0)
}

/// Clicks `at` over the passes egui's hit test needs, returning the command
/// the shell reported.
fn click_words(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    at: egui::Pos2,
) -> Option<Command> {
    let _ = window.frame(transport, queue, Vec::new());
    let _ = window.frame(transport, queue, vec![egui::Event::PointerMoved(at)]);
    let _ = window.frame(
        transport,
        queue,
        vec![crate::view::test_support::button(at, true)],
    );
    window
        .frame(
            transport,
            queue,
            vec![crate::view::test_support::button(at, false)],
        )
        .0
}
