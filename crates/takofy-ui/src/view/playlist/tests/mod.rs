use std::time::Duration;

use egui::{
    Context, Event, Modifiers, MouseWheelSource, MouseWheelUnit, Pos2, RawInput, Rect, TouchPhase,
    Vec2,
};

use super::*;
use crate::format::{library_note, root_filter_label};
use crate::library::{Library, MediaLayout};
use crate::library_filter::{self, Item, RootKind};
use crate::message::{Failure, Update};
use crate::theme::{fonts, stroke, style};
use crate::view::rows::Look;
use crate::view::track_row::State;
use crate::view::{filter_row, media, page_head};
use takofy_model::{
    AddedAt, AlbumDetail, CoverImage, CoverSet, CoverUrl, Owner, OwnerName, PlaylistAccess,
    PlaylistCover, PlaylistEntry, Revision, TrackEntry,
};

/// The probe window: 900 by 400, the size every bare-row test runs in.
const PROBE: Vec2 = Vec2::new(900.0, 400.0);
/// The frame's wall clock in the tests: 2023-11-14T22:13:20Z.
const NOW_MS: u64 = 1_700_000_000_000;
/// One day in milliseconds.
const DAY: u64 = 86_400_000;
/// A probe window tall enough to hold the band, the header and two rows in
/// full, so a click under the band is not clipped away by the viewport.
const TALL: Vec2 = Vec2::new(900.0, 800.0);

fn input(events: Vec<Event>) -> RawInput {
    sized_input(PROBE, events)
}

fn sized_input(size: Vec2, events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
        events,
        ..Default::default()
    }
}

/// Runs one pass, consumes the font-atlas delta it may have produced, and
/// hands back the frame for reading.
fn run(ctx: &Context, input: RawInput, draw: impl FnMut(&mut Ui)) -> egui::FullOutput {
    let mut output = ctx.run_ui(input, draw);
    output.textures_delta.clear();
    output
}

/// Every galley the frame painted: its text, the top of its box and its
/// height. The band's stack is only visible in these numbers.
fn drawn_galleys(output: &egui::FullOutput) -> Vec<(String, f32, f32)> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => Some((
                text.galley.text().to_owned(),
                text.pos.y,
                text.galley.size().y,
            )),
            _ => None,
        })
        .collect()
}

/// The top of the galley whose text is exactly `label`.
fn galley_top(drawn: &[(String, f32, f32)], label: &str) -> f32 {
    drawn
        .iter()
        .find(|(text, _, _)| text == label)
        .unwrap_or_else(|| panic!("{label:?} was not painted: {drawn:?}"))
        .1
}

/// The height of the galley whose text is exactly `label`.
fn galley_height(drawn: &[(String, f32, f32)], label: &str) -> f32 {
    drawn
        .iter()
        .find(|(text, _, _)| text == label)
        .unwrap_or_else(|| panic!("{label:?} was not painted: {drawn:?}"))
        .2
}

/// The galley whose text is exactly `label`, from whatever painted shapes
/// hold it.
fn galley_of(output: &egui::FullOutput, label: &str) -> std::sync::Arc<egui::Galley> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some(text.galley.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{label:?} was not painted"))
}

/// Where the galley whose text is exactly `label` was painted.
fn galley_at(output: &egui::FullOutput, label: &str) -> Pos2 {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => Some(text.pos),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{label:?} was not painted"))
}

/// The text the frame actually painted, in paint order.
fn drawn_texts(output: &egui::FullOutput) -> Vec<&str> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect()
}

/// The row indices whose title the frame painted. A row that is laid out
/// but clipped away paints nothing, so this is the row set the window shows.
fn painted_rows(output: &egui::FullOutput) -> Vec<usize> {
    // A row the scroll area clips away is not painted: the table hands the
    // page a row that starts above the fold, and the clip is what keeps it
    // off the screen.
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => {
                let index = text.galley.text().strip_prefix('t')?.parse().ok()?;
                let rect = Rect::from_min_size(text.pos, text.galley.size());
                rect.intersects(clipped.clip_rect).then_some(index)
            }
            _ => None,
        })
        .collect()
}

