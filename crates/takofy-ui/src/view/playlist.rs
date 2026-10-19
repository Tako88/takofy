//! The main pane's tracklist: the selected playlist's tracks, or the state of
//! their load.
//!
//! The header and the rows are solved from one width and one set of tokens, so the
//! columns line up by construction rather than because two pieces agree. The header
//! scrolls with the list, which is what the reference's tracklist does.

use std::time::Instant;

use egui::emath::GuiRounding;
use egui::{FontId, Pos2, Rect, Sense, Ui, vec2};
use takofy_model::{PlaylistEntry, TrackEntry};

use crate::cover::{Art, Cover};
use crate::format::{format_position, hero_meta};
use crate::icon;
use crate::layout::{Table, cards, table as table_layout};
use crate::library::{BandCover, Library, Load, band_cover};
use crate::message::{Command, Failure};
use crate::player::{Transport, TransportState};
use crate::queue::Queue;
use crate::theme::icon::Size;
use crate::theme::{Tokens, extent, space, stroke, text, tracking};

use super::card;
use super::hero;
use super::text as recipe;
use super::track_row::{self, Row, State};

/// The main pane: the selected playlist's tracks, or the state of their load.
#[expect(
    clippy::too_many_arguments,
    reason = "the pane, its table, the state it draws and the frame's clock"
)]
pub fn tracks_view(
    ui: &mut Ui,
    table: Table,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<Command> {
    ui.add_space(space::S3);

    if matches!(library.tracks, Load::Idle) {
        return idle(ui, library, art, now, tokens);
    }

    // The band draws `Loading` until the selected playlist's tracks settle: it
    // must not fetch a first-track cover it is about to replace with the
    // context image.
    let settled = !matches!(library.tracks, Load::Loading);
    let mut command = None;
    let mut retry = false;
    egui::ScrollArea::vertical().show(ui, |ui| {
        let width = ui.available_width();
        let columns = table_layout::columns(width, table.columns());

        // The band heads the playlist whenever the library can name the
        // selection; with no name there is nothing to say above the rows. It
        // takes the width and starts at the cursor, so the header and the rows
        // follow it down one scroll area.
        if let Some(entry) = library.selected_entry() {
            let cover = band_cover_for(entry, settled, art, now);
            let ready = match &library.tracks {
                Load::Ready(tracks) => Some(tracks),
                Load::Idle | Load::Loading | Load::Failed(_) => None,
            };
            let (meta, playable) = match ready {
                Some(tracks) if !tracks.tracks.is_empty() => (
                    hero_meta(tracks.tracks.len(), total_duration_ms(&tracks.tracks)),
                    true,
                ),
                _ => (Vec::new(), false),
            };
            let band = hero::Band {
                eyebrow: "Playlist",
                title: &entry.name,
                meta,
                playable,
                cover,
            };
            if hero::show(ui, &band, tokens)
                && let Some(tracks) = ready
            {
                command = queue.start(&tracks.tracks, 0, transport);
            }
        }

        match &library.tracks {
            Load::Idle => {}
            Load::Loading => {
                ui.spinner();
            }
            Load::Failed(failure) => {
                ui.label(recipe::muted(
                    crate::signin::failure_message(*failure),
                    tokens,
                ));
                if ui.button("Retry").clicked() {
                    retry = true;
                }
            }
            Load::Ready(tracks) if tracks.tracks.is_empty() => {
                ui.label(recipe::muted("No tracks in this playlist.", tokens));
            }
            Load::Ready(tracks) => {
                head(ui, columns, tokens);
                let count = tracks.tracks.len();
                // The whole list is reserved at once, so the scrollbar is right,
                // and only the rows the viewport can show are laid out and given
                // interaction: a thousand-track playlist paints the dozen rows on
                // screen.
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
                        // The added date is playlist membership rather than track
                        // metadata, so the window is not handed one yet.
                        added: None,
                        duration: duration.as_deref(),
                        state: state_of(&entry.uri, transport),
                        cover: art.look(entry.cover.as_ref(), now),
                    };
                    if track_row::show(ui, rect, columns, &row, tokens) {
                        command = queue.start(&tracks.tracks, index, transport);
                    }
                }
            }
        }
    });

    if retry {
        let playlist = library.selected().map(str::to_owned);
        return playlist.map(|playlist| library.select(&playlist));
    }
    command
}

