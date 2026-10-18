//! The main pane's tracklist: the selected playlist's tracks, or the state of
//! their load.
//!
//! The header and the rows are solved from one width and one set of tokens, so the
//! columns line up by construction rather than because two pieces agree. The header
//! scrolls with the list, which is what the reference's tracklist does.

use egui::emath::GuiRounding;
use egui::{FontId, Pos2, Rect, Sense, Ui, vec2};
use takofy_model::TrackEntry;

use crate::format::{format_position, hero_meta};
use crate::icon;
use crate::layout::{Table, table as table_layout};
use crate::library::{Library, Load};
use crate::message::Command;
use crate::player::{Transport, TransportState};
use crate::queue::Queue;
use crate::theme::icon::Size;
use crate::theme::{Tokens, extent, space, stroke, text, tracking};

use super::hero;
use super::text as recipe;
use super::track_row::{self, Row, State};

/// The main pane: the selected playlist's tracks, or the state of their load.
pub fn tracks_view(
    ui: &mut Ui,
    table: Table,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    tokens: &Tokens,
) -> Option<Command> {
    ui.add_space(space::S3);

    match &library.tracks {
        Load::Idle => {
            ui.label(recipe::muted("Select a playlist.", tokens));
            return None;
        }
        Load::Loading => {
            ui.spinner();
            return None;
        }
        Load::Ready(tracks) if tracks.tracks.is_empty() => {
            ui.label(recipe::muted("No tracks in this playlist.", tokens));
            return None;
        }
        Load::Failed(failure) => {
            ui.label(recipe::muted(
                crate::signin::failure_message(*failure),
                tokens,
            ));
            if ui.button("Retry").clicked() {
                let playlist = library.selected().map(str::to_owned);
                return playlist.map(|playlist| library.select(&playlist));
            }
            return None;
        }
        Load::Ready(_) => {}
    }

    let mut command = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        let width = ui.available_width();
        let columns = table_layout::columns(width, table.columns());

        let Load::Ready(tracks) = &library.tracks else {
            return;
        };
        let count = tracks.tracks.len();

        // The band heads the playlist and is drawn only when the library can name
        // the selection; with no name there is nothing to say above the rows. It
        // takes the width and starts at the cursor, so the header and the rows
        // follow it down one scroll area. An empty playlist never reaches here —
        // its own state returns above — so the band only ever heads a playable
        // list.
        if let Some(title) = library.selected_name() {
            let band = hero::Band {
                eyebrow: "Playlist",
                title,
                meta: hero_meta(count, total_duration_ms(&tracks.tracks)),
                playable: count > 0,
            };
            if hero::show(ui, &band, tokens) {
                command = queue.start(&tracks.tracks, 0, transport);
            }
        }

        head(ui, columns, tokens);
        // The whole list is reserved at once, so the scrollbar is right, and only
        // the rows the viewport can show are laid out and given interaction: a
        // thousand-track playlist paints the dozen rows on screen.
        let list = list_rect(ui, count);
        ui.allocate_rect(list, Sense::hover());
        let visible = ui.clip_rect();

        for (index, entry) in tracks.tracks.iter().enumerate() {
            let rect = table_layout::row_rect(list, index, extent::ROW_H);
            if !visible.intersects(rect) {
                continue;
            }
            let duration = entry.duration_ms.map(format_position);
            let row = Row {
                uri: &entry.uri,
                number: index + 1,
                title: &entry.title,
                artist: &entry.artists,
                album: entry.album.as_deref(),
                // The added date is playlist membership rather than track metadata,
                // so the window is not handed one yet.
                added: None,
                duration: duration.as_deref(),
                state: state_of(&entry.uri, transport),
            };
            if track_row::show(ui, rect, columns, &row, tokens) {
                command = queue.start(&tracks.tracks, index, transport);
            }
        }
    });

    command
}

