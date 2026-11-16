//! The shelf at chance as the page paints it: the seed as the head's title, the
//! four cards its station yielded, and the foot line when a draw has nothing to
//! show.
//!
//! The look is judged by eye; what is pinned here is the contract the presses
//! and the sinks depend on — which page a seed opens, which row a card plays, and
//! that a drawing square still holds its node while a read is out.

use std::time::Instant;

use egui::{Context, Event, Rect};
use takofy_model::TrackEntry;

use super::*;
use crate::cover::Art;
use crate::home::Section;
use crate::home::chance::{Candidate, Card, CardKind, Draw};
use crate::message::Failure;
use crate::sidebar_kind::SidebarKind;
use crate::theme::Tokens;
use crate::view::home::tests::{NOW_MS, context, input, painted, text_rect};
use crate::view::playlist::root::MediaDraw;
use crate::view::test_support::click_events;

/// The square's key: one per page.
const SQUARE: &str = "draw";

/// The seed every draw in this file carries.
fn seed() -> Candidate {
    Candidate {
        kind: SeedKind::Playlist,
        uri: "spotify:playlist:seed".to_owned(),
        name: "Mix One".to_owned(),
        at: None,
    }
}

fn row(uri: &str, title: &str, artist: &str, album_uri: &str, album_name: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: title.to_owned(),
        artists: artist.to_owned(),
        album: Some(album_name.to_owned()),
        album_uri: Some(album_uri.to_owned()),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

/// The four rows a draw in this file reads, each with its own album.
fn rows() -> Vec<TrackEntry> {
    (0..4)
        .map(|at| {
            row(
                &format!("spotify:track:{at}"),
                &format!("T{at}"),
                &format!("A{at}"),
                &format!("spotify:album:{at}"),
                &format!("Record {at}"),
            )
        })
        .collect()
}

/// A draw of `rows` as `kinds` says they were drawn, one per row.
fn drawn<'a>(rows: &'a [TrackEntry], kinds: &[CardKind]) -> Draw<'a> {
    let cards: Vec<Card<'a>> = rows
        .iter()
        .zip(kinds)
        .map(|(row, kind)| Card { kind: *kind, row })
        .collect();
    let tracks = cards
        .iter()
        .filter(|card| card.kind == CardKind::Track)
        .count();
    Draw {
        seed: seed(),
        album_uri: None,
        tracks,
        albums: cards.len() - tracks,
        cards,
        pending: false,
        nothing: false,
        failed: None,
        pressed_ms: None,
    }
}

/// A draw of four rows all taken as `kind`.
fn drawn_as<'a>(rows: &'a [TrackEntry], kind: CardKind) -> Draw<'a> {
    drawn(rows, &[kind; 4])
}

/// One pass of the shelf, with the sinks a press fills and the rects it drew.
struct Frame {
    output: egui::FullOutput,
    out: Pressed,
    open: Option<Page>,
    play: Option<(String, SidebarKind)>,
    seed: Rect,
    square: Rect,
    square_disabled: Option<bool>,
    /// The play a card keyed `card` drew, when the pointer reached it.
    card_play: Rect,
}

/// One pass over the shelf at the page's probe width.
fn pass(
    ctx: &Context,
    shelf: &Section<Draw<'_>>,
    stacked: bool,
    events: Vec<Event>,
    card: &str,
) -> Frame {
    let tokens = Tokens::DARK;
    let library = crate::library::Library::default();
    let mut art = Art::default();
    let mut select = None;
    let mut open = None;
    let mut play = None;
    let mut out = Pressed::default();
    let mut seed = Rect::NOTHING;
    let mut square = Rect::NOTHING;
    let mut square_disabled = None;
    let mut card_play = Rect::NOTHING;
    let mut output = ctx.run_ui(input(events), |ui| {
        let mut draw = MediaDraw {
            art: &mut art,
            now: Instant::now(),
            tokens: &tokens,
            library: &library,
            select: &mut select,
            open: &mut open,
            play: &mut play,
        };
        show(ui, shelf, NOW_MS, stacked, &tokens, &mut draw, &mut out);
        let read = |id| {
            ui.ctx()
                .read_response(id)
                .map_or(Rect::NOTHING, |response| response.rect)
        };
        let planted = read(
            ui.scope_id()
                .with(("row-head-seed", "spotify:playlist:seed")),
        );
        let song = read(ui.scope_id().with(("row-head-seed", "spotify:track:seed")));
        seed = if planted.width() > 0.0 { planted } else { song };
        let id = crate::view::controls::slot_id(ui, Control::DrawAnother, SQUARE);
        square = read(id);
        if ui.ctx().has_accesskit_node(id) {
            square_disabled = ui
                .ctx()
                .accesskit_node_builder(id, |node| node.is_disabled());
        }
        card_play = read(crate::view::controls::slot_id(ui, Control::CardPlay, card));
    });
    output.textures_delta.clear();
    Frame {
        output,
        out,
        open,
        play,
        seed,
        square,
        square_disabled,
        card_play,
    }
}

