use takofy_model::PlaylistEntry;

use super::super::*;
use super::test_support::*;
use crate::message::Update;
use crate::theme::space;

const PLAYLIST: &str = "spotify:playlist:a";
/// The band's top as the shell's entry point draws it: `tracks_view` adds the
/// page's one leading gap and dispatches, so this is the offset a viewer sees
/// rather than one of the page's internal solves.
fn pane_band_top(library: &mut Library) -> f32 {
    let ctx = context();
    let tokens = Tokens::DARK;
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();
    let output = run(&ctx, input(Vec::new()), |ui| {
        let _ = crate::view::playlist::tracks_view(
            ui,
            Table::Full,
            library,
            &mut transport,
            &mut queue,
            &mut art,
            Instant::now(),
            0,
            9,
            &tokens,
        );
    });
    band_top(&output, PROBE.x).expect("the pane paints a band")
}

/// A library on a ready playlist page, so the album page can be measured
/// against the page every other page is laid out beside.
fn playlist_library() -> Library {
    let mut library = Library::default();
    library.select(PLAYLIST);
    library.apply(&Update::Playlists(vec![PlaylistEntry {
        saved_at: None,
        composed: Vec::new(),
        uri: PLAYLIST.to_owned(),
        name: "Mix".to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: None,
        access: None,
    }]));
    library.apply(&Update::Tracks {
        playlist: PLAYLIST.to_owned(),
        tracks: vec![track("spotify:track:a", "One", "Band", 222_000)],
        cover: None,
        access: None,
    });
    library
}

/// The album's content sits one gutter inside the pane, so its table lines up
/// under the band's own padding rather than rubbing the panel's edge.
#[test]
fn the_album_table_sits_one_gutter_inside_the_pane() {
    let ctx = context();
    let mut library = library_with(detail(vec![track(
        "spotify:track:a",
        "First",
        "Band",
        222_000,
    )]));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut Art::default(),
    );

    let heading = text_rect(&output, "TITLE").expect("the table head is painted");
    // The head paints the title column's heading, so the gutter shows up as
    // the difference between the painted left and the column solved at the
    // pane's width less the two gutters.
    let columns = table_layout::columns(
        PROBE.x - 2.0 * space::GUTTER,
        Table::Full.columns().for_plays(),
    );
    assert!(
        (heading.left() - (space::GUTTER + columns.title.left())).abs() < 0.01,
        "the table head sits at {}, one gutter plus the title column is {}",
        heading.left(),
        space::GUTTER + columns.title.left()
    );
}

/// The album page starts where every other page starts: flush under the
/// topbar. A page that adds its own air sits a whole step lower, which no
/// painted string can catch.
#[test]
fn the_album_page_starts_where_the_playlist_does() {
    let album = pane_band_top(&mut library_with(detail(vec![track(
        "spotify:track:a",
        "One",
        "Band",
        222_000,
    )])));
    let playlist = pane_band_top(&mut playlist_library());

    assert_eq!(album, 0.0, "the album band starts below the topbar");
    assert_eq!(
        album, playlist,
        "the album band's top {album} is not the playlist's {playlist}"
    );
}