/// The row a cover URL names, read from the tail the test's `track_with_cover`
/// wrote into the URL itself.
fn cover_row(url: &CoverUrl) -> usize {
    url.as_str()
        .rsplit('c')
        .next()
        .and_then(|tail| tail.parse().ok())
        .expect("a cover URL names its row")
}

fn track(uri: &str, title: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: title.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

/// A track whose album and first artist both name a page, so both cells link.
fn linkable(uri: &str) -> TrackEntry {
    TrackEntry {
        artists: "Band".to_owned(),
        album: Some("Record".to_owned()),
        album_uri: Some("spotify:album:rec".to_owned()),
        artist_uris: vec!["spotify:artist:band".to_owned()],
        artist_names: vec!["Band".to_owned()],
        ..track(uri, "t0")
    }
}

/// One press of `key` with no modifiers.
fn key(key: egui::Key) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

fn date(millis: u64) -> AddedAt {
    AddedAt::from_wire(&millis.to_string()).expect("a date above the floor")
}

fn dated(uri: &str, title: &str, added_at: Option<AddedAt>) -> TrackEntry {
    TrackEntry {
        added_at,
        ..track(uri, title)
    }
}

/// A library whose rows belong to `playlist`, with `tracks` loaded.
fn library_selected(playlist: &str, tracks: Vec<TrackEntry>) -> Library {
    let mut library = Library::default();
    library.select(playlist);
    library.apply(&Update::Tracks {
        playlist: playlist.to_owned(),
        tracks,
        cover: None,
        access: None,
    });
    library
}

/// One frame of the tracklist at the test's fixed clock.
fn tracklist_frame(ctx: &Context, table: Table, library: &mut Library) -> egui::FullOutput {
    let tokens = Tokens::DARK;
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    run(ctx, input(Vec::new()), |ui| {
        let _ = tracks_view(
            ui,
            table,
            library,
            &mut transport,
            &mut queue,
            &mut Art::default(),
            Instant::now(),
            NOW_MS,
            &tokens,
        );
    })
}

/// One frame of the tracklist in a window of `size`.
fn tracklist_frame_in(ctx: &Context, size: Vec2, library: &mut Library) -> egui::FullOutput {
    let tokens = Tokens::DARK;
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    run(ctx, sized_input(size, Vec::new()), |ui| {
        let _ = tracks_view(
            ui,
            Table::Full,
            library,
            &mut transport,
            &mut queue,
            &mut Art::default(),
            Instant::now(),
            NOW_MS,
            &tokens,
        );
    })
}

/// Draws one frame of the tracklist and returns the text it painted.
fn painted_lines(ctx: &Context, table: Table, library: &mut Library) -> Vec<String> {
    drawn_texts(&tracklist_frame(ctx, table, library))
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// The visible run of every painted line: an elided line is read as what it
/// shows, not as the string it was given.
fn painted_runs(ctx: &Context, table: Table, library: &mut Library) -> Vec<String> {
    tracklist_frame(ctx, table, library)
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => text.galley.rows.first(),
            _ => None,
        })
        .map(|row| row.row.text().to_owned())
        .collect()
}

/// The access a read hands a page: what it may change, and the version it was
/// read at.
fn access(editable: bool) -> Option<PlaylistAccess> {
    Some(PlaylistAccess {
        revision: Revision::from_wire(b"rev-1").expect("a non-empty revision"),
        editable,
    })
}

fn library_with(tracks: Vec<TrackEntry>) -> Library {
    let mut library = Library::default();
    library.select("spotify:playlist:a");
    library.apply(&Update::Tracks {
        playlist: "spotify:playlist:a".to_owned(),
        tracks,
        cover: None,
        access: access(true),
    });
    library
}

/// The same page, read at a revision the user may not write.
fn read_only_library_with(tracks: Vec<TrackEntry>) -> Library {
    let mut library = Library::default();
    library.select("spotify:playlist:a");
    library.apply(&Update::Tracks {
        playlist: "spotify:playlist:a".to_owned(),
        tracks,
        cover: None,
        access: access(false),
    });
    library
}

