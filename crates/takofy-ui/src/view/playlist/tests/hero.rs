use super::super::*;
use super::test_support::*;
use crate::library_filter::{self, Item, RootKind};
use crate::message::{Failure, Update};
use crate::view::rows::Look;
use crate::view::track_row::State;
use egui::{Context, Pos2, Rect, Vec2};
use takofy_model::TrackEntry;

/// The rect `tracks_view` draws the `index`-th row into while the band is
/// drawn, solved the same way the view solves it: the band and its action row
/// first, then the header, then the whole list reserved in one block, then
/// `layout::table::row_rect` for the row.
///
/// The band pushes the rows down. Every other geometry test builds no named
/// playlist, so the band never appears in them, and this is the only solve
/// that sees its offset.
fn row_rect_below_hero(
    ctx: &Context,
    screen: Vec2,
    count: usize,
    index: usize,
    title: &str,
) -> Rect {
    let tokens = Tokens::DARK;
    let table = Table::Full;
    let mut found = None;
    run(ctx, sized_input(screen, Vec::new()), |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let band = hero::Band {
                eyebrow: "Playlist",
                title,
                meta: Vec::new(),
                credit: None,
                description: "",
                cover: Cover::Missing,
                tint: None,
            };
            let _ = hero::show_with(ui, &band, &tokens, hero::Title::Plain);
            actionbar::row_rect(ui);
            crate::view::inner(ui, |ui| {
                let width = ui.available_width();
                let columns = table_layout::columns(width, table.columns());
                head(ui, columns, &tokens);
                let list = list_rect(ui, count);
                found = Some(table_layout::row_rect(list, index, extent::ROW_H));
            });
        });
    });
    found.expect("the hero, header and list lay out")
}

/// The x the header paints the upper-cased `label` at, read back from the
/// frame's text shapes. The header paints through the painter, so this is
/// where a heading actually landed rather than a re-solve of its columns.
fn heading_left(
    ctx: &Context,
    label: &str,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
) -> f32 {
    let tokens = Tokens::DARK;
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
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
    });
    output.textures_delta.clear();
    let label = label.to_uppercase();
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => Some(text.pos.x),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the header painted {label:?}"))
}

#[test]
fn the_header_and_the_rows_share_a_column_origin() {
    // The header and the rows are solved from one width, so a heading's left
    // edge and its column's left edge are the same screen x. A padding change
    // on one side alone would slide them apart; this reads the header's own
    // painted position rather than re-solving it, so it can tell.
    let ctx = context();
    let mut library = library_with(vec![track("a", "t0")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let row = row_rect_for(&ctx, 1, 0);
    let columns = table_layout::columns(row.width(), Table::Full.columns()).at(row.min);
    for (label, column) in [
        ("Title", columns.title),
        ("Artist", columns.artist),
        ("Album", columns.album),
        ("Added", columns.added),
    ] {
        let painted = heading_left(&ctx, label, &mut library, &mut transport, &mut queue);
        assert!(
            (painted - column.left()).abs() < 0.01,
            "the {label} heading is painted at {painted}, its column starts at {}",
            column.left()
        );
    }
}

/// The table sits one gutter inside the pane, so its first heading lines up
/// under the band's own padding rather than rubbing the panel's edge.
#[test]
fn the_heading_sits_one_gutter_inside_the_pane() {
    let ctx = context();
    let mut library = library_with(vec![track("a", "t0")]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let painted = heading_left(&ctx, "Title", &mut library, &mut transport, &mut queue);
    let columns = table_layout::columns(PROBE.x - 2.0 * space::GUTTER, Table::Full.columns());
    assert!(
        (painted - (space::GUTTER + columns.title.left())).abs() < 0.01,
        "the title heading is painted at {painted}, one gutter plus its column is {}",
        space::GUTTER + columns.title.left()
    );
}

#[test]
fn below_1000_the_playlist_heads_and_draws_the_artist_but_not_the_album() {
    // The reference drops the album at the same width it reduces the
    // playbar, from the header and from every row, and keeps the artist.
    let ctx = context();
    let mut library = library_with(vec![TrackEntry {
        uri: "a".to_owned(),
        title: "t0".to_owned(),
        artists: "Band".to_owned(),
        album: Some("Record".to_owned()),
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: Some(222_000),
        named: true,
        cover: None,
    }]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let _ = tracks_view(
            ui,
            Table::Reduced,
            &mut library,
            &mut transport,
            &mut queue,
            &mut Art::default(),
            Instant::now(),
            NOW_MS,
            9,
            &Tokens::DARK,
        );
    });
    output.textures_delta.clear();
    let texts = drawn_texts(&output);
    assert!(
        texts.contains(&"ARTIST"),
        "the header must keep the artist heading: {texts:?}"
    );
    assert!(
        texts.contains(&"Band"),
        "the row must keep the artist: {texts:?}"
    );
    assert!(
        !texts.contains(&"ALBUM"),
        "the header must drop the album heading: {texts:?}"
    );
    assert!(
        !texts.contains(&"Record"),
        "the row must drop the album: {texts:?}"
    );
}

#[test]
fn a_long_artist_elides_inside_its_column() {
    let tokens = Tokens::DARK;
    let ctx = context();
    let width = 700.0;
    let columns = table_layout::columns(width, Table::Reduced.columns());
    let rect = Rect::from_min_size(Pos2::ZERO, vec2(width, extent::ROW_H));
    let artist = "The Extremely Long-Named Orchestra of the Northern Coast and Its Friends";
    let row = Row {
        uri: "a",
        number: 1,
        title: "t0",
        artist,
        artist_uri: None,
        album: Some("Record"),
        album_uri: None,
        added: None,
        duration: Some("3:42"),
        state: State::Resting,
        cover: Some(Cover::Missing),
        look: Look::default(),
        menu: false,
        liked: None,
    };

    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let _ = track_row::show(ui, rect, columns, &row, &tokens);
    });
    output.textures_delta.clear();
    let galley = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == artist => {
                Some(text.galley.clone())
            }
            _ => None,
        })
        .expect("the artist column paints its label");

    assert_eq!(galley.rows.len(), 1, "the artist must not wrap");
    assert!(galley.elided, "the artist must elide");
    assert!(
        galley.rows[0].row.text().ends_with('…'),
        "the elided artist must end in an ellipsis: {:?}",
        galley.rows[0].row.text()
    );
    assert!(
        galley.size().x <= columns.artist.width() + 0.01,
        "the artist is {} wide in a {} column",
        galley.size().x,
        columns.artist.width()
    );
}

