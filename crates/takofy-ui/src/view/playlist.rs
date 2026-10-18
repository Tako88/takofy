//! The main pane's tracklist: the selected playlist's tracks, or the state of
//! their load.
//!
//! The header and the rows are solved from one width and one set of tokens, so the
//! columns line up by construction rather than because two pieces agree. The header
//! scrolls with the list, which is what the reference's tracklist does.

use egui::emath::GuiRounding;
use egui::{FontId, Pos2, Rect, Sense, Ui, vec2};

use crate::icon;
use crate::layout::{Table, table as table_layout};
use crate::library::{Library, Load};
use crate::message::Command;
use crate::player::{Transport, TransportState};
use crate::queue::Queue;
use crate::theme::icon::Size;
use crate::theme::{Tokens, extent, space, stroke, text, tracking};

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
        head(ui, columns, tokens);

        let Load::Ready(tracks) = &library.tracks else {
            return;
        };
        let count = tracks.tracks.len();
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
            let row = Row {
                uri: &entry.uri,
                number: index + 1,
                title: &entry.title,
                artist: &entry.artists,
                // An album, a date added and a length are not in what the window is
                // handed yet, so those columns are drawn only when they are.
                album: None,
                added: None,
                duration: None,
                state: state_of(&entry.uri, transport),
            };
            if track_row::show(ui, rect, columns, &row, tokens) {
                command = queue.start(&tracks.tracks, index, transport);
            }
        }
    });

    command
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
    use crate::theme::{fonts, style};
    use takofy_model::TrackEntry;

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 400.0))),
            events,
            ..Default::default()
        }
    }

    /// Runs one pass and consumes the font-atlas delta it may have produced.
    fn run(ctx: &Context, input: RawInput, draw: impl FnMut(&mut Ui)) {
        let mut output = ctx.run_ui(input, draw);
        output.textures_delta.clear();
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

    /// One double click on `centre`, fed one event per pass, and the command the
    /// view returned while it happened.
    fn double_click(
        ctx: &Context,
        centre: Pos2,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
    ) -> Option<Command> {
        let tokens = Tokens::DARK;
        let mut command = None;
        // Warm up so hit-testing has a previous pass to read.
        run(ctx, input(Vec::new()), |ui| {
            let _ = tracks_view(ui, Table::Full, library, transport, queue, &tokens);
        });
        for _ in 0..2 {
            for event in [
                Event::PointerMoved(centre),
                button(centre, true),
                button(centre, false),
            ] {
                let command = &mut command;
                let library = &mut *library;
                let transport = &mut *transport;
                let queue = &mut *queue;
                run(ctx, input(vec![event]), |ui| {
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
        // The window's TrackEntry carries no album, date added or duration, so
        // every row it can produce is this shape. Solving its columns and
        // double-clicking must still lay the row out and activate it.
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
}