/// The same selection, with the playlist list ready so the library can name
/// it. `selected_name` is the switch the hero hangs on.
fn library_named(tracks: Vec<TrackEntry>, name: &str) -> Library {
    let mut library = library_with(tracks);
    library.apply(&Update::Playlists(vec![PlaylistEntry {
        composed: Vec::new(),
        uri: "spotify:playlist:a".to_owned(),
        name: name.to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: None,
        access: None,
    }]));
    library
}

/// The same selection, with a playlist entry that carries a description and
/// an owner, so the band has both to draw.
fn library_detailed(
    tracks: Vec<TrackEntry>,
    name: &str,
    description: Option<&str>,
    owner: Option<&str>,
) -> Library {
    let mut library = library_with(tracks);
    library.apply(&Update::Playlists(vec![listed(name, description, owner)]));
    library
}

/// The selection listed but its tracks not asked for yet: the band draws
/// before anything is ready.
fn library_listed(name: &str, description: Option<&str>, owner: Option<&str>) -> Library {
    let mut library = Library::default();
    library.select("spotify:playlist:a");
    library.apply(&Update::Playlists(vec![listed(name, description, owner)]));
    library
}

/// The same page, heading a list of our own.
fn library_ours(tracks: Vec<TrackEntry>, name: &str) -> Library {
    let mut library = library_with(tracks);
    let mut entry = listed(name, None, None);
    entry.owner = Some(Owner::You);
    library.apply(&Update::Playlists(vec![entry]));
    library
}

fn listed(name: &str, description: Option<&str>, owner: Option<&str>) -> PlaylistEntry {
    PlaylistEntry {
        composed: Vec::new(),
        uri: "spotify:playlist:a".to_owned(),
        name: name.to_owned(),
        named: true,
        cover: None,
        description: description.map(str::to_owned),
        owner: owner.and_then(OwnerName::from_wire).map(Owner::Named),
        tracks: None,
        access: None,
    }
}

/// `count` tracks, all `duration_ms` long, so the band's total is exact.
fn timed_tracks(count: usize, duration_ms: u32) -> Vec<TrackEntry> {
    (0..count)
        .map(|index| TrackEntry {
            duration_ms: Some(duration_ms),
            ..track(&format!("t{index}"), &format!("t{index}"))
        })
        .collect()
}

fn playlist(uri: &str, name: &str) -> PlaylistEntry {
    PlaylistEntry {
        composed: Vec::new(),
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: None,
        access: None,
    }
}

fn show(uri: &str, name: &str) -> takofy_model::FollowedShow {
    takofy_model::FollowedShow {
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        publisher: None,
        cover: None,
        episodes: None,
    }
}

fn cover_url(tail: &str) -> CoverUrl {
    CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
}

fn cover_set(tail: &str) -> CoverSet {
    CoverSet::single(cover_url(tail))
}

/// A cover offering 64, 300 and 640 squares, with their URLs.
fn sized(tail: &str) -> (CoverSet, CoverUrl, CoverUrl, CoverUrl) {
    let small = cover_url(&format!("{tail}-64"));
    let middle = cover_url(&format!("{tail}-300"));
    let large = cover_url(&format!("{tail}-640"));
    let set = CoverSet::from_sizes([
        (Some((64, 64)), small.clone()),
        (Some((300, 300)), middle.clone()),
        (Some((640, 640)), large.clone()),
    ])
    .expect("a cover set");
    (set, small, middle, large)
}

fn playlist_with_cover(uri: &str, name: &str, cover: CoverUrl) -> PlaylistEntry {
    PlaylistEntry {
        composed: Vec::new(),
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        cover: Some(PlaylistCover::derived(CoverSet::single(cover))),
        description: None,
        owner: None,
        tracks: None,
        access: None,
    }
}

