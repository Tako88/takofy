use super::super::*;
use super::test_support::*;
use crate::library_filter::Item;
use crate::message::Update;
use egui::{Context, Event, Pos2, Rect};
use takofy_model::{AlbumDetail, TrackEntry};

/// The rect `tracks_view` draws the `index`-th row's `cell` into, solved the
/// way the painter solves it: the row's own rect, then the columns at its
/// origin.
///
/// The cell is the whole column, padding included, but a credit's link is the
/// words painted at its left edge: a probe past those words is the row's.
fn cell_rect_for(
    ctx: &Context,
    count: usize,
    index: usize,
    cell: fn(table_layout::Columns) -> Rect,
) -> Rect {
    let row = row_rect_for(ctx, count, index);
    cell(table_layout::columns(row.width(), Table::Full.columns()).at(row.min))
}

/// The rect the painted words of `label` occupy in one frame of the
/// tracklist, read back from the galleys the view painted.
fn words_of(ctx: &Context, library: &mut Library, label: &str) -> Rect {
    let output = tracklist_frame(ctx, Table::Full, library);
    text_rect(&output, label).unwrap_or_else(|| panic!("{label:?} was not painted"))
}

#[test]
fn a_click_on_a_card_loads_its_playlist() {
    let ctx = context();
    let mut library = library_idle(vec![playlist("a", "A"), playlist("b", "B")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    // The click is where the view draws the first card, not a probe's
    // geometry.
    let centre = card_rect_for(&ctx, 0).center();
    let command = click(&ctx, centre, &mut library, &mut transport, &mut queue);

    assert_eq!(
        command,
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        })
    );
    assert_eq!(library.selected(), Some("a"));
    assert_eq!(library.tracks, Load::Loading);
}

#[test]
fn a_card_click_asks_for_tracks_once() {
    // The whole click sequence twice: the first click selects the playlist
    // and starts its load, and the second lands on the progress the view is
    // showing instead of a card. Counting the commands the view reports is
    // the assertion the single command in `a_click_on_a_card_loads_its_
    // playlist` cannot make.
    let ctx = context();
    let tokens = Tokens::DARK;
    let mut library = library_idle(vec![playlist("a", "A")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let centre = card_rect_for(&ctx, 0).center();

    run(&ctx, input(Vec::new()), |ui| {
        let _ = tracks_view(
            ui,
            Table::Full,
            &mut library,
            &mut transport,
            &mut queue,
            &mut Art::default(),
            Instant::now(),
            NOW_MS,
            9,
            &tokens,
        );
    });

    let mut loads = 0;
    for _ in 0..2 {
        for event in crate::view::test_support::click_events(centre) {
            run(&ctx, input(vec![event]), |ui| {
                if let Some(Command::LoadTracks { .. }) = tracks_view(
                    ui,
                    Table::Full,
                    &mut library,
                    &mut transport,
                    &mut queue,
                    &mut Art::default(),
                    Instant::now(),
                    NOW_MS,
                    9,
                    &tokens,
                ) {
                    loads += 1;
                }
            });
        }
    }

    assert_eq!(loads, 1, "two clicks on the same card must load it once");
    assert_eq!(library.selected(), Some("a"));
}

#[test]
fn the_tile_height_the_grid_solves_contains_the_last_line_the_tile_paints() {
    let ctx = context();
    let tokens = Tokens::DARK;
    // A realistic tile: the grid's own solve at the probe's width, so the
    // rect's height is `tile_w + media::extra_height`, the number the grid is
    // given.
    let rect = card_rect_for(&ctx, 0);
    let media = crate::view::media::Media {
        uri: "a",
        name: "Evening",
        kind: "Playlist",
        sub: "Playlist · Vela Moreno",
        size: "62 songs",
        round: false,
        shelf: SidebarKind::Playlists,
        playable: true,
        cover: Cover::Missing,
        picture: hero::Picture::Cover,
    };
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let _ = crate::view::media::tile(ui, rect, &media, &tokens);
    });
    output.textures_delta.clear();

    let last = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => Some(text.pos.y + text.galley.size().y),
            _ => None,
        })
        .max_by(f32::total_cmp)
        .expect("the card paints a name line and a kind line");

    // The grid's card must contain the painter's lowest line, and it must
    // leave no more than the layout's own `S2` below it. The first half
    // catches an `extra_height` short enough to spill the line out of the
    // card; the second catches the 16 points of slack left when the padding
    // was counted against the art as well as against the card.
    assert!(
        last <= rect.bottom(),
        "the last painted line ends at {last}, past the card's {}",
        rect.bottom()
    );
    assert!(
        rect.bottom() - last <= space::S2 + 0.5,
        "the card leaves {} below its last line, more than the {} it lays out",
        rect.bottom() - last,
        space::S2
    );
}