/// The sum of the track lengths, in milliseconds, or zero when any length is
/// unknown.
///
/// The model holds each length as an `Option`: a track that could not be
/// labelled carries none, exactly as it carries no album. A sum over only the
/// lengths that happen to be present is a lower bound, not the runtime, but the
/// byline would read an exact figure with nothing to mark it as a bound; the
/// missing lengths are already blank on their own rows, so a total that hides
/// its gap is worse than no total at all. A list with any unknown length
/// therefore reports zero — the absent total `hero_meta` leaves out — and a
/// wholly known list reports its honest sum. Saturating addition keeps an absurd
/// list from wrapping.
fn total_duration_ms(tracks: &[TrackEntry]) -> u32 {
    if tracks.iter().any(|entry| entry.duration_ms.is_none()) {
        return 0;
    }
    tracks
        .iter()
        .filter_map(|entry| entry.duration_ms)
        .fold(0u32, u32::saturating_add)
}

/// The rect the rows are laid out in: the list's width, and the height of every
/// row, starting where the cursor is — which is under the header.
///
/// The view and its test both call this, so a row's place and a test's click
/// cannot drift apart.
fn list_rect(ui: &mut Ui, count: usize) -> Rect {
    Rect::from_min_size(
        ui.cursor().min,
        vec2(ui.available_width(), extent::ROW_H * count as f32),
    )
}

/// How a row relates to the transport.
fn state_of(uri: &str, transport: &Transport) -> State {
    let current = transport
        .now_playing
        .as_ref()
        .is_some_and(|now| now.uri == uri);
    match (current, transport.state) {
        (false, _) => State::Resting,
        (true, TransportState::Playing) => State::Playing,
        (true, _) => State::Paused,
    }
}

/// The column headings, in the same columns as the rows beneath them.
fn head(ui: &mut Ui, columns: table_layout::Columns, tokens: &Tokens) {
    let font = recipe::track_mono(text::XXS);
    let line = ui.ctx().fonts_mut(|fonts| fonts.row_height(&font));
    let band = ui
        .allocate_space(vec2(ui.available_width(), line + space::S2))
        .1;
    let columns = columns.at(Pos2::new(band.left(), band.top()));
    let cell = |rect: Rect| Rect::from_x_y_ranges(rect.x_range(), band.y_range());

    heading(ui, cell(columns.index), "#", true, &font, tokens);
    heading(ui, cell(columns.title), "Title", false, &font, tokens);
    heading(ui, cell(columns.album), "Album", false, &font, tokens);
    heading(ui, cell(columns.added), "Added", false, &font, tokens);

    // The duration column is headed by a clock rather than by a word, and the like
    // and the actions columns carry no heading: a label over a glyph repeats it.
    let clock = cell(columns.duration);
    if clock.width() > 0.0 {
        icon::paint(
            ui.painter(),
            icon::Icon::Clock,
            Rect::from_center_size(clock.center(), vec2(Size::SM.side, Size::SM.side))
                .round_to_pixels(ui.pixels_per_point()),
            Size::SM,
            tokens.palette.text_subtle,
        );
    }

    ui.painter().rect_filled(
        Rect::from_min_size(
            Pos2::new(band.left(), band.bottom()),
            vec2(band.width(), stroke::HAIRLINE),
        ),
        0.0,
        tokens.palette.border,
    );
    ui.add_space(space::S1);
}