/// The cover the band draws this frame.
///
/// Once the tracks have settled this is the entry's own cover. While they load
/// it is `Pending`, but a texture the store already holds is kept on screen
/// rather than flashing the placeholder: a reload that re-selected the same
/// playlist shows the picture it already had, and only an entry with nothing
/// held draws `Loading`.
fn band_cover_for(entry: &PlaylistEntry, settled: bool, art: &mut Art, now: Instant) -> Cover {
    match band_cover(entry, settled) {
        BandCover::Pending => entry
            .cover
            .as_ref()
            .and_then(|url| art.look_held(url, now))
            .unwrap_or(Cover::Loading),
        BandCover::Entry(url) => art.look(url, now),
    }
}

/// What the idle main view shows: the grid of playlists, or the state of the
/// load that fetches them.
///
/// The main view is idle when no playlist is selected — the state `deselect`
/// returns to. This reads that from the playlists' load alone: the tracks are
/// already `Idle` by the time the branch that calls it is taken, so the grid
/// never competes with the tracklist for a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleView<'a> {
    /// The playlists are in flight, or have not been asked for yet.
    Loading,
    /// The fetch failed; the failure is what the retry needs.
    Failed(Failure),
    /// The fetch succeeded and the library holds no playlists.
    Empty,
    /// The fetch succeeded and the library holds these playlists.
    Grid(&'a [PlaylistEntry]),
}

/// Reads the idle view from the playlists' load.
///
/// The two unstaged states are one `Loading`: a list nobody has asked for yet
/// and a list being fetched both have nothing to draw, and the reference gives
/// both the same treatment.
pub fn idle_view(library: &Library) -> IdleView<'_> {
    match &library.playlists {
        Load::Idle | Load::Loading => IdleView::Loading,
        Load::Failed(failure) => IdleView::Failed(*failure),
        Load::Ready(playlists) if playlists.is_empty() => IdleView::Empty,
        Load::Ready(playlists) => IdleView::Grid(playlists),
    }
}

/// Draws the idle main view and returns the command its controls asked for.
///
/// This is the whole idle-grid wiring in one place: the `Load::Idle` arm calls
/// it and nothing else does, so phase 4 can lift it out when it builds the
/// library route. The copy and the retry are the sidebar's, reusing
/// `library.reload()` so the two library surfaces recover the same way.
fn idle(
    ui: &mut Ui,
    library: &mut Library,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<Command> {
    match idle_view(library) {
        IdleView::Loading => {
            // The sidebar's loading copy, so both library surfaces say the same
            // thing while the list is fetched.
            ui.label(recipe::muted("Loading your playlists…", tokens));
            None
        }
        IdleView::Empty => {
            ui.label(recipe::muted("No playlists yet.", tokens));
            None
        }
        IdleView::Failed(failure) => {
            ui.label(recipe::muted(
                crate::signin::failure_message(failure),
                tokens,
            ));
            // The sidebar retries a failed playlist load with `reload`; the
            // grid offers the same recovery behind a control.
            if ui.button("Retry").clicked() {
                return Some(library.reload());
            }
            None
        }
        IdleView::Grid(entries) => {
            // The borrow ends with this copy, so the click below can select
            // through the library. The sidebar copies its list for the same
            // reason.
            let entries = entries.to_vec();
            grid(ui, &entries, library, art, now, tokens)
        }
    }
}

/// The grid of playlist cards that fills the idle main view.
///
/// Every value is a token or a measure the painter owns: the minimum card
/// width and the gap are tokens, and the height beyond the square art is what
/// `card::extra_height` reports. The grid loads nothing on its own — the one
/// click asks for tracks, through `library.select`.
fn grid(
    ui: &mut Ui,
    entries: &[PlaylistEntry],
    library: &mut Library,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<Command> {
    let mut picked: Option<String> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let solved = card_grid(ui);
            let list = grid_rect(ui, &solved, entries.len());
            // The whole grid is reserved at once, so the scrollbar is right, and
            // only the cards the viewport can show are laid out and given
            // interaction.
            ui.allocate_rect(list, Sense::hover());
            let visible = ui.clip_rect();

            for (index, entry) in entries.iter().enumerate() {
                let rect = solved.card_rect(index);
                if !visible.intersects(rect) {
                    continue;
                }
                let card = card::Card {
                    uri: &entry.uri,
                    name: &entry.name,
                    kind: "Playlist",
                    cover: art.look(entry.cover.as_ref(), now),
                };
                if card::show(ui, rect, &card, tokens) {
                    picked = Some(entry.uri.clone());
                }
            }
        });

    picked.and_then(|uri| select_playlist(library, &uri))
}