/// The texts one frame painted.
fn texts(output: &egui::FullOutput) -> Vec<String> {
    painted(output)
        .into_iter()
        .map(|(text, _)| text.to_owned())
        .collect()
}

/// What one click filled, in the passes egui needs to report it.
#[derive(Default)]
struct Pressed5 {
    another: bool,
    open: Option<Page>,
    play: Option<(String, SidebarKind)>,
    rows: Option<Vec<TrackEntry>>,
}

/// One click at `at`.
fn press(
    ctx: &Context,
    shelf: &Section<Draw<'_>>,
    stacked: bool,
    at: egui::Pos2,
    card: &str,
) -> Pressed5 {
    let mut pressed = Pressed5::default();
    for event in click_events(at) {
        let frame = pass(ctx, shelf, stacked, vec![event], card);
        pressed.another |= frame.out.another;
        pressed.open = frame.open.or(pressed.open);
        pressed.play = frame.play.or(pressed.play);
        pressed.rows = frame.out.rows.or(pressed.rows);
    }
    pressed
}

#[test]
fn the_seed_is_the_heads_title_and_its_link_opens_its_page() {
    let ctx = context();
    let shelf = Section::Ready(drawn(&[], &[]));

    let first = pass(&ctx, &shelf, false, Vec::new(), "spotify:track:none");
    assert!(
        texts(&first.output).iter().any(|text| text == "Mix One"),
        "the shelf's seed is its title"
    );
    assert!(
        first.seed.width() > 0.0,
        "the seed is drawn as a link: {:?}",
        first.seed
    );

    let pressed = press(
        &ctx,
        &shelf,
        false,
        first.seed.center(),
        "spotify:track:none",
    );
    assert_eq!(
        pressed.open,
        Some(Page::Playlist("spotify:playlist:seed".to_owned())),
        "the seed opens the page it names"
    );
}

#[test]
fn four_cards_paint_their_kind_and_artist() {
    let ctx = context();
    let rows = rows();
    let shelf = Section::Ready(drawn(
        &rows,
        &[
            CardKind::Track,
            CardKind::Album,
            CardKind::Track,
            CardKind::Album,
        ],
    ));

    let frame = pass(&ctx, &shelf, false, Vec::new(), "spotify:track:0");
    let drawn = texts(&frame.output);

    for name in ["T0", "Record 1", "T2", "Record 3"] {
        assert!(
            drawn.iter().any(|text| text == name),
            "the card {name:?} painted no name: {drawn:?}"
        );
    }
    for artist in ["A0", "A1", "A2", "A3"] {
        assert!(
            drawn.iter().any(|text| text == artist),
            "the card {artist:?} painted no artist: {drawn:?}"
        );
    }
    assert_eq!(
        drawn.iter().filter(|text| *text == "TRACK").count(),
        2,
        "two cards say they are tracks"
    );
    assert_eq!(
        drawn.iter().filter(|text| *text == "ALBUM").count(),
        2,
        "two cards say they are albums"
    );
}

#[test]
fn the_ledger_counts_what_is_there_and_never_zero() {
    let ctx = context();
    let rows = rows();
    let mixed = Section::Ready(drawn(
        &rows,
        &[
            CardKind::Track,
            CardKind::Album,
            CardKind::Track,
            CardKind::Album,
        ],
    ));
    let drawn = texts(&pass(&ctx, &mixed, false, Vec::new(), "spotify:track:0").output);
    assert!(
        drawn.contains(&crate::format::draw_ledger(2, 2).to_uppercase()),
        "the head counts both kinds: {drawn:?}"
    );

    let tracks = Section::Ready(drawn_as(&rows, CardKind::Track));
    let drawn = texts(&pass(&ctx, &tracks, false, Vec::new(), "spotify:track:0").output);
    assert!(drawn.contains(&"4 TRACKS".to_owned()));
    assert!(
        !drawn.iter().any(|text| text == "0 ALBUMS"),
        "a draw is never said to hold none of a kind: {drawn:?}"
    );
}