#[test]
fn an_empty_track_list_draws_without_panicking() {
    let tokens = Tokens::DARK;
    let ctx = context();
    let mut library = library_with(Vec::new());
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    let mut command = None;
    run(&ctx, input(Vec::new()), |ui| {
        command = tracks_view(
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

    assert_eq!(command, None, "an empty list has nothing to activate");
}

#[test]
fn the_hero_heads_a_named_playlist_and_not_an_unnamed_one() {
    let tokens = Tokens::DARK;
    let ctx = context();
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    // A selected playlist whose name the library knows: the band is drawn.
    let mut named = library_named(vec![track("a", "t0")], "Evening");
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let _ = tracks_view(
            ui,
            Table::Full,
            &mut named,
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
    let texts = drawn_texts(&output);
    assert!(
        texts.contains(&"PLAYLIST"),
        "the band's eyebrow must be drawn over a named playlist: {texts:?}"
    );
    assert!(
        texts.contains(&"Evening"),
        "the band must name the playlist: {texts:?}"
    );

    // The same selection with nothing in the library to name it: no band,
    // only the rows.
    let mut unnamed = library_with(vec![track("a", "t0")]);
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let _ = tracks_view(
            ui,
            Table::Full,
            &mut unnamed,
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
    let texts = drawn_texts(&output);
    assert!(
        !texts.contains(&"PLAYLIST"),
        "no name means no band, only the rows: {texts:?}"
    );
}

#[test]
fn the_hero_play_control_starts_the_first_track_through_the_queue() {
    let ctx = context();
    let mut library = library_named(vec![track("a", "t0"), track("b", "t1")], "Evening");
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    // The click is where the view draws the control, not a probe's geometry.
    let play = play_tile_rect_for(&ctx, "Evening").center();
    let command = click(&ctx, play, &mut library, &mut transport, &mut queue);

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "a".to_owned()
        })
    );
    assert!(
        queue.can_next(),
        "the hero's play must start the queue, not only the transport"
    );
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.uri.as_str()),
        Some("a")
    );
}

#[test]
fn a_double_click_on_the_second_row_plays_it_while_the_band_is_drawn() {
    let ctx = context();
    let mut library = library_named(vec![track("a", "t0"), track("b", "t1")], "Evening");
    let mut transport = Transport::default();
    let mut queue = Queue::default();

    // The band pushes the rows down, so the only solve that sees its offset
    // is the one below it. Clicking the second row must start the second
    // track: if the offset were dropped, the same point would land on the
    // first.
    let second = row_rect_below_hero(&ctx, TALL, 2, 1, "Evening").center();
    let command = clicks(
        TALL,
        &ctx,
        second,
        2,
        &mut library,
        &mut transport,
        &mut queue,
    );

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "b".to_owned()
        })
    );
    assert!(
        !queue.can_next(),
        "the second of two rows was started, not the first"
    );
    assert_eq!(
        transport.now_playing.as_ref().map(|now| now.uri.as_str()),
        Some("b")
    );
}

#[test]
fn idle_view_reads_the_playlists_load() {
    // A list nobody has asked for yet and a list in flight are one state:
    // neither has anything to draw.
    assert_eq!(idle_view(&Library::default()), IdleView::Loading);

    let mut loading = Library::default();
    loading.apply(&Update::SignedIn);
    assert_eq!(idle_view(&loading), IdleView::Loading);
}

#[test]
fn idle_view_failed_playlists_win_for_every_chip() {
    for kind in library_filter::ROW {
        let mut library = library_idle(Vec::new());
        library.root_filter.choose(kind);
        library.apply(&Update::PlaylistFailed(Failure::Load));
        assert_eq!(
            idle_view(&library),
            IdleView::Failed(Failure::Load),
            "{kind:?} must still report the page's failure"
        );
    }
}