fn track_with_cover(index: usize) -> TrackEntry {
    TrackEntry {
        uri: format!("t{index}"),
        title: format!("t{index}"),
        artists: String::new(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: Some(cover_set(&format!("c{index}"))),
    }
}

fn image() -> CoverImage {
    CoverImage::new(1, 1, vec![0; 4]).expect("a 1x1 image")
}

/// A playlist page whose band heads `covers`, one per track, ready.
fn mosaic_library(covers: &[&str]) -> Library {
    let tracks = covers
        .iter()
        .enumerate()
        .map(|(index, tail)| TrackEntry {
            cover: Some(cover_set(tail)),
            ..track(&format!("t{index}"), &format!("t{index}"))
        })
        .collect();
    library_detailed(tracks, "Evening", Some(DESCRIPTION), None)
}

/// The same page, with a picture of its own to fall back to.
fn mosaic_library_with_picture(covers: &[&str]) -> Library {
    let mut library = mosaic_library(covers);
    library.apply(&Update::Playlists(vec![PlaylistEntry {
        composed: Vec::new(),
        cover: Some(PlaylistCover::derived(CoverSet::single(cover_url("m0")))),
        ..listed("Evening", Some(DESCRIPTION), None)
    }]));
    library
}

/// A long list whose first four tracks carry distinct covers, so the band has
/// a mosaic to draw beside a full byline and description.
fn mosaic_full() -> Library {
    let mut tracks = timed_tracks(62, 223_548);
    for (index, track) in tracks.iter_mut().enumerate().take(mosaic::QUADRANTS) {
        track.cover = Some(cover_set(&format!("m{index}")));
    }
    library_detailed(tracks, "Evening", Some(DESCRIPTION), Some("alice"))
}

/// Holds every URL in `art`, as the fetches would have by the time the band
/// draws.
///
/// One URL at a time: a batch larger than the store's in-flight cap would
/// leave its tail wanted and never sent.
fn hold(ctx: &Context, art: &mut Art, urls: &[CoverUrl], now: Instant) {
    for url in urls {
        art.begin_frame();
        let _ = art.look(Some(&CoverSet::single(url.clone())), 64, now);
        while !art.requests(now).is_empty() {}
        art.apply(
            &Update::Cover {
                url: url.clone(),
                image: image(),
            },
            now,
        );
        art.upload(ctx, crate::theme::TintParams::DARK.alpha_floor);
    }
}

/// Holds `url` with a one-pixel cover of `rgba`, so its tint is known.
fn hold_pixel(ctx: &Context, art: &mut Art, url: &CoverUrl, rgba: [u8; 4], now: Instant) {
    art.begin_frame();
    let _ = art.look(Some(&CoverSet::single(url.clone())), 64, now);
    while !art.requests(now).is_empty() {}
    art.apply(
        &Update::Cover {
            url: url.clone(),
            image: CoverImage::new(1, 1, rgba.to_vec()).expect("a 1x1 image"),
        },
        now,
    );
    art.upload(ctx, crate::theme::TintParams::DARK.alpha_floor);
}

/// Drives `url` to a failure it will not retry, as three failed answers do.
///
/// The second and third attempts are only sent once their retry delay is
/// due, so the clock moves with the store's own schedule.
fn fail_permanently(art: &mut Art, url: &CoverUrl, now: Instant) -> Instant {
    let mut at = now;
    for delay in [
        Duration::ZERO,
        Duration::from_secs(5),
        Duration::from_secs(30),
    ] {
        at += delay;
        art.begin_frame();
        let _ = art.look(Some(&CoverSet::single(url.clone())), 64, at);
        while !art.requests(at).is_empty() {}
        art.apply(&Update::CoverFailed { url: url.clone() }, at);
    }
    at
}

/// One frame of the tracklist with a store the test owns.
fn band_frame_with(
    ctx: &Context,
    library: &mut Library,
    art: &mut Art,
    now: Instant,
) -> egui::FullOutput {
    let tokens = Tokens::DARK;
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    run(ctx, input(Vec::new()), |ui| {
        let _ = tracks_view(
            ui,
            Table::Full,
            library,
            &mut transport,
            &mut queue,
            art,
            now,
            NOW_MS,
            &tokens,
        );
    })
}

/// The covers the band paints, read from the art box's own hairline.
fn band_art(output: &egui::FullOutput) -> Rect {
    let tokens = Tokens::DARK;
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape)
                if (shape.rect.width() - crate::theme::hero::ART).abs() < 0.01
                    && (shape.rect.height() - crate::theme::hero::ART).abs() < 0.01
                    && (shape.stroke.width - stroke::HAIRLINE).abs() < 0.01
                    && shape.stroke.color == tokens.palette.border_strong =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
        .expect("the band draws its art box")
}