#[test]
fn to_playlists_returns_to_the_grid() {
    let mut library = library_idle(vec![playlist("a", "A")]);
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t", "t")],
        cover: None,
        access: None,
    });
    library.to_playlists();

    let IdleView::Grid { items, .. } = idle_view(&library) else {
        panic!(
            "to_playlists must return to the grid: {:?}",
            idle_view(&library)
        );
    };
    assert_eq!(items.len(), 1);
    assert!(matches!(&items[0], Item::Playlist(entry) if entry.uri == "a"));
    assert!(
        matches!(library.tracks, Load::Ready(_)),
        "the grid draws while the tracks stay ready"
    );
}

#[test]
fn the_root_draws_the_grid_while_the_tracks_stay_ready() {
    let ctx = context();
    let mut library = library_idle(vec![playlist("a", "A")]);
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t0", "t0")],
        cover: None,
        access: None,
    });
    library.to_playlists();

    assert_eq!(library.page(), None);
    assert!(
        matches!(library.tracks, Load::Ready(_)),
        "the tracks stay ready behind the root"
    );

    let texts = painted_lines(&ctx, Table::Full, &mut library);

    assert!(
        texts.contains(&"A".to_owned()),
        "the grid must draw its card: {texts:?}"
    );
    assert!(
        !texts.contains(&"t0".to_owned()),
        "the ready tracklist must not draw at the root: {texts:?}"
    );
}

