use egui::{Context, Event, Pos2, RawInput, Rect, Vec2, vec2};
use takofy_model::{ArtistDetail, CoverSet, CoverUrl, Release, ReleaseKind, TrackEntry};

use super::super::*;
use crate::discography::Chip;
use crate::library::Library;
use crate::message::Update;
use crate::theme::{Tokens, extent, fonts, space, style};
use crate::view::test_support::click_events;

pub(super) const ARTIST: &str = "spotify:artist:a";
/// The artist a stale ready detail names while the page is on [`ARTIST`].
pub(super) const OTHER_ARTIST: &str = "spotify:artist:b";

pub(super) fn context() -> Context {
    let tokens = Tokens::DARK;
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&tokens));
    ctx
}

pub(super) fn input(size: Vec2, events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
        events,
        ..Default::default()
    }
}

pub(super) fn run(
    ctx: &Context,
    input: RawInput,
    draw: impl FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    let mut output = ctx.run_ui(input, draw);
    output.textures_delta.clear();
    output
}

pub(super) fn drawn_texts(output: &egui::FullOutput) -> Vec<&str> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect()
}

pub(super) fn text_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
}

pub(super) fn track(uri: &str, title: &str, artists: &str, duration_ms: u32) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: title.to_owned(),
        artists: artists.to_owned(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: Some(duration_ms),
        named: true,
        cover: None,
    }
}

pub(super) fn release(uri: &str, name: &str, tracks: &[&str]) -> Release {
    release_of_kind(uri, name, ReleaseKind::Album, tracks)
}

pub(super) fn release_of_kind(
    uri: &str,
    name: &str,
    kind: ReleaseKind,
    tracks: &[&str],
) -> Release {
    Release {
        uri: uri.to_owned(),
        name: name.to_owned(),
        kind,
        year: Some(2024),
        cover: None,
        tracks: tracks.iter().map(|track| (*track).to_owned()).collect(),
    }
}

/// The rect a chip occupies, recovered from the label it painted: a chip's
/// label sits at the chip's top-left plus its padding.
pub(super) fn chip_rect(output: &egui::FullOutput, chip: Chip) -> Option<Rect> {
    let label = crate::format::filter_label(chip).to_uppercase();
    let text = text_rect(output, &label)?;
    Some(text.expand2(Vec2::new(extent::CHIP_PAD_X, extent::CHIP_PAD_Y)))
}

/// The pane every block of the three-release fixture fits in.
pub(super) fn tall() -> Vec2 {
    Vec2::new(900.0, 1500.0)
}

/// Two albums and one EP: two chips beside `All`, and three blocks.
pub(super) fn mixed() -> Vec<Release> {
    vec![
        release_of_kind(
            "spotify:album:r1",
            "Album One",
            ReleaseKind::Album,
            &["spotify:track:1"],
        ),
        release_of_kind(
            "spotify:ep:r2",
            "An EP",
            ReleaseKind::Ep,
            &["spotify:track:2"],
        ),
        release_of_kind(
            "spotify:album:r3",
            "Album Two",
            ReleaseKind::Album,
            &["spotify:track:3"],
        ),
    ]
}

pub(super) fn drawn(output: &egui::FullOutput, name: &str) -> bool {
    drawn_texts(output).contains(&name)
}

pub(super) fn cover_url(tail: &str) -> CoverUrl {
    CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
}

pub(super) fn cover(tail: &str) -> CoverSet {
    CoverSet::single(cover_url(tail))
}

pub(super) const PAGE: Vec2 = Vec2::new(900.0, 1500.0);

pub(super) fn detail(top: Vec<TrackEntry>, releases: Vec<Release>) -> ArtistDetail {
    ArtistDetail {
        uri: ARTIST.to_owned(),
        name: "Vela Moreno".to_owned(),
        portrait: None,
        top_tracks: top,
        releases,
        related: Vec::new(),
    }
}

pub(super) fn library_with(detail: ArtistDetail) -> Library {
    let mut library = Library::default();
    library.open(Page::Artist(detail.uri.clone()));
    library.apply(&Update::Artist(detail));
    library
}

pub(super) fn frame(
    ctx: &Context,
    size: Vec2,
    events: Vec<Event>,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
) -> (Option<Command>, egui::FullOutput) {
    let tokens = Tokens::DARK;
    let mut command = None;
    let output = run(ctx, input(size, events), |ui| {
        // A notch lands in the frame it arrives, as the window's own draw
        // pass makes it: egui's spread of one notch over several frames
        // would still be moving the page while a probe reads a row's place.
        crate::scroll::instant(ui.ctx());
        ui.add_space(space::S3);
        if let Some(next) = artist_view(
            ui,
            Table::Full,
            library,
            transport,
            queue,
            art,
            Instant::now(),
            &tokens,
        ) {
            command = Some(next);
        }
    });
    (command, output)
}

#[expect(
    clippy::too_many_arguments,
    reason = "one test click needs the frame's whole state"
)]
pub(super) fn click(
    ctx: &Context,
    size: Vec2,
    pos: Pos2,
    times: usize,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
) -> Option<Command> {
    let mut last = None;
    for _ in 0..times {
        for event in click_events(pos) {
            last = frame(ctx, size, vec![event], library, transport, queue, art).0;
        }
    }
    last
}

/// The row a pinned page draws, recovered from the fill it paints: the same
/// box the page's children are solved against.
pub(super) fn bar_rect(output: &egui::FullOutput) -> Option<Rect> {
    let tokens = Tokens::DARK;
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape)
                if shape.fill == tokens.palette.surface
                    && (shape.rect.height() - crate::theme::actionbar::STUCK_H).abs() < 0.01 =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
}