#[test]
fn a_draw_that_found_nothing_says_so_and_keeps_no_ledger() {
    let ctx = context();
    let mut draw = drawn(&[], &[]);
    draw.nothing = true;
    let shelf = Section::Ready(draw);

    let drawn = texts(&pass(&ctx, &shelf, false, Vec::new(), "spotify:track:none").output);
    assert!(
        drawn.iter().any(|text| text == DRAW_NOTHING),
        "the foot line says the draw found nothing: {drawn:?}"
    );
    assert!(
        !drawn
            .iter()
            .any(|text| text.contains("TRACK") || text.contains("ALBUM")),
        "a draw with no cards counts nothing: {drawn:?}"
    );
}

#[test]
fn a_short_draw_paints_what_it_has_and_says_no_sentence() {
    let ctx = context();
    let rows = rows();
    let shelf = Section::Ready(drawn(&rows[..2], &[CardKind::Track, CardKind::Track]));

    let frame = pass(&ctx, &shelf, false, Vec::new(), "spotify:track:0");
    let drawn = texts(&frame.output);

    assert!(drawn.contains(&"T0".to_owned()));
    assert!(drawn.contains(&"T1".to_owned()));
    assert!(
        drawn.contains(&"2 TRACKS".to_owned()),
        "the ledger counts the two it drew: {drawn:?}"
    );
    assert!(
        !drawn
            .iter()
            .any(|text| text == DRAW_NOTHING || text == DRAW_FAILED),
        "a partial row of cards is not a fault: {drawn:?}"
    );
}

#[test]
fn a_failed_draw_says_so_and_drops_the_ledger_but_keeps_its_cards() {
    let ctx = context();
    let rows = rows();
    let mut shelf = drawn_as(&rows, CardKind::Track);
    shelf.failed = Some(Failure::Load);
    let frame = pass(
        &ctx,
        &Section::Ready(shelf),
        false,
        Vec::new(),
        "spotify:track:0",
    );
    let drawn = texts(&frame.output);

    assert!(
        drawn.contains(&DRAW_FAILED.to_owned()),
        "a failed draw said nothing: {drawn:?}"
    );
    assert!(
        drawn.contains(&"T0".to_owned()),
        "a failure blanked the cards"
    );
    assert!(
        !drawn.contains(&draw_ledger(4, 0).to_uppercase()),
        "a failed draw counted what it no longer has: {drawn:?}"
    );
}

#[test]
fn the_square_draws_another_and_holds_its_node_while_a_draw_is_out() {
    let ctx = context();
    ctx.enable_accesskit();
    let rows = rows();
    let shelf = Section::Ready(drawn_as(&rows, CardKind::Track));

    let first = pass(&ctx, &shelf, false, Vec::new(), "spotify:track:0");
    assert!(
        first.square.width() > 0.0,
        "the square is drawn at the head's left: {:?}",
        first.square
    );
    let pressed = press(
        &ctx,
        &shelf,
        false,
        first.square.center(),
        "spotify:track:0",
    );
    assert!(pressed.another, "the square asks for another draw");

    let mut pending = drawn_as(&rows, CardKind::Track);
    pending.pending = true;
    let shelf = Section::Ready(pending);
    let waiting = pass(&ctx, &shelf, false, Vec::new(), "spotify:track:0");
    assert!(waiting.square.width() > 0.0, "a busy square keeps its node");
    assert_eq!(
        waiting.square_disabled,
        Some(true),
        "and is announced disabled"
    );
    let pressed = press(
        &ctx,
        &shelf,
        false,
        waiting.square.center(),
        "spotify:track:0",
    );
    assert!(!pressed.another, "a busy square does nothing");
}