/// The textured rects inside `area`, in paint order.
fn textured(output: &egui::FullOutput, area: Rect) -> Vec<Rect> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape)
                if shape.brush.is_some() && area.contains(shape.rect.center()) =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
        .collect()
}

/// The sunken holes inside `area`, in paint order.
fn holes(output: &egui::FullOutput, area: Rect) -> Vec<Rect> {
    let tokens = Tokens::DARK;
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape)
                if shape.brush.is_none()
                    && shape.fill == tokens.palette.surface_sunken
                    && area.contains(shape.rect.center()) =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
        .collect()
}

/// A held cover's drawing, so a quadrant can be named without a texture id.
fn drawn_cover(art: &mut Art, tail: &str, now: Instant) -> Cover {
    art.look(
        Some(&cover_set(tail)),
        crate::cover::want_px(crate::theme::hero::ART / 2.0, 1.0),
        now,
    )
}

fn fetch_urls(commands: Vec<Command>) -> Vec<CoverUrl> {
    commands
        .into_iter()
        .filter_map(|command| match command {
            Command::Fetch { url } => Some(url),
            _ => None,
        })
        .collect()
}

/// A library whose playlists are ready and whose tracks are untouched: the
/// idle state the grid fills.
fn library_idle(playlists: Vec<PlaylistEntry>) -> Library {
    let mut library = Library::default();
    library.apply(&Update::Playlists(playlists));
    library
}

/// The rect the idle grid draws the `index`-th card into, solved the same
/// way the view solves it: the leading space, then the grid at the current
/// width. Nothing here is a coordinate copied out of the view.
fn card_rect_for(ctx: &Context, index: usize) -> Rect {
    let tokens = Tokens::DARK;
    let mut found = None;
    run(ctx, input(Vec::new()), |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            crate::view::inner(ui, |ui| {
                page_head::show(ui, "Your Library", &library_note(2), &tokens);
                let _ = filter_row::show_with(
                    ui,
                    &tokens,
                    &library_filter::ROW,
                    RootKind::All,
                    RootKind::control,
                    root_filter_label,
                    filter_row::RowSlot::Trailing(0.0),
                );
                let solved = media_grid(ui);
                found = Some(solved.card_rect(index));
            });
        });
    });
    found.expect("the idle grid lays out")
}

/// A context with the app's fonts and style installed.
fn context() -> Context {
    let tokens = Tokens::DARK;
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&tokens));
    ctx
}

/// The rect `tracks_view` draws the `index`-th row into, solved the same way
/// the view solves it: the header, then the whole list reserved in one block,
/// then `layout::table::row_rect` for the row.
///
/// `Table::Full` is the width that gives every column room, so the row is
/// solved with all seven tracks present. Nothing here is a coordinate copied
/// out of the view: the header and the row pitch are the view's own solver.
fn row_rect_for(ctx: &Context, count: usize, index: usize) -> Rect {
    let tokens = Tokens::DARK;
    let table = Table::Full;
    let mut found = None;
    run(ctx, input(Vec::new()), |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            crate::view::inner(ui, |ui| {
                let width = ui.available_width();
                actionbar::row_rect(ui);
                let columns = table_layout::columns(width, table.columns());
                head(ui, columns, &tokens);
                let list = list_rect(ui, count);
                found = Some(table_layout::row_rect(list, index, extent::ROW_H));
            });
        });
    });
    found.expect("the header and the list lay out")
}

/// Makes the library's liked set these tracks, as a landed read would.
fn liked(library: &mut Library, uris: &[&str]) {
    assert!(library.likes.request(true));
    library.likes.apply(&crate::likes::Event::Set(
        uris.iter().map(|uri| (*uri).to_owned()).collect(),
    ));
}

/// The action row's tiles, solved by the row itself. The probe window is the
/// page's own width, so the form is the one the page would have there.
fn tiles_of(row: Rect) -> crate::layout::actionbar::Placed {
    crate::layout::actionbar::solve(
        row,
        crate::layout::actionbar::Kind::Playlist(crate::layout::actionbar::Form::at(
            PROBE.x,
            Table::Full,
        )),
        crate::layout::actionbar::Parts {
            shuffle: true,
            field: true,
            follow: None,
        },
    )
}