#[test]
fn a_click_on_the_album_cell_opens_the_album() {
    let ctx = context();
    let mut library = library_with(vec![linkable("a")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let words = words_of(&ctx, &mut library, "Record");
    let command = click(
        &ctx,
        words.center(),
        &mut library,
        &mut transport,
        &mut queue,
    );

    assert_eq!(
        library.page(),
        Some(&Page::Album("spotify:album:rec".to_owned())),
        "a single click on the album cell's words must open the album"
    );
    assert_eq!(
        command,
        Some(Command::LoadAlbum {
            album: "spotify:album:rec".to_owned()
        }),
        "opening an album asks the worker for it"
    );
    assert!(!queue.can_next(), "opening a page must not start the queue");
    assert_eq!(transport.now_playing, None, "opening a page must not play");
}

#[test]
fn a_click_in_the_album_cells_padding_selects_the_row() {
    // The link is the painted words, not the cell: a click past the end of
    // "Record" reaches the row, which selects it.
    let ctx = context();
    let mut library = library_with(vec![linkable("a")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.album);
    let words = words_of(&ctx, &mut library, "Record");
    let padding = Pos2::new(cell.right() - space::S3, cell.center().y);
    assert!(
        words.right() < padding.x && padding.x < cell.right(),
        "the probe at x={} must be past \"Record\", which ends at x={}",
        padding.x,
        words.right()
    );

    let command = click(&ctx, padding, &mut library, &mut transport, &mut queue);
    assert_eq!(command, None, "a click past the words is the row's own");
    assert_eq!(
        library.page(),
        Some(&Page::Playlist("spotify:playlist:a".to_owned())),
        "a click past the album's words must not navigate"
    );
    assert!(!queue.can_next(), "a click past the words must not play");

    // The cell still belongs to the row, so a double click there plays.
    let command = double_click(&ctx, padding, &mut library, &mut transport, &mut queue);
    assert_eq!(
        command,
        Some(Command::Play {
            uri: "a".to_owned()
        }),
        "a double click past the words must play the row"
    );
}

#[test]
fn a_click_on_the_artist_cell_opens_the_first_artist() {
    let ctx = context();
    let mut library = library_with(vec![linkable("a")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let words = words_of(&ctx, &mut library, "Band");
    let command = click(
        &ctx,
        words.center(),
        &mut library,
        &mut transport,
        &mut queue,
    );

    assert_eq!(
        library.page(),
        Some(&Page::Artist("spotify:artist:band".to_owned())),
        "a single click on the artist cell's words must open the first artist"
    );
    assert_eq!(
        command,
        Some(Command::LoadArtist {
            artist: "spotify:artist:band".to_owned()
        }),
        "opening an artist asks the worker for it"
    );
    assert!(!queue.can_next());
    assert_eq!(transport.now_playing, None);
}

#[test]
fn a_click_in_the_artist_cells_padding_selects_the_row() {
    let ctx = context();
    let mut library = library_with(vec![linkable("a")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.artist);
    let words = words_of(&ctx, &mut library, "Band");
    let padding = Pos2::new(cell.right() - space::S3, cell.center().y);
    assert!(
        words.right() < padding.x && padding.x < cell.right(),
        "the probe at x={} must be past \"Band\", which ends at x={}",
        padding.x,
        words.right()
    );

    let command = click(&ctx, padding, &mut library, &mut transport, &mut queue);
    assert_eq!(command, None, "a click past the words is the row's own");
    assert_eq!(
        library.page(),
        Some(&Page::Playlist("spotify:playlist:a".to_owned())),
        "a click past the artist's words must not navigate"
    );
    assert!(!queue.can_next(), "a click past the words must not play");

    let command = double_click(&ctx, padding, &mut library, &mut transport, &mut queue);
    assert_eq!(
        command,
        Some(Command::Play {
            uri: "a".to_owned()
        }),
        "a double click past the words must play the row"
    );
}

#[test]
fn a_link_cell_points_the_hand_and_brightens_its_label() {
    // A screenshot shows the colour but not the cursor the window reports, so
    // both of the hover's effects are pinned here.
    let ctx = context();
    let tokens = Tokens::DARK;
    let mut library = library_with(vec![linkable("a")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.album);
    let words = words_of(&ctx, &mut library, "Record");

    let mut frame = |events: Vec<Event>| {
        let mut output = run(&ctx, input(events), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                9,
                &tokens,
            );
        });
        output.textures_delta.clear();
        let ink = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == "Record" => {
                    Some(text.fallback_color)
                }
                _ => None,
            })
            .expect("the album label is painted");
        (output.platform_output.cursor_icon, ink)
    };

    // At rest the label is muted and the cursor is the window's own.
    let (rest_cursor, rest_ink) = frame(Vec::new());
    assert_eq!(rest_ink, tokens.palette.text_muted);
    assert_ne!(rest_cursor, egui::CursorIcon::PointingHand);

    // Resting on the words points the hand and brightens them to the body
    // colour, over the reveal's own time.
    for _ in 0..12 {
        frame(vec![Event::PointerMoved(words.center())]);
    }
    let (hover_cursor, hover_ink) = frame(vec![Event::PointerMoved(words.center())]);
    assert_eq!(hover_cursor, egui::CursorIcon::PointingHand);
    assert_eq!(hover_ink, tokens.palette.text);

    // Resting past them is the row's: no hand, and the label goes muted.
    let padding = Pos2::new(cell.right() - space::S3, cell.center().y);
    assert!(
        words.right() < padding.x && padding.x < cell.right(),
        "the probe at x={} must be past \"Record\", which ends at x={}",
        padding.x,
        words.right()
    );
    for _ in 0..12 {
        frame(vec![Event::PointerMoved(padding)]);
    }
    let (padding_cursor, padding_ink) = frame(vec![Event::PointerMoved(padding)]);
    assert_ne!(
        padding_cursor,
        egui::CursorIcon::PointingHand,
        "the empty part of the cell must not point the hand"
    );
    assert_eq!(padding_ink, tokens.palette.text_muted);
}

#[test]
fn the_row_keeps_its_hover_over_a_link_cell() {
    // A link cell's interact is created after the row's, and egui marks only
    // the top hit widget as hovered, so the row's own `hovered()` goes false
    // over the link's words. The row must read where the pointer is instead,
    // or its highlight, its index play glyph and its like all vanish there.
    // The fill is the visible half of that; the glyph and the like follow the
    // same `revealed` value. The label's ink says which widget the hit test
    // gave the pointer to.
    let ctx = context();
    let tokens = Tokens::DARK;
    let mut library = library_with(vec![linkable("a")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let row = row_rect_for(&ctx, 1, 0);
    let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.album);
    let words = words_of(&ctx, &mut library, "Record");

    // The frame's fill behind this row and the album label's ink, read back
    // from the shapes painted.
    let mut frame = |events: Vec<Event>| {
        let output = run(&ctx, input(events), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                9,
                &tokens,
            );
        });
        let fill = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape) if shape.rect == row => Some(shape.fill),
                _ => None,
            });
        let ink = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == "Record" => {
                    Some(text.fallback_color)
                }
                _ => None,
            });
        (fill, ink)
    };

    // With the pointer away the row paints no fill at all.
    assert_eq!(
        frame(Vec::new()),
        (None, Some(tokens.palette.text_muted)),
        "a resting row paints no fill"
    );

    // Resting on the link's words lights the row to the same fill the title
    // would, while the link itself takes the pointer's hover: both are true
    // at once, which is the tie this pins.
    for _ in 0..12 {
        frame(vec![Event::PointerMoved(words.center())]);
    }
    assert_eq!(
        frame(vec![Event::PointerMoved(words.center())]),
        (Some(tokens.palette.row_hover), Some(tokens.palette.text)),
        "the row must keep its hover fill over the album link's words"
    );

    // Past the words the row owns the point, so the fill stays and the link
    // is not hovered.
    let padding = Pos2::new(cell.right() - space::S3, cell.center().y);
    assert!(
        words.right() < padding.x && padding.x < cell.right(),
        "the probe at x={} must be past \"Record\", which ends at x={}",
        padding.x,
        words.right()
    );
    for _ in 0..12 {
        frame(vec![Event::PointerMoved(padding)]);
    }
    assert_eq!(
        frame(vec![Event::PointerMoved(padding)]),
        (
            Some(tokens.palette.row_hover),
            Some(tokens.palette.text_muted)
        ),
        "the row must keep its hover fill past the album link's words"
    );
}