#[test]
fn idle_view_all_shows_playlists_and_shows() {
    // The four kinds in one column are pinned by `library_filter`; this is
    // the root reading them together, and settling.
    let mut library = library_idle(vec![playlist("a", "A")]);
    library.apply(&Update::FollowedShows(vec![show("s", "S")]));
    library.apply(&Update::FollowedArtists(Vec::new()));
    library.apply(&Update::SavedAlbums(Vec::new()));
    let IdleView::Grid { items, note } = idle_view(&library) else {
        panic!("a ready root is a grid: {:?}", idle_view(&library));
    };
    assert_eq!(items.len(), 2, "playlists first, then shows");
    assert!(matches!(items[0], Item::Playlist(entry) if entry.uri == "a"));
    assert!(matches!(&items[1], Item::Show(entry) if entry.uri == "s"));
    assert_eq!(note, LoadNote::None);
}

/// All draws four reads, so its note names the first still owed, in the
/// order the cards are drawn.
#[test]
fn idle_view_all_names_the_first_read_it_is_still_owed() {
    let mut library = library_idle(vec![playlist("a", "A")]);
    library.apply(&Update::FollowedShows(Vec::new()));
    let IdleView::Grid { note, .. } = idle_view(&library) else {
        panic!("a ready root is a grid: {:?}", idle_view(&library));
    };
    assert_eq!(
        note,
        LoadNote::Loading(FollowedRead::Artists),
        "the shows are in, so the artists are the first owed"
    );

    library.apply(&Update::FollowedArtists(Vec::new()));
    let IdleView::Grid { note, .. } = idle_view(&library) else {
        panic!("a ready root is a grid: {:?}", idle_view(&library));
    };
    assert_eq!(note, LoadNote::Loading(FollowedRead::Albums));

    library.apply(&Update::SavedAlbums(Vec::new()));
    let IdleView::Grid { note, .. } = idle_view(&library) else {
        panic!("a ready root is a grid: {:?}", idle_view(&library));
    };
    assert_eq!(note, LoadNote::None);
}

#[test]
fn idle_view_all_with_no_playlists_waits_on_the_followed_list() {
    let mut library = library_idle(Vec::new());
    let _ = library.followed_request();
    let IdleView::Grid { items, note } = idle_view(&library) else {
        panic!("the shows are still coming: {:?}", idle_view(&library));
    };
    assert!(items.is_empty());
    assert_eq!(note, LoadNote::Loading(FollowedRead::Shows));

    library.apply(&Update::FollowedShowsFailed(Failure::Followed));
    let IdleView::Grid { note, .. } = idle_view(&library) else {
        panic!("a failed list is its own note: {:?}", idle_view(&library));
    };
    assert_eq!(
        note,
        LoadNote::Failed(FollowedRead::Shows, Failure::Followed)
    );

    let mut settled = library_idle(Vec::new());
    settled.apply(&Update::FollowedShows(Vec::new()));
    settled.apply(&Update::FollowedArtists(Vec::new()));
    settled.apply(&Update::SavedAlbums(Vec::new()));
    assert_eq!(idle_view(&settled), IdleView::Empty(RootKind::All));
}

#[test]
fn idle_view_playlists_hides_shows_and_their_loading() {
    let mut library = library_idle(vec![playlist("a", "A")]);
    library.root_filter.choose(RootKind::Playlists);
    // The followed list is in flight; the playlists chip does not wait on it.
    let _ = library.followed_request();
    let IdleView::Grid { items, note } = idle_view(&library) else {
        panic!("the playlists chip is a grid: {:?}", idle_view(&library));
    };
    assert_eq!(items.len(), 1);
    assert_eq!(note, LoadNote::None);
}

#[test]
fn idle_view_podcasts_loading_failed_and_empty() {
    let mut library = library_idle(vec![playlist("a", "A")]);
    library.root_filter.choose(RootKind::Podcasts);

    let _ = library.followed_request();
    let IdleView::Grid { items, note } = idle_view(&library) else {
        panic!(
            "a loading followed list is a grid: {:?}",
            idle_view(&library)
        );
    };
    assert!(
        items.is_empty(),
        "the playlists chip is not the podcasts one"
    );
    assert_eq!(note, LoadNote::Loading(FollowedRead::Shows));

    library.apply(&Update::FollowedShowsFailed(Failure::Load));
    let IdleView::Grid { items, note } = idle_view(&library) else {
        panic!(
            "a failed followed list is a grid: {:?}",
            idle_view(&library)
        );
    };
    assert!(items.is_empty());
    assert_eq!(note, LoadNote::Failed(FollowedRead::Shows, Failure::Load));

    let mut empty = library_idle(vec![playlist("a", "A")]);
    empty.root_filter.choose(RootKind::Podcasts);
    empty.apply(&Update::FollowedShows(Vec::new()));
    assert_eq!(
        idle_view(&empty),
        IdleView::Empty(RootKind::Podcasts),
        "nothing to show is the chip's own empty line"
    );
}