/// The solved card grid for the current width and the tokens the reference
/// gives a card.
///
/// The view and its tests both call this, so a card's place and a test's click
/// cannot drift apart.
fn card_grid(ui: &mut Ui) -> cards::Grid {
    let area = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 0.0));
    cards::grid(area, extent::CARD_MIN_W, space::S3, card::extra_height(ui))
}

/// The rect that holds every card: the width the grid solved and the rows'
/// total height, gaps included.
fn grid_rect(ui: &mut Ui, grid: &cards::Grid, count: usize) -> Rect {
    let height = if count == 0 {
        0.0
    } else {
        let rows = grid.rows(count);
        rows as f32 * grid.card_rect(0).height() + (rows - 1) as f32 * space::S3
    };
    Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), height))
}

/// Starts a playlist's load from an idle-grid card click.
///
/// `Library::select` always starts a load and returns its command, and that is
/// all this does. The grid is drawn only while the tracks are `Idle`, so the card
/// under a click can never be the playlist whose tracks are already loading;
/// there is nothing to suppress here.
///
/// `deselect()` is the one way the same playlist can be clicked again while its
/// first fetch is still coming back: it clears the selection and resets the
/// tracks to `Idle`, and the library keeps no record of the in-flight request, so
/// the second click does start it again. The first reply is discarded as stale,
/// so the repeat costs a fetch and nothing else.
fn select_playlist(library: &mut Library, uri: &str) -> Option<Command> {
    Some(library.select(uri))
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
    use egui::{
        Context, Event, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, RawInput,
        TouchPhase, Vec2,
    };

    use super::*;
    use crate::library::Library;
    use crate::message::Update;
    use crate::theme::hero::PLAY;
    use crate::theme::{fonts, style};
    use takofy_model::{CoverImage, CoverUrl, PlaylistEntry, TrackEntry};

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

    /// The row indices whose title the frame painted. A row that is laid out
    /// but clipped away paints nothing, so this is the row set the window shows.
    fn painted_rows(output: &egui::FullOutput) -> Vec<usize> {
        drawn_texts(output)
            .into_iter()
            .filter_map(|text| text.strip_prefix('t'))
            .filter_map(|tail| tail.parse::<usize>().ok())
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
            cover: None,
        }
    }

    fn library_with(tracks: Vec<TrackEntry>) -> Library {
        let mut library = Library::default();
        library.select("spotify:playlist:a");
        library.apply(&Update::Tracks {
            playlist: "spotify:playlist:a".to_owned(),
            tracks,
            cover: None,
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
            cover: None,
        }]));
        library
    }

    fn playlist(uri: &str, name: &str) -> PlaylistEntry {
        PlaylistEntry {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            cover: None,
        }
    }

    fn cover_url(tail: &str) -> CoverUrl {
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
    }

    fn playlist_with_cover(uri: &str, name: &str, cover: CoverUrl) -> PlaylistEntry {
        PlaylistEntry {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            cover: Some(cover),
        }
    }

    fn track_with_cover(index: usize) -> TrackEntry {
        TrackEntry {
            uri: format!("t{index}"),
            title: format!("t{index}"),
            artists: String::new(),
            album: None,
            duration_ms: None,
            named: true,
            cover: Some(cover_url(&format!("c{index}"))),
        }
    }

    fn image() -> CoverImage {
        CoverImage::new(1, 1, vec![0; 4]).expect("a 1x1 image")
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
        let mut found = None;
        run(ctx, input(Vec::new()), |ui| {
            ui.add_space(space::S3);
            egui::ScrollArea::vertical().show(ui, |ui| {
                let solved = card_grid(ui);
                found = Some(solved.card_rect(index));
            });
        });
        found.expect("the idle grid lays out")
    }

    /// The rect the idle grid draws the `index`-th card's play tile into at rest,
    /// solved through the card's own placement rather than a copied coordinate.
    /// The tile is drawn only once the card is revealed, so this is the rect a
    /// click finds after hovering; the reveal only raises the tile from here.
    fn card_play_rect_for(ctx: &Context, index: usize) -> Rect {
        card::play_rect(card_rect_for(ctx, index))
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
                    cover: Cover::Missing,
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
                    cover: Cover::Missing,
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
            let _ = tracks_view(
                ui,
                Table::Full,
                library,
                transport,
                queue,
                &mut Art::default(),
                Instant::now(),
                &tokens,
            );
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
                    if let Some(next) = tracks_view(
                        ui,
                        Table::Full,
                        library,
                        transport,
                        queue,
                        &mut Art::default(),
                        Instant::now(),
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

    /// One click at `centre` after resting the pointer there for `hover_frames`
    /// passes, and the command the view returned while the click happened.
    ///
    /// A card draws its play tile only once its reveal animation has run, and
    /// that animation advances because each [`Context::run_ui`] carries
    /// `input.time` forward by one `predicted_dt`. Pressing and releasing on the
    /// same pass would click an unrevealed card, so the pointer is left on the
    /// card until it is fully revealed first, which is what a real hover does.
    fn click_after_hover(
        screen: Vec2,
        ctx: &Context,
        centre: Pos2,
        hover_frames: usize,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
    ) -> Option<Command> {
        let tokens = Tokens::DARK;
        let mut command = None;
        // Warm up so hit-testing has a previous pass to read, pointer away.
        run(ctx, sized_input(screen, Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                library,
                transport,
                queue,
                &mut Art::default(),
                Instant::now(),
                &tokens,
            );
        });
        // Rest the pointer on the card until the reveal animation has finished.
        for _ in 0..hover_frames {
            run(
                ctx,
                sized_input(screen, vec![Event::PointerMoved(centre)]),
                |ui| {
                    let _ = tracks_view(
                        ui,
                        Table::Full,
                        library,
                        transport,
                        queue,
                        &mut Art::default(),
                        Instant::now(),
                        &tokens,
                    );
                },
            );
        }
        for event in [button(centre, true), button(centre, false)] {
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
                    &tokens,
                ) {
                    *command = Some(next);
                }
            });
        }
        command
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
            let _ = tracks_view(
                ui,
                Table::Full,
                library,
                transport,
                queue,
                &mut Art::default(),
                Instant::now(),
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
            cover: Cover::Missing,
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
            cover: Cover::Missing,
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
            cover: None,
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
                &mut Art::default(),
                Instant::now(),
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
                &mut Art::default(),
                Instant::now(),
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

    #[test]
    fn idle_view_reads_the_playlists_load() {
        // A list nobody has asked for yet and a list in flight are one state:
        // neither has anything to draw.
        assert_eq!(idle_view(&Library::default()), IdleView::Loading);

        let mut loading = Library::default();
        loading.apply(&Update::SignedIn);
        assert_eq!(idle_view(&loading), IdleView::Loading);

        let mut failed = Library::default();
        failed.apply(&Update::PlaylistFailed(Failure::Load));
        assert_eq!(idle_view(&failed), IdleView::Failed(Failure::Load));

        assert_eq!(idle_view(&library_idle(Vec::new())), IdleView::Empty);

        let ready = library_idle(vec![playlist("a", "A"), playlist("b", "B")]);
        let IdleView::Grid(entries) = idle_view(&ready) else {
            panic!("ready playlists are a grid: {:?}", idle_view(&ready));
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].uri, "a");
        assert_eq!(entries[1].uri, "b");
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
                &tokens,
            );
        });

        let mut loads = 0;
        for _ in 0..2 {
            for event in [
                Event::PointerMoved(centre),
                button(centre, true),
                button(centre, false),
            ] {
                run(&ctx, input(vec![event]), |ui| {
                    if let Some(Command::LoadTracks { .. }) = tracks_view(
                        ui,
                        Table::Full,
                        &mut library,
                        &mut transport,
                        &mut queue,
                        &mut Art::default(),
                        Instant::now(),
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
    fn a_click_on_a_cards_play_tile_does_not_load_the_playlist() {
        let ctx = context();
        let mut library = library_idle(vec![playlist("a", "A")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        // The click is the centre of the tile the card itself places, after the
        // pointer has rested on the card long enough for the reveal animation to
        // have drawn it.
        let tile = card_play_rect_for(&ctx, 0);
        let command = click_after_hover(
            PROBE,
            &ctx,
            tile.center(),
            16,
            &mut library,
            &mut transport,
            &mut queue,
        );

        assert_eq!(
            command, None,
            "a click on the waiting play tile must not fall through to the card"
        );
        assert_eq!(library.selected(), None, "the card must not be selected");
        assert_eq!(library.tracks, Load::Idle, "the playlist must not load");
    }

    #[test]
    fn the_card_height_the_grid_solves_contains_the_last_line_the_card_paints() {
        let ctx = context();
        let tokens = Tokens::DARK;
        // A realistic card: the grid's own solve at the probe's width, so the
        // rect's height is `card_w + card::extra_height`, the number the grid is
        // given.
        let rect = card_rect_for(&ctx, 0);
        let card = card::Card {
            uri: "a",
            name: "Evening",
            kind: "Playlist",
            cover: Cover::Missing,
        };
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = card::show(ui, rect, &card, &tokens);
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
    fn deselect_returns_to_the_grid() {
        let mut library = library_idle(vec![playlist("a", "A")]);
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t", "t")],
            cover: None,
        });
        library.deselect();

        let IdleView::Grid(entries) = idle_view(&library) else {
            panic!(
                "deselect must return to the grid: {:?}",
                idle_view(&library)
            );
        };
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].uri, "a");
    }

    #[test]
    fn the_frame_loop_requests_only_visible_covers() {
        let ctx = context();
        let tokens = Tokens::DARK;
        // A thousand distinct covers in a short window: only the dozen rows the
        // viewport can show are ever laid out, even as the list is scrolled.
        let tracks: Vec<TrackEntry> = (0..1_000).map(track_with_cover).collect();
        let mut library = library_with(tracks);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let now = Instant::now();
        let mut requested: std::collections::HashMap<CoverUrl, usize> =
            std::collections::HashMap::new();
        let mut painted: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let centre = Pos2::new(PROBE.x / 2.0, PROBE.y / 2.0);

        // Scroll from the top down. Every requested cover must belong to a row
        // the window painted in some frame, and the outstanding set must never
        // exceed the cap.
        for step in 0..240 {
            art.begin_frame();
            let mut events = vec![Event::PointerMoved(centre)];
            if step > 0 {
                events.push(Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    delta: vec2(0.0, -extent::ROW_H),
                    phase: TouchPhase::Move,
                    source: MouseWheelSource::Unknown,
                    modifiers: Modifiers::NONE,
                });
            }
            let mut output = ctx.run_ui(input(events), |ui| {
                let _ = tracks_view(
                    ui,
                    Table::Full,
                    &mut library,
                    &mut transport,
                    &mut queue,
                    &mut art,
                    now,
                    &tokens,
                );
            });
            output.textures_delta.clear();
            painted.extend(painted_rows(&output));

            let commands = art.requests(now);
            assert!(
                art.outstanding() <= crate::cover::MAX_IN_FLIGHT,
                "{} fetches outstanding",
                art.outstanding()
            );
            for command in commands {
                if let Command::Fetch { url } = command {
                    requested.insert(url.clone(), cover_row(&url));
                    // Answer it, so the next frame can ask for the next row.
                    art.apply(
                        &Update::Cover {
                            url,
                            image: image(),
                        },
                        now,
                    );
                }
            }
        }

        assert!(!requested.is_empty(), "the visible rows must be fetched");
        assert!(
            painted.iter().copied().max().unwrap_or(0) > 50,
            "the frame loop must scroll through the list to exercise the cap"
        );
        for (url, row) in &requested {
            assert!(
                painted.contains(row),
                "{url:?} was requested for row {row}, which no frame ever painted"
            );
        }
    }

    #[test]
    fn a_grid_card_switches_to_the_context_image_when_it_arrives() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let first = cover_url("first");
        let context_cover = cover_url("context");
        let mut library = library_idle(vec![playlist_with_cover("a", "A", first.clone())]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let now = Instant::now();

        art.begin_frame();
        run(&ctx, input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
                now,
                &tokens,
            );
        });
        assert_eq!(fetch_urls(art.requests(now)), vec![first.clone()]);

        // A context image for an unselected playlist still upgrades its entry,
        // because the upgrade is keyed by the update's own playlist.
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: Vec::new(),
            cover: Some(context_cover.clone()),
        });

        art.begin_frame();
        run(&ctx, input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
                now,
                &tokens,
            );
        });
        assert_eq!(fetch_urls(art.requests(now)), vec![context_cover.clone()]);
    }

    #[test]
    fn the_band_waits_for_the_context_image_and_then_shows_it() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let first = cover_url("first");
        let context_cover = cover_url("context");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist_with_cover(
            "a",
            "Evening",
            first.clone(),
        )]));
        library.select("a");
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let now = Instant::now();

        // While the tracks load, the band is drawn waiting and asks for nothing:
        // it must not fetch the first-track cover it is about to replace.
        art.begin_frame();
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
                now,
                &tokens,
            );
        });
        output.textures_delta.clear();
        assert!(
            drawn_texts(&output).contains(&"PLAYLIST"),
            "the band must head a loading playlist"
        );
        assert!(art.requests(now).is_empty(), "no cover yet");

        // The first page lands with the context image: the entry is upgraded and
        // the band wants it on the same frame.
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t", "t")],
            cover: Some(context_cover.clone()),
        });
        art.begin_frame();
        run(&ctx, input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
                now,
                &tokens,
            );
        });
        assert_eq!(fetch_urls(art.requests(now)), vec![context_cover.clone()]);
    }

    #[test]
    fn the_band_keeps_an_already_held_cover_while_the_tracks_reload() {
        let ctx = context();
        let cover = cover_url("held");
        let entry = playlist_with_cover("a", "Evening", cover.clone());
        let now = Instant::now();
        let mut art = Art::default();

        // An earlier visit to the playlist fetched and held its cover.
        art.begin_frame();
        assert_eq!(art.look(Some(&cover), now), Cover::Loading);
        assert_eq!(fetch_urls(art.requests(now)), vec![cover.clone()]);
        art.apply(
            &Update::Cover {
                url: cover.clone(),
                image: image(),
            },
            now,
        );
        art.upload(&ctx);
        assert!(matches!(art.look(Some(&cover), now), Cover::Ready(_)));

        // Re-selecting the playlist puts the tracks back into `Loading`. The
        // band still draws the picture it already holds rather than flashing the
        // placeholder, and does not fetch it again.
        art.begin_frame();
        assert!(
            matches!(
                band_cover_for(&entry, false, &mut art, now),
                Cover::Ready(_)
            ),
            "the band keeps the cover it already holds"
        );
        assert!(
            fetch_urls(art.requests(now)).is_empty(),
            "a held cover is not fetched again"
        );

        // A playlist whose cover is not held still waits without fetching it.
        let other = playlist_with_cover("b", "Other", cover_url("unheld"));
        art.begin_frame();
        assert_eq!(band_cover_for(&other, false, &mut art, now), Cover::Loading);
        assert!(fetch_urls(art.requests(now)).is_empty());
    }
}