#[test]
fn a_double_click_in_the_album_cell_opens_it_without_playing() {
    let ctx = context();
    let mut library = library_with(vec![linkable("a")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let words = words_of(&ctx, &mut library, "Record");
    let command = double_click(
        &ctx,
        words.center(),
        &mut library,
        &mut transport,
        &mut queue,
    );

    assert_eq!(
        library.page(),
        Some(&Page::Album("spotify:album:rec".to_owned())),
        "the second click lands on the album page, not on the row"
    );
    assert_eq!(
        command,
        Some(Command::LoadAlbum {
            album: "spotify:album:rec".to_owned()
        }),
        "the album page's load is the command, not a play"
    );
    assert!(!queue.can_next(), "a link cell must never start the queue");
    assert_eq!(transport.now_playing, None);
}

#[test]
fn a_double_click_on_the_title_still_plays() {
    let ctx = context();
    let mut library = library_with(vec![linkable("a"), linkable("b")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let cell = cell_rect_for(&ctx, 2, 0, |columns| columns.title);
    let command = double_click(
        &ctx,
        cell.center(),
        &mut library,
        &mut transport,
        &mut queue,
    );

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "a".to_owned()
        })
    );
    assert_eq!(
        library.page(),
        Some(&Page::Playlist("spotify:playlist:a".to_owned()))
    );
    assert!(
        queue.can_next(),
        "the double-clicked row must start the queue"
    );
}

#[test]
fn a_row_without_an_album_uri_keeps_a_plain_cell() {
    let ctx = context();
    let mut library = library_with(vec![TrackEntry {
        album_uri: None,
        ..linkable("a")
    }]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.album);
    let command = click(
        &ctx,
        cell.center(),
        &mut library,
        &mut transport,
        &mut queue,
    );
    assert_eq!(
        command, None,
        "a cell with no URI must take no click of its own"
    );
    assert_eq!(
        library.page(),
        Some(&Page::Playlist("spotify:playlist:a".to_owned())),
        "a plain cell must not navigate"
    );

    // The cell still belongs to the row, so a double click there plays.
    let command = double_click(
        &ctx,
        cell.center(),
        &mut library,
        &mut transport,
        &mut queue,
    );
    assert_eq!(
        command,
        Some(Command::Play {
            uri: "a".to_owned()
        })
    );
}

#[test]
fn an_album_with_no_displayed_name_is_not_a_link() {
    // The labelling can resolve a gid for an album the row never names. An
    // empty cell painted nothing, so it must offer no hit area either.
    let ctx = context();
    let mut library = library_with(vec![TrackEntry {
        album: Some(String::new()),
        ..linkable("a")
    }]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.album);
    let command = click(
        &ctx,
        cell.center(),
        &mut library,
        &mut transport,
        &mut queue,
    );

    assert_eq!(command, None);
    assert_eq!(
        library.page(),
        Some(&Page::Playlist("spotify:playlist:a".to_owned())),
        "an unnamed album must not navigate"
    );
}

#[test]
fn the_enter_key_plays_with_the_pointer_on_a_link_cell() {
    // The pointer's hover gates only the pointer's double click. A keyboard
    // activation of the focused row is never taken by a link. Enter is the
    // row's key: a space belongs to the transport, so it reaches no row.
    for pressed in [egui::Key::Enter, egui::Key::Space] {
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut library = library_with(vec![linkable("a")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let words = words_of(&ctx, &mut library, "Record");

        // Warm up so focus traversal and the hit test read a previous pass.
        // The action row's widgets come before the list, so the keyboard is
        // walked on until it lands on a row rather than counted out.
        let pass = |library: &mut Library,
                    transport: &mut Transport,
                    queue: &mut Queue,
                    events: Vec<Event>| {
            run(&ctx, input(events), |ui| {
                let _ = tracks_view(
                    ui,
                    Table::Full,
                    library,
                    transport,
                    queue,
                    &mut Art::default(),
                    Instant::now(),
                    NOW_MS,
                    9,
                    &tokens,
                );
            })
        };
        let warm = pass(&mut library, &mut transport, &mut queue, Vec::new());
        // The action row's box at rest: the field sits on its centre line.
        let bar_bottom = field_at(&warm).center().y + crate::theme::actionbar::REST_H / 2.0;
        let mut focused = None;
        for _ in 0..8 {
            pass(
                &mut library,
                &mut transport,
                &mut queue,
                vec![key(egui::Key::Tab)],
            );
            let landed = ctx.memory(|memory| memory.focused());
            if landed.is_some_and(|id| {
                // A row of the list: a row's height, below the action row.
                ctx.read_response(id).is_some_and(|row| {
                    (row.rect.height() - extent::ROW_H).abs() < 0.01
                        && row.rect.top() >= bar_bottom - 0.01
                })
            }) {
                focused = landed;
                break;
            }
        }
        assert!(focused.is_some(), "the keyboard never reached a row");

        // Rest the pointer on the album cell's words, then press the key.
        let mut command = None;
        for events in [
            vec![Event::PointerMoved(words.center())],
            vec![key(pressed)],
        ] {
            let command = &mut command;
            let library = &mut library;
            let transport = &mut transport;
            let queue = &mut queue;
            run(&ctx, input(events), |ui| {
                if let Some(next) = tracks_view(
                    ui,
                    Table::Full,
                    library,
                    transport,
                    queue,
                    &mut Art::default(),
                    Instant::now(),
                    NOW_MS,
                    9,
                    &tokens,
                ) {
                    *command = Some(next);
                }
            });
        }

        let expected = (pressed == egui::Key::Enter).then(|| Command::Play {
            uri: "a".to_owned(),
        });
        assert_eq!(
            command, expected,
            "{pressed:?} on the focused row, with the pointer on a link"
        );
        assert_eq!(
            library.page(),
            Some(&Page::Playlist("spotify:playlist:a".to_owned())),
            "{pressed:?} must not navigate"
        );
    }
}

/// The album's own page data for the view tests: one track, so a page can be
/// told apart from the playlist behind it by its rows alone.
fn album_page(uri: &str, name: &str) -> AlbumDetail {
    AlbumDetail {
        uri: uri.to_owned(),
        name: name.to_owned(),
        artists: "Band".to_owned(),
        artist_uri: None,
        year: Some(1997),
        cover: None,
        tracks: vec![TrackEntry {
            artists: "Band".to_owned(),
            ..track("spotify:track:t1", "Album Song")
        }],
    }
}

#[test]
fn back_from_an_album_page_redraws_the_playlist() {
    let ctx = context();
    let mut library = library_with(vec![linkable("a")]);
    assert_eq!(
        library.open(Page::Album("spotify:album:rec".to_owned())),
        Some(Command::LoadAlbum {
            album: "spotify:album:rec".to_owned()
        })
    );
    library.apply(&Update::Album(album_page("spotify:album:rec", "Record")));
    let texts = painted_lines(&ctx, Table::Full, &mut library);
    assert!(
        texts.contains(&"Record".to_owned()),
        "the album page draws its own band: {texts:?}"
    );
    assert!(
        !texts.contains(&"t0".to_owned()),
        "the playlist behind it is not what is drawn: {texts:?}"
    );

    assert_eq!(
        library.back(),
        None,
        "a ready playlist with a matching wanted load needs no command"
    );

    let texts = painted_lines(&ctx, Table::Full, &mut library);
    assert!(
        texts.contains(&"t0".to_owned()),
        "Back must redraw the ready tracklist: {texts:?}"
    );
    assert!(
        !texts.contains(&"Album Song".to_owned()),
        "the album's rows are gone: {texts:?}"
    );
}

#[test]
fn an_album_page_draws_the_album_view() {
    let ctx = context();
    let mut library = library_idle(vec![playlist("a", "A")]);
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t0", "t0")],
        cover: None,
        access: None,
    });
    assert_eq!(
        library.open(Page::Album("spotify:album:x".to_owned())),
        Some(Command::LoadAlbum {
            album: "spotify:album:x".to_owned()
        })
    );
    library.apply(&Update::Album(album_page(
        "spotify:album:x",
        "Signal Decay",
    )));

    let texts = painted_lines(&ctx, Table::Full, &mut library);

    assert!(
        texts.contains(&"ALBUM".to_owned()),
        "the band's eyebrow: {texts:?}"
    );
    assert!(
        texts.contains(&"Signal Decay".to_owned()),
        "the band names the album: {texts:?}"
    );
    assert!(
        texts.contains(&"Album Song".to_owned()),
        "the album's table lists its own track: {texts:?}"
    );
    assert!(
        !texts.contains(&"t0".to_owned()),
        "the playlist behind the page is not what is drawn: {texts:?}"
    );
}