#[test]
fn a_cards_open_goes_to_its_album_and_its_play_to_the_pages_sink() {
    let ctx = context();
    let rows = rows();
    let shelf = Section::Ready(drawn(
        &rows,
        &[
            CardKind::Track,
            CardKind::Album,
            CardKind::Track,
            CardKind::Album,
        ],
    ));
    let first = pass(&ctx, &shelf, false, Vec::new(), "spotify:track:0");

    // A track card opens its row's album.
    let track = text_rect(&first.output, "T0")
        .expect("the track card paints its title")
        .center();
    let pressed = press(&ctx, &shelf, false, track, "spotify:track:0");
    assert_eq!(
        pressed.open,
        Some(Page::Album("spotify:album:0".to_owned()))
    );

    // An album card opens its own.
    let album = text_rect(&first.output, "Record 1")
        .expect("the album card paints the album's name")
        .center();
    let pressed = press(&ctx, &shelf, false, album, "spotify:album:1");
    assert_eq!(
        pressed.open,
        Some(Page::Album("spotify:album:1".to_owned()))
    );

    // A track card's play is one row, queued through the page's own sink.
    let hover = pass(
        &ctx,
        &shelf,
        false,
        vec![Event::PointerMoved(track)],
        "spotify:track:0",
    );
    assert!(
        hover.card_play.width() > 0.0,
        "the card reveals its play under the pointer"
    );
    let pressed = press(
        &ctx,
        &shelf,
        false,
        hover.card_play.center(),
        "spotify:track:0",
    );
    assert_eq!(
        pressed.rows,
        Some(vec![rows[0].clone()]),
        "a track card plays the one row it is"
    );

    // An album card plays the album.
    let hover = pass(
        &ctx,
        &shelf,
        false,
        vec![Event::PointerMoved(album)],
        "spotify:album:1",
    );
    assert!(hover.card_play.width() > 0.0);
    let pressed = press(
        &ctx,
        &shelf,
        false,
        hover.card_play.center(),
        "spotify:album:1",
    );
    assert_eq!(
        pressed.play,
        Some(("spotify:album:1".to_owned(), SidebarKind::Albums))
    );
}

#[test]
fn a_song_seed_opens_the_album_its_name_read_named() {
    let ctx = context();
    let mut draw = drawn(&[], &[]);
    draw.seed = Candidate {
        kind: SeedKind::Song,
        uri: "spotify:track:seed".to_owned(),
        name: "The Song".to_owned(),
        at: None,
    };
    draw.album_uri = Some("spotify:album:named".to_owned());
    let shelf = Section::Ready(draw);
    let first = pass(&ctx, &shelf, false, Vec::new(), "spotify:track:none");
    assert!(first.seed.width() > 0.0);

    let pressed = press(
        &ctx,
        &shelf,
        false,
        first.seed.center(),
        "spotify:track:none",
    );
    assert_eq!(
        pressed.open,
        Some(Page::Album("spotify:album:named".to_owned()))
    );
}

/// The separator discs painted at or below `top`, told from an icon's rounded
/// line ends by their hairline radius.
fn dots(output: &egui::FullOutput, top: f32) -> usize {
    output
        .shapes
        .iter()
        .filter(|clipped| match &clipped.shape {
            egui::epaint::Shape::Circle(circle) => {
                circle.center.y >= top
                    && (circle.radius - crate::theme::stroke::HAIRLINE).abs() < 0.01
            }
            _ => false,
        })
        .count()
}

#[test]
fn a_stacked_kind_takes_its_own_line_and_paints_no_dot() {
    let ctx = context();
    let rows = rows();
    let shelf = Section::Ready(drawn_as(&rows, CardKind::Track));

    let stacked = pass(&ctx, &shelf, true, Vec::new(), "spotify:track:0");
    let kind = text_rect(&stacked.output, "TRACK").expect("the kind is painted");
    let artist = text_rect(&stacked.output, "A0").expect("the artist is painted");
    assert!(
        artist.top() >= kind.bottom() - 0.5,
        "the stacked kind takes its own line: {kind:?} and {artist:?}"
    );
    assert_eq!(
        dots(&stacked.output, kind.top()),
        0,
        "the stacked form drops the separator"
    );

    let inline = pass(&ctx, &shelf, false, Vec::new(), "spotify:track:0");
    let kind = text_rect(&inline.output, "TRACK").expect("the kind is painted");
    let artist = text_rect(&inline.output, "A0").expect("the artist is painted");
    assert!(
        (kind.top() - artist.top()).abs() < 0.5,
        "the plain kind shares the artist's line: {kind:?} and {artist:?}"
    );
    assert!(
        dots(&inline.output, kind.top()) > 0,
        "the plain form paints its separator"
    );
}