/// One heading, upper-cased and tracked apart, in its column.
fn heading(ui: &Ui, rect: Rect, label: &str, right: bool, font: &FontId, tokens: &Tokens) {
    if rect.width() <= 0.0 || label.is_empty() {
        return;
    }
    let galley = recipe::line(
        ui,
        &label.to_uppercase(),
        font.clone(),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XXS, tracking::CAPS),
        rect.width(),
    );
    let x = if right {
        rect.right() - galley.size().x
    } else {
        rect.left()
    };
    let y = rect.center().y - galley.size().y / 2.0;
    ui.painter()
        .galley(Pos2::new(x, y), galley, tokens.palette.text_subtle);
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, Modifiers, PointerButton, RawInput, Vec2};

    use super::*;
    use crate::library::Library;
    use crate::message::Update;
    use crate::theme::hero::PLAY;
    use crate::theme::{fonts, style};
    use takofy_model::{PlaylistEntry, TrackEntry};

    /// The probe window: 900 by 400, the size every bare-row test runs in.
    const PROBE: Vec2 = Vec2::new(900.0, 400.0);
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

    /// Runs one pass and consumes the font-atlas delta it may have produced.
    fn run(ctx: &Context, input: RawInput, draw: impl FnMut(&mut Ui)) {
        let mut output = ctx.run_ui(input, draw);
        output.textures_delta.clear();
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

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

    fn track(uri: &str, title: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: title.to_owned(),
            artists: String::new(),
            album: None,
            duration_ms: None,
            named: true,
        }
    }

    fn library_with(tracks: Vec<TrackEntry>) -> Library {
        let mut library = Library::default();
        library.select("spotify:playlist:a");
        library.apply(&Update::Tracks {
            playlist: "spotify:playlist:a".to_owned(),
            tracks,
        });
        library
    }

    /// The same selection, with the playlist list ready so the library can name
    /// it. `selected_name` is the switch the hero hangs on.
    fn library_named(tracks: Vec<TrackEntry>, name: &str) -> Library {
        let mut library = library_with(tracks);
        library.apply(&Update::Playlists(vec![PlaylistEntry {
            uri: "spotify:playlist:a".to_owned(),
            name: name.to_owned(),
            named: true,
        }]));
        library
    }

    fn track_ms(uri: &str, duration_ms: u32) -> TrackEntry {
        TrackEntry {
            duration_ms: Some(duration_ms),
            ..track(uri, uri)
        }
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
            ui.add_space(space::S3);
            egui::ScrollArea::vertical().show(ui, |ui| {
                let width = ui.available_width();
                let columns = table_layout::columns(width, table.columns());
                head(ui, columns, &tokens);
                let list = list_rect(ui, count);
                found = Some(table_layout::row_rect(list, index, extent::ROW_H));
            });
        });
        found.expect("the header and the list lay out")
    }

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
            ui.add_space(space::S3);
            egui::ScrollArea::vertical().show(ui, |ui| {
                let width = ui.available_width();
                let columns = table_layout::columns(width, table.columns());
                let band = hero::Band {
                    eyebrow: "Playlist",
                    title,
                    meta: Vec::new(),
                    playable: count > 0,
                };
                let _ = hero::show(ui, &band, &tokens);
                head(ui, columns, &tokens);
                let list = list_rect(ui, count);
                found = Some(table_layout::row_rect(list, index, extent::ROW_H));
            });
        });
        found.expect("the hero, header and list lay out")
    }

    /// The rect `tracks_view` draws the hero's play control into, solved the way
    /// the view solves it: the band takes the width and a fixed height, the action
    /// row a fixed height under it, and the tile sits at the row's left with the
    /// gutter. The band is drawn through the view's own painter to advance the
    /// layout, so only the action row's height is re-derived here.
    fn play_tile_rect_for(ctx: &Context, title: &str) -> Rect {
        let tokens = Tokens::DARK;
        let mut found = None;
        run(ctx, input(Vec::new()), |ui| {
            ui.add_space(space::S3);
            egui::ScrollArea::vertical().show(ui, |ui| {
                let width = ui.available_width();
                let top = ui.cursor().min;
                let band = hero::Band {
                    eyebrow: "Playlist",
                    title,
                    meta: Vec::new(),
                    playable: true,
                };
                let _ = hero::show(ui, &band, &tokens);
                let bottom = ui.cursor().min.y;
                let height = PLAY + 2.0 * space::S4;
                let row =
                    Rect::from_min_size(Pos2::new(top.x, bottom - height), vec2(width, height));
                let centre = row.center().y;
                found = Some(Rect::from_min_size(
                    Pos2::new(row.left() + space::GUTTER, centre - PLAY / 2.0),
                    Vec2::splat(PLAY),
                ));
            });
        });
        found.expect("the hero and its action row lay out")
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
            let _ = tracks_view(ui, Table::Full, library, transport, queue, &tokens);
        });
        for _ in 0..times {
            for event in [
                Event::PointerMoved(centre),
                button(centre, true),
                button(centre, false),
            ] {
                let command = &mut command;
                let library = &mut *library;
                let transport = &mut *transport;
                let queue = &mut *queue;
                run(ctx, sized_input(screen, vec![event]), |ui| {
                    if let Some(next) =
                        tracks_view(ui, Table::Full, library, transport, queue, &tokens)
                    {
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
            let _ = tracks_view(ui, Table::Full, library, transport, queue, &tokens);
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
    fn a_double_click_on_a_row_starts_that_track_through_the_queue() {
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0"), track("b", "t1")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        // The click is where the view draws row 0, not a probe's geometry.
        let centre = row_rect_for(&ctx, 2, 0).center();
        let command = double_click(&ctx, centre, &mut library, &mut transport, &mut queue);

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
        assert!(
            queue.can_next(),
            "the double-clicked row must start the queue, not only the transport"
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("a")
        );
    }

    #[test]
    fn a_single_track_list_starts_its_only_track() {
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let centre = row_rect_for(&ctx, 1, 0).center();
        let command = double_click(&ctx, centre, &mut library, &mut transport, &mut queue);

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
        assert!(
            !queue.can_next(),
            "a one-track queue has nothing after its only entry"
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("a")
        );
    }

    #[test]
    fn a_row_with_no_album_added_or_duration_still_lays_out() {
        let tokens = Tokens::DARK;
        let ctx = context();
        // An album or an artist context carries neither an album name nor a
        // playlist's added date, and a track may have no length. Solving its
        // columns and double-clicking must still lay the row out and activate it.
        let columns = table_layout::columns(900.0, Table::Full.columns());
        let rect = table_layout::row_rect(
            Rect::from_min_size(Pos2::ZERO, vec2(900.0, extent::ROW_H)),
            0,
            extent::ROW_H,
        );
        let row = Row {
            uri: "a",
            number: 1,
            title: "t0",
            artist: "",
            album: None,
            added: None,
            duration: None,
            state: State::Resting,
        };

        run(&ctx, input(Vec::new()), |ui| {
            let _ = track_row::show(ui, rect, columns, &row, &tokens);
        });
        let mut clicked = false;
        for _ in 0..2 {
            for event in [
                Event::PointerMoved(rect.center()),
                button(rect.center(), true),
                button(rect.center(), false),
            ] {
                run(&ctx, input(vec![event]), |ui| {
                    clicked |= track_row::show(ui, rect, columns, &row, &tokens);
                });
            }
        }
        assert!(
            clicked,
            "a row with no album, added or duration must still lay out and activate"
        );
    }

    #[test]
    fn a_row_given_an_album_and_a_duration_draws_them() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let columns = table_layout::columns(900.0, Table::Full.columns());
        let rect = table_layout::row_rect(
            Rect::from_min_size(Pos2::ZERO, vec2(900.0, extent::ROW_H)),
            0,
            extent::ROW_H,
        );
        let row = Row {
            uri: "a",
            number: 1,
            title: "t0",
            artist: "Band",
            album: Some("Record"),
            added: None,
            duration: Some("3:42"),
            state: State::Resting,
        };

        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = track_row::show(ui, rect, columns, &row, &tokens);
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"Record"),
            "the album column must draw its label: {texts:?}"
        );
        assert!(
            texts.contains(&"3:42"),
            "the duration column must draw its label: {texts:?}"
        );
    }

    #[test]
    fn the_view_hands_a_tracks_album_and_length_to_its_row() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let mut library = library_with(vec![TrackEntry {
            uri: "a".to_owned(),
            title: "t0".to_owned(),
            artists: "Band".to_owned(),
            album: Some("Record".to_owned()),
            duration_ms: Some(222_000),
            named: true,
        }]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &tokens,
            );
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"Record"),
            "the row must be handed the track's album: {texts:?}"
        );
        assert!(
            texts.contains(&"3:42"),
            "the row must be handed the track's length as m:ss: {texts:?}"
        );
    }

    #[test]
    fn the_second_of_two_identical_entries_starts_the_second() {
        // Two entries holding one uri: keyed by the uri alone, both rows would
        // share one egui id, so one click would drive both and the higher index
        // would win. The command's uri is `a` whichever row is taken, so the
        // queue's position is the only thing that tells the two apart: starting
        // the second leaves nothing after it, while the first would leave entry
        // two. `Queue::can_previous` reports only that the queue is non-empty, so
        // it cannot sharpen this; `can_next` is the sharp assertion.
        //
        // Index 1 also exercises `row_rect`'s pitch: the other tests only ever
        // click index 0.
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0"), track("a", "t0")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let first = row_rect_for(&ctx, 2, 0);
        let second = row_rect_for(&ctx, 2, 1);
        assert!(
            (second.top() - first.bottom()).abs() < 0.01,
            "the second row must start where the first ends: {first:?} then {second:?}"
        );
        let command = double_click(
            &ctx,
            second.center(),
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
        assert!(
            !queue.can_next(),
            "the second of two identical entries was started, not the first"
        );
    }

    #[test]
    fn a_double_click_on_the_first_of_two_identical_entries_starts_the_first() {
        // The mirror of the test above. When the two rows shared one id, the
        // higher index overwrote the lower, so clicking the first row started
        // the second; this pins that the first is the one that answers, and it
        // fails against the uri-only key where the other test alone would not.
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0"), track("a", "t0")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let first = row_rect_for(&ctx, 2, 0).center();
        let command = double_click(&ctx, first, &mut library, &mut transport, &mut queue);

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
        assert!(
            queue.can_next(),
            "the first of two identical entries was started, not the second"
        );
    }

    #[test]
    fn a_row_outside_the_viewport_takes_no_click() {
        let ctx = context();
        // Sixty rows at `extent::ROW_H` each put row forty far below the 400-point
        // probe window, so `ui.clip_rect()` cannot contain it and the view skips
        // laying it out. A shorter list would leave the clicked row on screen and
        // the test would prove nothing.
        let tracks: Vec<TrackEntry> = (0..60)
            .map(|i| track(&format!("t{i}"), &format!("t{i}")))
            .collect();
        let mut library = library_with(tracks);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let offscreen = row_rect_for(&ctx, 60, 40);
        assert!(
            offscreen.top() > 400.0,
            "the culled row must sit below the probe's viewport: {offscreen:?}"
        );
        let command = double_click(
            &ctx,
            offscreen.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );

        assert_eq!(command, None, "a culled row must not activate");
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
    fn the_total_is_withheld_when_any_length_is_unknown() {
        assert_eq!(total_duration_ms(&[]), 0, "no tracks is the absent total");
        assert_eq!(
            total_duration_ms(&[track_ms("a", 1_000), track_ms("b", 2_000)]),
            3_000
        );
        assert_eq!(
            total_duration_ms(&[track_ms("a", 1_000), track("b", "t1")]),
            0,
            "one unknown length withholds the whole total"
        );
    }

    #[test]
    fn the_total_saturates_instead_of_wrapping() {
        assert_eq!(
            total_duration_ms(&[track_ms("a", u32::MAX), track_ms("b", u32::MAX)]),
            u32::MAX,
            "two maximal lengths saturate rather than wrap"
        );
    }
}