/// The rect `tracks_view` draws the action row's play control into, solved
/// the way the view solves it: the band, then the row's slot in the flow, then
/// the row's own solver.
fn play_tile_rect_for(ctx: &Context, title: &str) -> Rect {
    let tokens = Tokens::DARK;
    let mut found = None;
    run(ctx, input(Vec::new()), |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let band = super::hero::Band {
                eyebrow: "Playlist",
                title,
                meta: Vec::new(),
                credit: None,
                description: "",
                cover: Cover::Missing,
                tint: None,
            };
            let _ = super::hero::show_with(ui, &band, &tokens, super::hero::Title::Plain);
            let row = actionbar::row_rect(ui);
            found = Some(tiles_of(row).play);
        });
    });
    found.expect("the band and its action row lay out")
}

/// Clicks `centre` `times` times, one event per pass, and returns the command
/// the view reported while they happened. `screen` is the probe window, so a
/// test that needs the band and two whole rows can ask for a taller one.
fn clicks(
    screen: Vec2,
    ctx: &Context,
    centre: Pos2,
    times: usize,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
) -> Option<Command> {
    let tokens = Tokens::DARK;
    let mut command = None;
    // Warm up so hit-testing has a previous pass to read.
    run(ctx, sized_input(screen, Vec::new()), |ui| {
        let _ = tracks_view(
            ui,
            Table::Full,
            library,
            transport,
            queue,
            &mut Art::default(),
            Instant::now(),
            NOW_MS,
            &tokens,
        );
    });
    for _ in 0..times {
        for event in crate::view::test_support::click_events(centre) {
            let command = &mut command;
            let library = &mut *library;
            let transport = &mut *transport;
            let queue = &mut *queue;
            run(ctx, sized_input(screen, vec![event]), |ui| {
                if let Some(next) = tracks_view(
                    ui,
                    Table::Full,
                    library,
                    transport,
                    queue,
                    &mut Art::default(),
                    Instant::now(),
                    NOW_MS,
                    &tokens,
                ) {
                    *command = Some(next);
                }
            });
        }
    }
    command
}

/// One click on `centre`, and the command the view returned while it happened.
fn click(
    ctx: &Context,
    centre: Pos2,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
) -> Option<Command> {
    clicks(PROBE, ctx, centre, 1, library, transport, queue)
}

/// The same click, in a window of `screen`: a point read from a frame drawn
/// in a wider window only lands on the same widget where it was drawn.
fn click_in(
    screen: Vec2,
    ctx: &Context,
    centre: Pos2,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
) -> Option<Command> {
    clicks(screen, ctx, centre, 1, library, transport, queue)
}

/// One double click on `centre`, and the command the view returned while it
/// happened.
fn double_click(
    ctx: &Context,
    centre: Pos2,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
) -> Option<Command> {
    clicks(PROBE, ctx, centre, 2, library, transport, queue)
}

/// Whether `playlist`'s list holds `index` as part of its selection.
fn picked(ctx: &Context, playlist: &str, count: usize, index: usize) -> bool {
    crate::view::carry::List::open(ctx, egui::Id::unique(("playlist-tracks", playlist)), count)
        .look(index)
        .selected
}

/// One pass of the tracklist with one event, and the command it reported.
fn pass(
    ctx: &Context,
    event: Event,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
) -> Option<Command> {
    let tokens = Tokens::DARK;
    let mut command = None;
    run(ctx, input(vec![event]), |ui| {
        command = tracks_view(
            ui,
            Table::Full,
            library,
            transport,
            queue,
            &mut Art::default(),
            Instant::now(),
            NOW_MS,
            &tokens,
        );
    });
    command
}

/// A press on `from` and a nine-point drag: the passes that lift a row.
fn lift_row(
    ctx: &Context,
    from: Pos2,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
) {
    let mut events = vec![
        Event::PointerMoved(from),
        crate::view::test_support::button(from, true),
    ];
    for step in 1..=3 {
        events.push(Event::PointerMoved(
            from + Vec2::new(step as f32 * 3.0, 0.0),
        ));
    }
    for event in events {
        let _ = pass(ctx, event, library, transport, queue);
    }
}