/// One frame with the wheel moved by `points` at the foot of the page, which
/// is how the artist page scrolls.
pub(super) fn wheel(
    ctx: &Context,
    size: Vec2,
    points: f32,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
) -> egui::FullOutput {
    let at = Pos2::new(size.x / 2.0, size.y - extent::ROW_H);
    frame(
        ctx,
        size,
        vec![
            Event::PointerMoved(at),
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: Vec2::new(0.0, points),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        library,
        transport,
        queue,
        art,
    )
    .1
}

/// The cells the Popular table's `index`-th row occupies, solved the way the
/// page solves them: the title, the artist and the row itself.
pub(super) fn popular_cells(
    ctx: &Context,
    size: Vec2,
    index: usize,
    count: usize,
) -> (Rect, Rect, Rect) {
    let tokens = Tokens::DARK;
    let mut found = None;
    run(ctx, input(size, Vec::new()), |ui| {
        ui.add_space(space::S3);
        let band = crate::layout::hero::artist_frame(ui.ctx().content_rect().width()).height(0.0);
        let _ = ui.allocate_exact_size(vec2(ui.available_width(), band), Sense::hover());
        actionbar::row_rect(ui);
        crate::view::inner(ui, |ui| {
            section::head(ui, "Popular", "", &tokens);
            let columns = table_layout::columns(
                ui.available_width(),
                Table::Full.columns().for_plays_album(),
            );
            head(ui, columns, &tokens);
            let list = list_rect(ui, count);
            let row = table_layout::row_rect(list, index, extent::ROW_H);
            let placed = columns.at(row.min);
            found = Some((placed.title, placed.album, row));
        });
    });
    found.expect("the popular table lays out")
}

/// Whether one release block's list holds `index` as part of its selection.
pub(super) fn release_picked(ctx: &Context, release: &str, index: usize) -> bool {
    crate::view::carry::List::open(ctx, egui::Id::unique(("release-tracks", release)), 1)
        .look(index)
        .selected
}

/// Eight releases over four kinds, two tracks each: a discography long
/// enough to scroll and mixed enough to offer every chip.
pub(super) fn eight() -> Vec<Release> {
    [
        ("Album One", ReleaseKind::Album),
        ("An EP", ReleaseKind::Ep),
        ("Album Two", ReleaseKind::Album),
        ("A Single", ReleaseKind::Single),
        ("Album Three", ReleaseKind::Album),
        ("Another EP", ReleaseKind::Ep),
        ("Another Single", ReleaseKind::Single),
        ("A Compilation", ReleaseKind::Compilation),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (name, kind))| {
        let uri = format!("spotify:release:r{index}");
        Release {
            uri: uri.clone(),
            name: name.to_owned(),
            kind,
            year: Some(2024),
            cover: None,
            // The track's own URI is the row's title, so a row can be named
            // without depending on the order the store happened to ask.
            tracks: vec![format!("T{index}.1"), format!("T{index}.2")],
        }
    })
    .collect()
}

/// A page on [`ARTIST`] with the discography and a likeness row that is ready
/// to draw: the row the band must not band over.
pub(super) fn with_fans(releases: Vec<Release>) -> Library {
    let mut detail = detail(Vec::new(), releases);
    detail.related = vec!["spotify:artist:other".to_owned()];
    let mut library = library_with(detail);
    let _ = library.related_request();
    library.apply(&Update::Related {
        artist: ARTIST.to_owned(),
        cards: vec![FollowedArtist {
            uri: "spotify:artist:other".to_owned(),
            name: "Other Artist".to_owned(),
            named: true,
            portrait: None,
            releases: None,
        }],
    });
    library
}

/// The chip row's own surface, recovered from the fill it paints. The bar's
/// fill is the only other one on the page, and it is the bar's height.
pub(super) fn band_rect(output: &egui::FullOutput) -> Option<Rect> {
    let tokens = Tokens::DARK;
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape)
                if shape.fill == tokens.palette.surface
                    && (shape.rect.height() - crate::theme::actionbar::STUCK_H).abs() > 0.01 =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
}

/// Wheels the page until the chip row holds under the bar, and answers the
/// frame it held on.
pub(super) fn held_frame(
    ctx: &Context,
    size: Vec2,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
) -> egui::FullOutput {
    let mut output = frame(ctx, size, Vec::new(), library, transport, queue, art).1;
    for _ in 0..400 {
        if let (Some(bar), Some(band)) = (bar_rect(&output), band_rect(&output))
            && (band.top() - bar.bottom()).abs() < 0.5
        {
            return output;
        }
        output = wheel(ctx, size, -space::S4, library, transport, queue, art);
    }
    panic!("the chip row never held under the bar");
}

/// Wheels on until the chip row lets go of the floor and rides the end of the
/// discography, with enough of it below the bar to be seen.
pub(super) fn released_frame(
    ctx: &Context,
    size: Vec2,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
) -> egui::FullOutput {
    let mut output = frame(ctx, size, Vec::new(), library, transport, queue, art).1;
    for _ in 0..800 {
        if let (Some(bar), Some(band)) = (bar_rect(&output), band_rect(&output))
            && band.top() < bar.bottom() - 0.5
            && band.bottom() > bar.bottom() + 2.0
        {
            return output;
        }
        output = wheel(ctx, size, -space::S2, library, transport, queue, art);
    }
    panic!("the chip row never let go of the floor");
}