/// The band's worded parts below: 62 tracks of 223_548 ms sum to 13_859_976
/// ms, which the byline rounds to `3 hr 51 min`.
const DESCRIPTION: &str = "Windows down, city behind you.";

/// The box a text shape with `label` was painted in.
fn text_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
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

// -----------------------------------------------------------------------
// The filter field: the rows it leaves, the count it paints, and Escape.
// -----------------------------------------------------------------------

const PLAYLIST: &str = "spotify:playlist:a";

/// A window tall enough to hold the band-less page's action row, head and all
/// sixty-two rows, so a restore can be read row by row. Wider than the rail's
/// 900, because the count is not drawn in a rail.
const ALL: Vec2 = Vec2::new(1000.0, 3200.0);

/// The twelve true indices a `midnight` query leaves.
const MIDNIGHT: [usize; 12] = [1, 4, 9, 12, 20, 23, 31, 38, 39, 51, 57, 60];

/// Sixty-two rows whose artist says `Midnight Artist` at twelve true
/// positions, so the title can stay `t<index>` for `painted_rows`.
fn midnight_tracks() -> Vec<TrackEntry> {
    (0..62)
        .map(|index| TrackEntry {
            artists: if MIDNIGHT.contains(&index) {
                "Midnight Artist".to_owned()
            } else {
                "Vela Moreno".to_owned()
            },
            ..track(&format!("t{index}"), &format!("t{index}"))
        })
        .collect()
}

/// One frame of the tracklist in a window of `size`, carrying `events`.
fn frame_in(
    ctx: &Context,
    size: Vec2,
    library: &mut Library,
    events: Vec<Event>,
) -> egui::FullOutput {
    table_frame_in(ctx, size, Table::Full, library, events)
}

/// One frame of the tracklist handed `table`, so a test can put the page in
/// the form its own table names at a width the page is not classified for.
fn table_frame_in(
    ctx: &Context,
    size: Vec2,
    table: Table,
    library: &mut Library,
    events: Vec<Event>,
) -> egui::FullOutput {
    let tokens = Tokens::DARK;
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    run(ctx, sized_input(size, events), |ui| {
        let _ = tracks_view(
            ui,
            table,
            library,
            &mut transport,
            &mut queue,
            &mut Art::default(),
            Instant::now(),
            NOW_MS,
            &tokens,
        );
    })
}

/// The field's bottom hairline: the box it spans and the ink it carries.
fn filter_hairline(output: &egui::FullOutput) -> (Rect, egui::Color32) {
    let tokens = Tokens::DARK;
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape)
                if (shape.rect.height() - crate::theme::stroke::HAIRLINE).abs() < 0.01
                    && shape.rect.width() <= 240.5
                    && shape.rect.width() > 80.0
                    && (shape.fill == tokens.palette.border
                        || shape.fill == tokens.palette.text) =>
            {
                Some((shape.rect, shape.fill))
            }
            _ => None,
        })
        .min_by(|left, right| left.0.top().total_cmp(&right.0.top()))
        .expect("the filter field paints its bottom hairline")
}

/// The filter field's box, read from the hairline it paints under itself:
/// the line is the field's bottom, and the field is one large button tall.
fn field_at(output: &egui::FullOutput) -> Rect {
    let line = filter_hairline(output).0;
    Rect::from_min_max(
        Pos2::new(line.left(), line.bottom() - extent::BUTTON_LG),
        Pos2::new(line.right(), line.bottom()),
    )
}

/// A title's box and the rect the frame clipped it to.
fn clipped_title(output: &egui::FullOutput, label: &str) -> Option<(Rect, Rect)> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => Some((
                Rect::from_min_size(text.pos, text.galley.size()),
                clipped.clip_rect,
            )),
            _ => None,
        })
}

mod band;
mod band_rename;
mod dispatch;
mod filter;
mod hero;
mod pinned;
mod root;
mod row_layout;
mod rows;
