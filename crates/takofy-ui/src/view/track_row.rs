//! One track row.
//!
//! The row is handed its data and the rects the table solved for it; it fetches
//! nothing and decides no width. What it decides is what a state looks like: the
//! number gives way to a play glyph under the pointer, the equaliser stands in
//! while this is the transport's track, and the like appears on hover or focus.
//!
//! Its body is the click target and the drag source both. What a press or a lift
//! means to the list's selection belongs to the list, so the row only reports it,
//! and a carried row is drawn from its own [`Lifted`] snapshot.
//!
//! The row is up to three focus stops: its body, the like once a read has made
//! it live, and the `⋯` that opens its menu. The like is skipped while its state
//! is unknown and while a block is carried, so a list of sixty rows is a hundred
//! and twenty stops in the common case and a hundred and eighty only once every
//! heart is live. The album and artist cells are links that take a single click
//! anywhere on the cell, but their interaction is deliberately not focusable, so
//! they add no stop of their own: the cost is that a keyboard can reach neither
//! link yet, while Enter and Space on the row still play.
//!
//! The row's menu has three doors — the `⋯`, a right-click anywhere on the row,
//! and the context-menu key or Shift+F10 while the row or its `⋯` holds the
//! keyboard — and all three report one `Click::Menu`, anchored to the row's own
//! `⋯`. The row never opens a menu itself: the page's list turns the click into a
//! request and the shell opens it.
//!
//! The like appears on hover, on focus within the row, and whenever the track is
//! liked: a liked heart is the row's one piece of state that must be readable at
//! rest. It reports the opposite of the state it drew and never touches the
//! pick, because a heart acts on its own row and the menu's save row acts on the
//! pick.

use egui::emath::GuiRounding;
use egui::{Color32, FontId, Pos2, Rect, Sense, Ui, vec2};

use crate::availability::Control;
use crate::cover::Cover;
use crate::icon::{self, Icon};
use crate::layout::table::{Columns, number_box, title_parts};
use crate::menu::{Anchor, Device, Door};
use crate::player::{Transport, TransportState};
use crate::route::Page;
use crate::selection::Press;
use crate::theme::icon::Size;
use crate::theme::{Tokens, ease, eq, motion, radius, space, stroke, text, tracking};

use super::rows::{self, Ghosting, Look};
use super::{art, context_menu, controls, text as recipe};

/// A row's relationship to the transport.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum State {
    /// The transport is somewhere else.
    #[default]
    Resting,
    /// This is the transport's track, and it is moving.
    Playing,
    /// This is the transport's track, and it is held.
    Paused,
    /// This is the transport's track, but nothing is sounding: it ended, it
    /// failed, or it never started. The marker stays, because this is still
    /// where the transport is; the equaliser does not, because nothing is
    /// playing and a frozen one would read as a pause.
    Stopped,
}

impl State {
    /// Whether the transport is on this row at all.
    fn current(self) -> bool {
        !matches!(self, Self::Resting)
    }

    /// Whether the row is the transport's track and sounding: playing or held.
    /// This is what the equaliser stands for.
    fn sounding(self) -> bool {
        matches!(self, Self::Playing | Self::Paused)
    }
}

/// How a row relates to the transport.
///
/// The row never reaches for the transport itself: the view reads it once per
/// row and hands the row the answer. The held/pausable half is
/// [`Transport::hold`]'s, so the glyph and the action it stands for cannot
/// disagree.
pub fn state_of(uri: &str, transport: &Transport) -> State {
    let hold = transport.hold(uri);
    match (hold.loaded, hold.held, transport.state) {
        (false, _, _) => State::Resting,
        // Loading is a track on its way, so its bars move: it is not a pause.
        (true, true, TransportState::Paused) => State::Paused,
        (true, true, _) => State::Playing,
        // Loaded, but a move would restart it rather than pause it.
        (true, false, _) => State::Stopped,
    }
}

/// How a row draws its title cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TitleCell {
    /// The cover's box, then the name on one line: a playlist's row.
    ArtThenName,
    /// The name over the artist's name, with no art: an album's row.
    ///
    /// The artist line is not a link in this slice: it is the row's subline, not
    /// the artist column the playlist carries.
    NameOverArtist,
}

/// What one row draws, as text and flags.
///
/// The view assembles it from the track, the library and the transport. The row
/// never reaches back for any of them.
pub struct Row<'a> {
    /// The track's uri. It keys the row and the controls inside it.
    pub uri: &'a str,
    /// The number in the index column: the position in the list, from one.
    pub number: usize,
    pub title: &'a str,
    /// How the title cell is drawn at this table's breakpoint.
    pub title_cell: TitleCell,
    pub artist: &'a str,
    /// The first artist's page, when labelling resolved one. The cell links only
    /// when this is set and `artist` is non-empty.
    pub artist_uri: Option<&'a str>,
    pub album: Option<&'a str>,
    /// The album's page, when labelling resolved one. The cell links only when
    /// this is set and `album` is non-empty.
    pub album_uri: Option<&'a str>,
    pub added: Option<&'a str>,
    pub duration: Option<&'a str>,
    pub state: State,
    /// The track's cover, as the art pipeline resolves it.
    pub cover: Cover,
    /// Where the row stands in its list: picked, carried, or neither.
    pub look: Look,
    /// Whether this row's menu may open. The page decides, because the gate is
    /// about the list and the moment, not about the row.
    pub menu: bool,
    /// Whether the track is liked, or `None` while the set cannot say.
    pub liked: Option<bool>,
}

impl Row<'_> {
    /// The row as it is drawn in flight: owned, and free of interaction.
    pub fn lifted(&self, columns: Columns) -> Lifted {
        Lifted {
            columns,
            cell: self.title_cell,
            number: self.number,
            title: self.title.to_owned(),
            artist: self.artist.to_owned(),
            album: self.album.map(str::to_owned),
            added: self.added.map(str::to_owned),
            duration: self.duration.map(str::to_owned),
            cover: self.cover,
        }
    }
}

/// One row's own paint, kept so a carried row can be drawn away from its list.
#[derive(Clone, Debug, PartialEq)]
pub struct Lifted {
    columns: Columns,
    cell: TitleCell,
    number: usize,
    title: String,
    artist: String,
    album: Option<String>,
    added: Option<String>,
    duration: Option<String>,
    cover: Cover,
}

impl Lifted {
    /// Draws the row into `rect` with no interaction of its own: a ghost is
    /// painted over whatever is under the pointer, and must take nothing from it.
    pub fn paint(&self, ui: &Ui, rect: Rect, ghosting: Ghosting<'_>, tokens: &Tokens) {
        let columns = self.columns.at(rect.min);
        let title_ink = ink(
            tokens.palette.text,
            tokens.palette.text_subtle,
            ghosting.flat,
        );
        let secondary_ink = ink(
            tokens.palette.text_muted,
            tokens.palette.text_subtle,
            ghosting.flat,
        );

        let number = number_box(columns.index);
        if drawable(number) {
            centred_line(
                ui,
                number,
                &self.number.to_string(),
                recipe::track_mono(text::XXS),
                tokens.palette.text_subtle,
            );
        }

        match self.cell {
            TitleCell::ArtThenName => {
                let parts = title_parts(columns.title);
                if drawable(parts.art) {
                    art::tile(
                        ui.painter(),
                        parts.art,
                        radius::XS,
                        tokens.palette.border,
                        self.cover,
                        tokens,
                    );
                }
                if drawable(parts.text) {
                    let top = vertically_centred(ui, parts.text, &recipe::track_name());
                    self.paint_title(ui, parts.text, top, ghosting, title_ink, tokens);
                }
            }
            TitleCell::NameOverArtist => {
                let artist_font = recipe::track_secondary();
                let artist_h = if self.artist.is_empty() {
                    0.0
                } else {
                    line_height(ui, &artist_font)
                };
                let name_h = line_height(ui, &recipe::track_name());
                let (name_top, artist_top) = stacked_tops(columns.title, name_h, artist_h);
                self.paint_title(ui, columns.title, name_top, ghosting, title_ink, tokens);
                if !self.artist.is_empty() && drawable(columns.title) {
                    left_at(
                        ui,
                        columns.title,
                        artist_top,
                        &self.artist,
                        artist_font,
                        secondary_ink,
                    );
                }
            }
        }

        if !self.artist.is_empty() && drawable(columns.artist) {
            left(
                ui,
                columns.artist,
                &self.artist,
                recipe::track_secondary(),
                secondary_ink,
            );
        }
        if drawable(columns.album)
            && let Some(album) = self.album.as_deref().filter(|album| !album.is_empty())
        {
            left(
                ui,
                columns.album,
                album,
                recipe::track_secondary(),
                secondary_ink,
            );
        }
        if drawable(columns.added)
            && let Some(added) = self.added.as_deref()
        {
            left(
                ui,
                columns.added,
                added,
                recipe::track_mono(text::XXS),
                tokens.palette.text_subtle,
            );
        }
        // A block carries its running time where a single row carries its length,
        // and a block whose total is unknown carries none of its rows' lengths.
        let duration = if ghosting.count > 1 {
            ghosting.total
        } else {
            ghosting.total.or(self.duration.as_deref())
        };
        if drawable(columns.duration)
            && let Some(duration) = duration
        {
            right(
                ui,
                columns.duration,
                duration,
                recipe::track_mono(text::XS),
                tokens.palette.text_subtle,
            );
        }
    }

    /// Paints the title line, and the count a block stands for beside it.
    fn paint_title(
        &self,
        ui: &Ui,
        rect: Rect,
        top: f32,
        ghosting: Ghosting<'_>,
        title_ink: Color32,
        tokens: &Tokens,
    ) {
        if !drawable(rect) {
            return;
        }
        let count_ink = ink(
            tokens.palette.text_muted,
            tokens.palette.text_subtle,
            ghosting.flat,
        );
        let mut width = rect.width();
        let count = (ghosting.count > 1).then(|| {
            let galley = recipe::spaced(
                ui,
                &format!("×{}", ghosting.count),
                recipe::track_mono(text::XXS),
                count_ink,
                recipe::tracking_pts(text::XXS, tracking::WIDE),
            );
            // The count takes its room from the title rather than from the
            // column, so the two never overlap.
            width = (rect.width() - galley.size().x - space::S2).max(0.0);
            galley
        });
        let galley = recipe::line(ui, &self.title, recipe::track_name(), title_ink, 0.0, width);
        ui.painter()
            .galley(Pos2::new(rect.left(), top), galley.clone(), title_ink);
        if let Some(count) = count {
            let pos = Pos2::new(rect.left() + galley.size().x + space::S2, top);
            ui.painter().galley(pos, count, count_ink);
        }
    }
}

/// What a click on a row asked for.
///
/// No `Eq`: a door carries its anchor, and an anchor is a rect of floats.
#[derive(Clone, PartialEq, Debug)]
pub enum Click {
    /// Start the track.
    Play,
    /// Open the page its album or first artist cell links to.
    Open(Page),
    /// Pick the row, which is what a single click on its body does.
    Select(Press),
    /// The pointer began dragging the row out of the list.
    Lift,
    /// Open this row's menu, from whichever door asked for it.
    Menu(Door),
    /// Flip this row's heart to the given state.
    Like(bool),
}

/// What a link cell's interaction reports to the row.
///
/// A cell that is not linkable paints its label without an interaction, so it
/// reports nothing rather than an empty hit area.
struct Link {
    /// Whether the pointer is resting on the cell.
    hovered: bool,
    /// The page a click on the cell opens.
    page: Option<Page>,
}

/// The widget id of the body of the row at `number` in `uri`'s list.
///
/// The position is part of the key, because a playlist may hold the same track
/// twice: keyed by the track alone, two rows would share one focus node. The
/// number is one-based, as the row is drawn.
pub(super) fn id(ui: &Ui, uri: &str, number: usize) -> egui::Id {
    ui.scope_id().with(("track-row", (uri, number)))
}

/// Draws one row into `rect`, and reports what a click on it asked for.
///
/// `rect` is `layout::table::row_rect` for this row and `columns` is what
/// `layout::table::columns` solved for the same width, so a header and its rows
/// can never disagree about where a column is.
pub fn show(
    ui: &mut Ui,
    rect: Rect,
    columns: Columns,
    row: &Row<'_>,
    tokens: &Tokens,
) -> Option<Click> {
    // The table solves its rects row-local; this is where they reach the screen.
    let columns = columns.at(rect.min);
    // The position is part of the key, because a playlist may hold the same track
    // twice: keyed by the track alone, two rows would share one hover, one focus
    // node and one click, and the wrong occurrence would play.
    let key = (row.uri, row.number);
    let body = rows::body(ui, rect, id(ui, row.uri, row.number), row.look);
    let response = &body.response;
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!("{} by {}", row.title, row.artist),
        )
    });

    // The reference reveals the play glyph and the like on hover *and* on
    // focus-within, because a keyboard has no pointer.
    //
    // `hovered()` is ownership, not position: the album and artist cells are
    // interacted after this row, and egui marks only the top hit widget as
    // hovered, so the row loses the hover the moment the pointer enters a link
    // cell. `contains_pointer()` asks whether the pointer is in the row's rect
    // at all, which is what the highlight, the play glyph and the like follow. A
    // carried row reveals nothing: the pointer is holding a block, not reaching
    // for a control.
    let hovered = (response.hovered() || response.contains_pointer()) && !row.look.carrying;
    // A lift hands the row the focus, so the carried row has to shed that too.
    // The heart and the `⋯` are interacted after the body, so their focus is read
    // before they draw: a focused heart would otherwise be hidden the frame after
    // it took the keyboard, and the focus would fall off the row.
    let focused = ui.memory(|memory| memory.focused());
    let focus_within = focused.is_some_and(|id| {
        id == controls::slot_id(ui, Control::TrackLike, key)
            || id == controls::slot_id(ui, Control::TrackMore, key)
    });
    let revealed = (hovered || response.has_focus() || focus_within) && !row.look.carrying;
    let hover = rows::fade(
        ui,
        ui.scope_id().with(("track-row-hover", key)),
        revealed,
        row.look,
    );

    let before = rows::dim(ui, row.look);
    rows::wash(ui, rect, hover, row.look, tokens);
    // A ring on a carried row is noise: the pointer already holds it.
    if response.has_focus() && !row.look.carrying {
        controls::focus_ring(ui, rect, tokens);
    }

    // The sounding row is marked at the margin. Its title stays bone: a green name
    // would read as a link rather than as "this is the one playing".
    if row.state.current() {
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, vec2(stroke::RULE, rect.height())),
            radius::XS,
            tokens.palette.accent,
        );
    }

    index(ui, number_box(columns.index), row, revealed, tokens);
    let title_link = title(ui, columns.title, row, key, tokens);
    let artist_link = artist(ui, columns.artist, row, key, tokens);
    let album_link = album(ui, columns.album, row, key, tokens);
    added(ui, columns.added, row.added, tokens, row.look.selected);
    let on_like = like(ui, columns.like, key, revealed, row, tokens);
    duration(
        ui,
        columns.duration,
        row.duration,
        tokens,
        row.look.selected,
    );
    let more = more(ui, columns.actions, key, row, response.id, tokens);
    let on_more = more.hovered;

    // A control inside the row takes the click it is given rather than passing it
    // on: a click on one is not a click on the row.
    let on_control = on_like.hovered || on_more;
    let keyboard = response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));

    // The link cells are interacted after the row, so on a hit-test tie the cell
    // wins the click and the row never sees the double click that would play. A
    // click on either cell is a navigation, and takes precedence over playback.
    let mut link_hovered = false;
    let mut open = None;
    for link in [title_link, artist_link, album_link].into_iter().flatten() {
        link_hovered |= link.hovered;
        open = open.or(link.page);
    }

    ui.set_opacity(before);

    // The three doors, before anything the row itself does: a menu opens on the
    // row the user pointed at, and never also selects or plays it.
    if row.menu
        && !row.look.carrying
        && let Some(door) = more
            .door
            .or_else(|| key_door(ui, response, &more, columns.actions))
            .or_else(|| pointer_door(ui, rect, response.id, columns.actions))
    {
        return Some(Click::Menu(door));
    }

    // A heart click is the heart's own, and never the row's: it acts on one row
    // and reports the opposite of the state it drew.
    if let Some(want) = on_like.toggled {
        return Some(Click::Like(want));
    }

    // The row's own keyboard activation is answered before the link cells are
    // read, so a link click landing in the same frame can never swallow it.
    // (egui surrenders the row's focus on the frame a link's click is reported,
    // so the two cannot in fact both be true today; reading the keyboard first
    // keeps the play whatever the hit test does.)
    if keyboard && !on_control {
        return Some(Click::Play);
    }
    if let Some(page) = open {
        return Some(Click::Open(page));
    }
    // `link_hovered` is a belt-and-braces guard, not the mechanism: when a link
    // wins the hit test, the row's `double_clicked()` is already false because
    // egui tells only the hit widget, so the second click of a double click on a
    // link rides on the first click's navigation, which has already re-laid out
    // the page.
    if response.double_clicked() && !link_hovered && !on_control {
        return Some(Click::Play);
    }
    if body.lift {
        return Some(Click::Lift);
    }
    if link_hovered || on_control {
        return None;
    }
    body.press.map(Click::Select)
}

/// The tops of a stacked title's two lines, centred together in `rect`.
///
/// Pure, so the stacking is pinned without a window: the pair is treated as one
/// block and centred as a whole, which is what makes a short name over a long
/// artist read as a single cell rather than as two loose lines. A zero or
/// non-finite height takes no room, so the result is always finite.
pub fn stacked_tops(rect: Rect, name_h: f32, artist_h: f32) -> (f32, f32) {
    let name_h = room(name_h);
    let artist_h = room(artist_h);
    let top = rect.center().y - (name_h + artist_h) / 2.0;
    (top, top + name_h)
}

/// A height that takes room: anything zero, negative or non-finite takes none.
fn room(height: f32) -> f32 {
    if height.is_finite() && height > 0.0 {
        height
    } else {
        0.0
    }
}

/// The index column: the number, or the play glyph under the pointer, or the
/// equaliser while this is the transport's track.
fn index(ui: &Ui, rect: Rect, row: &Row<'_>, revealed: bool, tokens: &Tokens) {
    if !drawable(rect) {
        return;
    }
    if revealed {
        icon::paint(
            ui.painter(),
            Icon::Play,
            centred(ui, rect, Size::SM.side),
            Size::SM,
            tokens.palette.text,
        );
    } else if row.state.sounding() {
        equaliser(ui, rect, row.state, tokens);
    } else {
        // A picked row lifts its number and its figures, so the row reads as
        // chosen rather than as one the pointer happens to be over.
        centred_line(
            ui,
            rect,
            &row.number.to_string(),
            recipe::track_mono(text::XXS),
            ink(
                tokens.palette.text_subtle,
                tokens.palette.text,
                row.look.selected,
            ),
        );
    }
}

/// The equaliser: three bars, each a little behind the one before it.
pub(super) fn equaliser(ui: &Ui, rect: Rect, state: State, tokens: &Tokens) {
    let bar_w = eq::BAR_W;
    let bars = eq::BARS as f32;
    let width = bars * bar_w + (bars - 1.0) * eq::GAP;
    let left = rect.center().x - width / 2.0;
    let bottom = rect.center().y + eq::H / 2.0;

    // A held track keeps its bars and stops moving them. The reference freezes its
    // animation where it stood; pinning them at full height is this app's own
    // simplification, because the alternative is remembering when the pause began.
    let scales = match state {
        State::Paused => [1.0; eq::BARS],
        _ => {
            // The pulse is a moving picture, and the window's steady redraw is
            // nowhere near fast enough for it.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f32(eq::FRAME));
            bar_scales(ui.input(|input| input.time) as f32)
        }
    };

    for (bar, scale) in scales.into_iter().enumerate() {
        let x = left + bar as f32 * (bar_w + eq::GAP);
        let height = eq::H * scale;
        ui.painter().rect_filled(
            Rect::from_min_size(Pos2::new(x, bottom - height), vec2(bar_w, height)),
            0.0,
            tokens.palette.accent,
        );
    }
}

/// The height of each bar as a fraction of [`eq::H`], `time` seconds in.
///
/// One pulse runs from a quarter height up to full and back, and each bar starts
/// [`eq::DELAY`] after the one before it. Pure, so the movement is pinned without
/// a window.
fn bar_scales(time: f32) -> [f32; eq::BARS] {
    std::array::from_fn(|bar| {
        let phase = time / eq::PULSE - bar as f32 * eq::DELAY / eq::PULSE;
        let cycles = phase - phase.floor();
        let wave = (1.0 - (cycles * std::f32::consts::TAU).cos()) / 2.0;
        eq::MIN_SCALE + (1.0 - eq::MIN_SCALE) * wave
    })
}

/// The title column.
///
/// A playlist's row nests the cover's box inside the title and draws the name
/// beside it. An album's row has no per-row art — the band already holds the
/// record's one picture — so the whole cell is the two lines of text.
///
/// Only the stacked form's artist line can be a link, and it reports its
/// interaction so a click on it opens the artist rather than playing the row.
fn title(
    ui: &mut Ui,
    rect: Rect,
    row: &Row<'_>,
    key: (&str, usize),
    tokens: &Tokens,
) -> Option<Link> {
    if !drawable(rect) {
        return None;
    }
    match row.title_cell {
        TitleCell::ArtThenName => {
            art_then_name(ui, rect, row, tokens);
            None
        }
        TitleCell::NameOverArtist => name_over_artist(ui, rect, row, key, tokens),
    }
}

/// The playlist's title cell: the cover's box, then the name.
fn art_then_name(ui: &Ui, rect: Rect, row: &Row<'_>, tokens: &Tokens) {
    let parts = title_parts(rect);
    art::tile(
        ui.painter(),
        parts.art,
        radius::XS,
        tokens.palette.border,
        row.cover,
        tokens,
    );

    if drawable(parts.text) {
        left(
            ui,
            parts.text,
            row.title,
            recipe::track_name(),
            tokens.palette.text,
        );
    }
}

/// The album's title cell: the name over its artist, centred together.
///
/// The artist line is a link when the row resolved an artist URI, and only the
/// painted words take the click: the row's own double click still plays from the
/// name line, which is where the cell's centre sits.
fn name_over_artist(
    ui: &mut Ui,
    rect: Rect,
    row: &Row<'_>,
    key: (&str, usize),
    tokens: &Tokens,
) -> Option<Link> {
    let name_font = recipe::track_name();
    let artist_font = recipe::track_secondary();
    let name_h = line_height(ui, &name_font);
    let artist_h = if row.artist.is_empty() {
        0.0
    } else {
        line_height(ui, &artist_font)
    };
    let (name_top, artist_top) = stacked_tops(rect, name_h, artist_h);

    left_at(
        ui,
        rect,
        name_top,
        row.title,
        name_font,
        tokens.palette.text,
    );
    if row.artist.is_empty() {
        return None;
    }
    let Some(uri) = row.artist_uri else {
        left_at(
            ui,
            rect,
            artist_top,
            row.artist,
            artist_font,
            tokens.palette.text_muted,
        );
        return None;
    };

    // The link is the painted line, not the whole cell: a click below the name
    // must stay the row's, and the row's centre is only a hair above this band.
    let measured = recipe::line(
        ui,
        row.artist,
        artist_font.clone(),
        tokens.palette.text_muted,
        0.0,
        rect.width(),
    );
    let hit = Rect::from_min_size(
        Pos2::new(rect.left(), artist_top),
        vec2(measured.size().x, measured.size().y),
    );
    let response = ui.interact(
        hit,
        ui.scope_id().with(("track-artist-subline", key)),
        Sense::CLICK,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Link,
            true,
            format!("Open artist {}", row.artist),
        )
    });
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let reveal = ui.ctx().animate_bool_with_time_and_easing(
        ui.scope_id().with(("track-artist-subline", "hover", key)),
        response.hovered(),
        motion::FAST,
        ease::out,
    );
    left_at(
        ui,
        rect,
        artist_top,
        row.artist,
        artist_font,
        tokens
            .palette
            .text_muted
            .lerp_to_gamma(tokens.palette.text, reveal),
    );
    Some(Link {
        hovered: response.hovered(),
        page: response.clicked().then(|| Page::credit(uri)),
    })
}

/// The artist column, which only a playlist carries.
///
/// The cell is a link when its first artist resolved a page and its name is
/// non-empty: an empty cell painted nothing, so it gets no hit area either.
fn artist(
    ui: &mut Ui,
    rect: Rect,
    row: &Row<'_>,
    key: (&str, usize),
    tokens: &Tokens,
) -> Option<Link> {
    if !drawable(rect) || row.artist.is_empty() {
        return None;
    }
    let Some(uri) = row.artist_uri else {
        left(
            ui,
            rect,
            row.artist,
            recipe::track_secondary(),
            tokens.palette.text_muted,
        );
        return None;
    };
    let response = link_cell(ui, rect, "track-artist", row.artist, "artist", key, tokens);
    Some(Link {
        hovered: response.hovered(),
        page: response.clicked().then(|| Page::credit(uri)),
    })
}

/// The album column, which an album's own tracklist does not carry.
///
/// The cell is a link under the same rule as [`artist`]'s.
fn album(
    ui: &mut Ui,
    rect: Rect,
    row: &Row<'_>,
    key: (&str, usize),
    tokens: &Tokens,
) -> Option<Link> {
    let label = row.album?;
    if !drawable(rect) || label.is_empty() {
        return None;
    }
    let Some(uri) = row.album_uri else {
        left(
            ui,
            rect,
            label,
            recipe::track_secondary(),
            tokens.palette.text_muted,
        );
        return None;
    };
    let response = link_cell(ui, rect, "track-album", label, "album", key, tokens);
    Some(Link {
        hovered: response.hovered(),
        page: response.clicked().then(|| Page::Album(uri.to_owned())),
    })
}

/// Paints a link cell's label and returns its interaction.
///
/// The hit area is the whole cell, padding included, not only the painted text.
/// The interaction is created before the label is painted so the painter can
/// read its hover, and after the row's own so the cell takes the hit on a tie.
/// The sense is `Sense::CLICK` rather than `Sense::click()`: a link must not add
/// a focus stop to a sixty-row list.
fn link_cell(
    ui: &mut Ui,
    rect: Rect,
    id: &'static str,
    label: &str,
    kind: &str,
    key: (&str, usize),
    tokens: &Tokens,
) -> egui::Response {
    let response = ui.interact(rect, ui.scope_id().with((id, key)), Sense::CLICK);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Link, true, format!("Open {kind} {label}"))
    });
    let hovered = response.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let reveal = ui.ctx().animate_bool_with_time_and_easing(
        ui.scope_id().with((id, "hover", key)),
        hovered,
        motion::FAST,
        ease::out,
    );
    left(
        ui,
        rect,
        label,
        recipe::track_secondary(),
        tokens
            .palette
            .text_muted
            .lerp_to_gamma(tokens.palette.text, reveal),
    );
    response
}

/// The date the track was added, which only a playlist carries.
fn added(ui: &Ui, rect: Rect, label: Option<&str>, tokens: &Tokens, selected: bool) {
    if !drawable(rect) {
        return;
    }
    if let Some(label) = label {
        left(
            ui,
            rect,
            label,
            recipe::track_mono(text::XXS),
            ink(
                tokens.palette.text_subtle,
                tokens.palette.text_muted,
                selected,
            ),
        );
    }
}

/// What the like control reported.
#[derive(Clone, Copy, Default)]
struct Heart {
    /// Whether the pointer is on it, so a click here is not a click on the row.
    hovered: bool,
    /// The state the click asks for, or `None` when it asked for nothing.
    toggled: Option<bool>,
}

/// The like, which appears under the pointer or on focus; a liked heart is drawn
/// at rest too.
///
/// Returns the click it reported, if any, so the row does not also take it for
/// a click on itself, and whether the pointer is on it. A heart whose state is
/// unknown is still drawn on reveal and absorbs the click without reporting one:
/// the column is the reference's, and a click there is not a click on the row. It
/// is inert, so it adds no Tab stop.
fn like(
    ui: &mut Ui,
    rect: Rect,
    key: (&str, usize),
    revealed: bool,
    row: &Row<'_>,
    tokens: &Tokens,
) -> Heart {
    let liked = row.liked;
    if !drawable(rect) || row.look.carrying || !(liked == Some(true) || revealed) {
        return Heart::default();
    }
    let liked_now = liked == Some(true);
    let icon = if liked_now {
        Icon::HeartFilled
    } else {
        Icon::Heart
    };
    let label = heart_label(liked_now);
    let live = liked.is_some() && !row.look.carrying;
    let slot = if liked_now {
        controls::icon_button_liked(
            ui,
            tokens,
            Control::TrackLike,
            key,
            rect,
            icon,
            Size::SM,
            label,
            live,
        )
    } else {
        controls::icon_button_keyed(
            ui,
            tokens,
            Control::TrackLike,
            key,
            rect,
            icon,
            Size::SM,
            label,
            live,
        )
    };
    if let Some(liked) = liked {
        let _ = ui.ctx().accesskit_node_builder(slot.response.id, |node| {
            node.set_toggled(if liked {
                egui::accesskit::Toggled::True
            } else {
                egui::accesskit::Toggled::False
            });
        });
    }
    // egui reports a focused button's Enter or Space as a click, repeats too, so
    // a held key would toggle the heart once per frame. Only a fresh press is the
    // user's. A press takes the keyboard, as the `⋯`'s does, so Enter can toggle
    // the heart the pointer just clicked.
    let clicked = slot.response.clicked();
    if clicked {
        slot.response.request_focus();
    }
    let pointer = ui.input(|input| input.pointer.button_clicked(egui::PointerButton::Primary));
    let held = clicked && !pointer && context_menu::held_repeat(ui);
    Heart {
        hovered: under_pointer(&slot),
        toggled: (clicked && !held && live).then_some(!liked_now),
    }
}

/// The heart's accesskit label, which names the action rather than the state.
fn heart_label(liked: bool) -> &'static str {
    crate::verbs::heart_label(liked)
}

/// The track's length, right-aligned in its column.
fn duration(ui: &Ui, rect: Rect, label: Option<&str>, tokens: &Tokens, selected: bool) {
    if !drawable(rect) {
        return;
    }
    if let Some(label) = label {
        right(
            ui,
            rect,
            label,
            recipe::track_mono(text::XS),
            ink(
                tokens.palette.text_subtle,
                tokens.palette.text_muted,
                selected,
            ),
        );
    }
}

/// What the row's more button reports.
struct More {
    /// Whether the pointer is on it, so a click here is not a click on the row.
    hovered: bool,
    /// The door a press on it opened.
    door: Option<Door>,
    /// Its own widget id, which is the opener when the keyboard opened the menu.
    id: egui::Id,
    /// Whether it holds the keyboard.
    focused: bool,
}

/// The more button, which opens the row's menu.
///
/// It is live only when the page says the menu may open, and it says its menu is
/// up while it is: the sheet draws the open `⋯` in full ink on the hover's fill.
fn more(
    ui: &mut Ui,
    rect: Rect,
    key: (&str, usize),
    row: &Row<'_>,
    body: egui::Id,
    tokens: &Tokens,
) -> More {
    let live = row.menu && !row.look.carrying;
    if !drawable(rect) {
        return More {
            hovered: false,
            door: None,
            id: body,
            focused: false,
        };
    }
    let id = controls::slot_id(ui, Control::TrackMore, key);
    // A right-click or the keyboard opens the menu from the row's body, so the
    // button says it is open for either opener.
    let opener = context_menu::open_opener(ui.ctx());
    let open = opener.is_some_and(|opener| opener == id || opener == body);
    let slot = controls::icon_button_open(
        ui,
        tokens,
        Control::TrackMore,
        key,
        rect,
        Icon::More,
        Size::SM,
        &format!("More options for {}", row.title),
        live,
        open,
    );
    let _ = ui.ctx().accesskit_node_builder(slot.response.id, |node| {
        node.set_expanded(open);
        node.set_has_popup(egui::accesskit::HasPopup::Menu);
    });
    More {
        hovered: under_pointer(&slot),
        door: context_menu::button_door(ui, &slot, rect, id),
        id,
        focused: slot.response.has_focus(),
    }
}

/// The keyboard's door: the context-menu key or Shift+F10, on the focused row or
/// its focused `⋯`.
fn key_door(ui: &Ui, row: &egui::Response, more: &More, anchor: Rect) -> Option<Door> {
    let (row_focused, more_focused) = (row.has_focus(), more.focused);
    if !row_focused && !more_focused {
        return None;
    }
    context_menu::menu_key_pressed(ui).then_some(Door {
        anchor: Anchor::Button(anchor),
        opener: if more_focused { more.id } else { row.id },
        device: Device::Keyboard,
    })
}

/// The pointer's door: a right-click on the row.
///
/// The pointer is read rather than the row's own `secondary_clicked()`, because
/// the link cells are interacted after the row and win the hit test: a
/// right-click over the album or the artist is still the row's. The rect test is
/// the layer and clip test, so a right-click under another layer, or outside the
/// scroll's clip, opens nothing.
pub(super) fn pointer_door(ui: &Ui, rect: Rect, opener: egui::Id, anchor: Rect) -> Option<Door> {
    let secondary = ui.input(|input| input.pointer.secondary_clicked());
    let over = ui
        .ctx()
        .rect_contains_pointer(ui.layer_id(), rect.intersect(ui.clip_rect()));
    // At the cursor, not at the `⋯`: the click named a place. Falls back to the
    // `⋯` when the pointer has already gone.
    let at = ui
        .input(|input| input.pointer.interact_pos())
        .map_or(Anchor::Button(anchor), Anchor::Pointer);
    (secondary && over).then_some(Door {
        anchor: at,
        opener,
        device: Device::Pointer,
    })
}

/// Whether the pointer is on a control inside the row.
///
/// `hovered()` goes false while the button is down, because egui hovers only
/// what is being clicked. The row reads where the pointer is instead, so the
/// press that a control takes is not also taken for a press on the row.
fn under_pointer(slot: &controls::Slot) -> bool {
    slot.response.hovered() || slot.response.contains_pointer()
}

/// One of two inks, chosen by a flag: `picked` is the ink a picked row takes.
fn ink(resting: Color32, picked: Color32, picked_now: bool) -> Color32 {
    if picked_now { picked } else { resting }
}

/// Whether a rect is worth drawing into. An absent column arrives as a zero-width
/// rect, and a zero-width galley is a wasted layout.
fn drawable(rect: Rect) -> bool {
    rect.width() > 0.0
}

/// The height of one line of `font`.
fn line_height(ui: &Ui, font: &FontId) -> f32 {
    ui.ctx().fonts_mut(|fonts| fonts.row_height(font))
}

/// Paints a line against the left edge of `rect`, elided to its width.
fn left(ui: &Ui, rect: Rect, label: &str, font: FontId, color: Color32) {
    left_at(
        ui,
        rect,
        vertically_centred(ui, rect, &font),
        label,
        font,
        color,
    );
}

/// Paints a line against the left edge of `rect` with its top at `top`.
fn left_at(ui: &Ui, rect: Rect, top: f32, label: &str, font: FontId, color: Color32) {
    let galley = recipe::line(ui, label, font, color, 0.0, rect.width());
    ui.painter()
        .galley(Pos2::new(rect.left(), top), galley, color);
}

/// Paints a line centred in `rect`.
fn centred_line(ui: &Ui, rect: Rect, label: &str, font: FontId, color: Color32) {
    let galley = recipe::line(ui, label, font, color, 0.0, rect.width());
    let pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(pos, galley, color);
}

/// Paints a line against the right edge of `rect`.
fn right(ui: &Ui, rect: Rect, label: &str, font: FontId, color: Color32) {
    let galley = recipe::line(ui, label, font, color, 0.0, rect.width());
    let x = rect.right() - galley.size().x;
    ui.painter().galley(
        Pos2::new(x, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

/// The top a single line takes to sit centred in `rect`.
fn vertically_centred(ui: &Ui, rect: Rect, font: &FontId) -> f32 {
    rect.center().y - line_height(ui, font) / 2.0
}

/// A square of `side` centred in `rect`, snapped so a hairline glyph lands on
/// whole pixels.
fn centred(ui: &Ui, rect: Rect, side: f32) -> Rect {
    Rect::from_center_size(rect.center(), vec2(side, side)).round_to_pixels(ui.pixels_per_point())
}

#[cfg(test)]
mod tests {
    use egui::{Event, Key, Modifiers};

    use super::*;
    use crate::cover::Cover;
    use crate::route::Page;
    use crate::theme::Tokens;
    use crate::view::test_support::{button, click_events, secondary_click_events};

    fn input_with(events: Vec<Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 800.0))),
            events,
            ..Default::default()
        }
    }

    fn input() -> egui::RawInput {
        input_with(Vec::new())
    }

    /// The rect the pointer tests draw their row into.
    fn body_rect() -> Rect {
        crate::layout::table::row_rect(
            Rect::from_min_size(Pos2::ZERO, vec2(900.0, crate::theme::extent::ROW_H)),
            0,
            crate::theme::extent::ROW_H,
        )
    }

    fn columns() -> Columns {
        crate::layout::table::columns(900.0, crate::layout::Table::Full.columns())
    }

    /// A playlist row with both link cells live and every column filled.
    fn row_of(number: usize) -> Row<'static> {
        Row {
            uri: "spotify:track:a",
            number,
            title: "Harbour Wall",
            title_cell: TitleCell::ArtThenName,
            artist: "Slow Tide",
            artist_uri: Some("spotify:artist:b"),
            album: Some("Record"),
            album_uri: Some("spotify:album:c"),
            added: Some("2 Jan 2024"),
            duration: Some("3:42"),
            state: State::Resting,
            cover: Cover::Missing,
            look: Look::default(),
            menu: false,
            liked: None,
        }
    }

    /// A point on the row that is the row's own: the index column carries neither
    /// a link nor a control.
    fn body_point() -> Pos2 {
        number_box(columns().at(body_rect().min).index).center()
    }

    /// Runs one pass per event, and reports what each pass answered.
    fn run(
        ctx: &egui::Context,
        row: &Row<'_>,
        events: &[Event],
        modifiers: Modifiers,
    ) -> Vec<Option<Click>> {
        let tokens = Tokens::DARK;
        let rect = body_rect();
        let columns = columns();
        // Warm up, so hit testing has a previous pass to read. The held
        // modifiers arrive as an event and stay held for the passes after it.
        let mut output = ctx.run_ui(input_with(vec![Event::ModifiersChanged(modifiers)]), |ui| {
            let _ = show(ui, rect, columns, row, &tokens);
        });
        output.textures_delta.clear();

        let mut answers = Vec::new();
        for event in events {
            let mut answer = None;
            let mut output = ctx.run_ui(input_with(vec![event.clone()]), |ui| {
                answer = show(ui, rect, columns, row, &tokens);
            });
            output.textures_delta.clear();
            answers.push(answer);
        }
        answers
    }

    /// The fill painted behind the row on one frame.
    fn row_fill(ctx: &egui::Context, row: &Row<'_>, pos: Pos2) -> Option<Color32> {
        let tokens = Tokens::DARK;
        let rect = body_rect();
        let columns = columns();
        let mut output = ctx.run_ui(input_with(vec![Event::PointerMoved(pos)]), |ui| {
            let _ = show(ui, rect, columns, row, &tokens);
        });
        output.textures_delta.clear();
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape) if shape.rect == rect => Some(shape.fill),
                _ => None,
            })
    }

    /// Every label one frame painted.
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

    /// Whether one frame painted anything into `rect`, glyphs and icons alike.
    fn painted_into(output: &egui::FullOutput, rect: Rect) -> bool {
        output
            .shapes
            .iter()
            .any(|clipped| clipped.shape.visual_bounding_rect().intersects(rect))
    }

    /// One press at `from` released `by` points away.
    fn twitch(from: Pos2, by: f32) -> Vec<Event> {
        let to = from + vec2(by, 0.0);
        vec![
            Event::PointerMoved(from),
            button(from, true),
            Event::PointerMoved(to),
            button(to, false),
        ]
    }

    #[test]
    fn the_equaliser_bars_breathe_inside_their_range() {
        for step in 0..64 {
            let time = step as f32 * eq::PULSE / 16.0;
            for scale in bar_scales(time) {
                assert!(
                    (eq::MIN_SCALE..=1.0).contains(&scale),
                    "{scale} at {time}s is outside the pulse"
                );
            }
        }
    }

    #[test]
    fn a_bar_starts_short_and_peaks_halfway_through_its_pulse() {
        let at_rest = bar_scales(0.0)[0];
        assert!((at_rest - eq::MIN_SCALE).abs() < 1e-4, "{at_rest}");
        let at_peak = bar_scales(eq::PULSE / 2.0)[0];
        assert!((at_peak - 1.0).abs() < 1e-4, "{at_peak}");
    }

    #[test]
    fn each_bar_follows_the_one_before_it_late() {
        // The reference staggers the bars in time rather than by height, so a bar's
        // height now is the bar before it's height one delay ago.
        let phase = 0.3;
        let scales = bar_scales(eq::PULSE * phase);
        for (bar, scale) in scales.iter().enumerate().skip(1) {
            let behind = bar_scales(eq::PULSE * phase - bar as f32 * eq::DELAY)[0];
            assert!(
                (*scale - behind).abs() < 1e-4,
                "bar {bar} is {scale} where its leader was {behind}"
            );
        }
    }

    #[test]
    fn the_bars_are_not_all_the_same_height_at_once() {
        let spread = (0..64).any(|step| {
            let scales = bar_scales(step as f32 * eq::PULSE / 64.0);
            scales[0] != scales[1] || scales[1] != scales[2]
        });
        assert!(
            spread,
            "the bars pulse in unison, so the stagger is invisible"
        );
    }

    /// The two stacked lines' heights at the app's own fonts, read the way a
    /// frame reads them: after one pass, because egui has no metrics before it.
    fn font_heights() -> (f32, f32) {
        let tokens = Tokens::DARK;
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::theme::fonts::definitions());
        ctx.set_global_style(crate::theme::style(&tokens));
        let mut heights = None;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            heights = Some(ui.ctx().fonts_mut(|fonts| {
                (
                    fonts.row_height(&recipe::track_name()),
                    fonts.row_height(&recipe::track_secondary()),
                )
            }));
        });
        output.textures_delta.clear();
        heights.expect("a frame ran")
    }

    #[test]
    fn stacked_tops_centres_the_two_lines_as_one_block() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), vec2(200.0, 44.0));
        let (name_top, artist_top) = stacked_tops(rect, 16.0, 12.0);

        // The block is 28 tall, so it starts 8 above and ends 8 below the centre.
        assert!((name_top - (rect.center().y - 14.0)).abs() < 1e-4);
        assert!((artist_top - (name_top + 16.0)).abs() < 1e-4);
    }

    #[test]
    fn stacked_tops_keeps_both_lines_inside_the_rect() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), vec2(200.0, 44.0));
        for (name_h, artist_h) in [(16.0, 12.0), (20.0, 0.0), (0.0, 12.0), (44.0, 0.0)] {
            let (name_top, artist_top) = stacked_tops(rect, name_h, artist_h);
            assert!(name_top >= rect.top() - 1e-4, "{name_h}/{artist_h}");
            assert!(
                artist_top + artist_h <= rect.bottom() + 1e-4,
                "{name_h}/{artist_h}: the block spills below the rect"
            );
        }
    }

    #[test]
    fn stacked_tops_is_finite_for_a_zero_height() {
        let empty = Rect::from_min_size(Pos2::ZERO, vec2(0.0, 0.0));
        for (name_h, artist_h) in [(0.0, 0.0), (f32::NAN, 0.0), (0.0, f32::INFINITY)] {
            let (name_top, artist_top) = stacked_tops(empty, name_h, artist_h);
            assert!(name_top.is_finite(), "{name_h}/{artist_h}: {name_top}");
            assert!(artist_top.is_finite(), "{name_h}/{artist_h}: {artist_top}");
        }
    }

    /// The measured fit, not an assumed one: the two lines must fit the row's
    /// inner box at today's tokens. If a token grows, this fails before a page
    /// can clip its own subtitle.
    #[test]
    fn the_stacked_title_fits_the_rows_inner_box() {
        let (name_h, artist_h) = font_heights();
        let inner_h = crate::theme::extent::ROW_H - 2.0 * crate::theme::space::S1;

        assert!(
            name_h + artist_h <= inner_h,
            "the two title lines take {} (name {name_h}, artist {artist_h}) but the \
             row's inner box is {inner_h}",
            name_h + artist_h
        );
    }

    #[test]
    fn an_absent_column_is_never_drawn_into() {
        // An absent column arrives as a zero-width rect rather than as a missing
        // field, so nothing may lay out a galley against a width it never got.
        let empty = Rect::from_min_size(Pos2::ZERO, vec2(0.0, crate::theme::extent::ROW_H));
        assert!(!drawable(empty));
        assert!(drawable(Rect::from_min_size(
            Pos2::ZERO,
            vec2(
                crate::theme::extent::TRACK_INDEX_W,
                crate::theme::extent::ROW_H
            )
        )));
    }

    /// A wide album-block row: the name over its artist subline, no art box.
    fn episode_row() -> Row<'static> {
        Row {
            uri: "spotify:episode:a",
            number: 1,
            title: "The Episode",
            title_cell: TitleCell::NameOverArtist,
            artist: "Lateral with Tom Scott",
            // The episode names its show as its credit, so this subline opens
            // the show rather than an artist.
            artist_uri: Some("spotify:show:s"),
            album: None,
            album_uri: None,
            added: None,
            duration: None,
            state: State::Resting,
            cover: Cover::Missing,
            look: Look::default(),
            menu: false,
            liked: None,
        }
    }

    fn context() -> egui::Context {
        let tokens = Tokens::DARK;
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::theme::fonts::definitions());
        ctx.set_global_style(crate::theme::style(&tokens));
        ctx
    }

    /// The rect a painted line occupies, read back from a frame.
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

    /// The click one sequence of passes reports for a pointer at `pos`.
    fn click(ctx: &egui::Context, pos: Pos2) -> Option<Click> {
        run(ctx, &episode_row(), &click_events(pos), Modifiers::NONE)
            .pop()
            .flatten()
    }

    /// Finding 4: `Loading` is a track on its way and an ended one has nothing
    /// sounding, so neither may masquerade as a pause — the equaliser would
    /// freeze on a stopped track and read as "held".
    #[test]
    fn a_loaded_row_is_only_paused_when_the_transport_is() {
        let uri = "spotify:track:a";
        let cases = [
            (TransportState::Idle, State::Stopped),
            (TransportState::Loading, State::Playing),
            (TransportState::Playing, State::Playing),
            (TransportState::Paused, State::Paused),
            (TransportState::Unavailable, State::Stopped),
            (TransportState::Stopped, State::Stopped),
            (
                TransportState::Failed(crate::message::Failure::Other),
                State::Stopped,
            ),
        ];
        for (state, expected) in cases {
            let mut transport = Transport::default();
            transport.play(uri, "A", "B", None);
            transport.state = state;
            assert_eq!(state_of(uri, &transport), expected, "{state:?}");
            assert_eq!(
                state_of("spotify:track:b", &transport),
                State::Resting,
                "another row while {state:?}"
            );
        }
    }

    #[test]
    fn a_credit_click_on_an_episode_row_opens_the_show() {
        let ctx = context();
        // The artist subline is the bottom line of the two stacked lines, so the
        // click lands on the painted artist words rather than the name above, or
        // on the index column to the left of the title. The words' own rect is
        // where the link's hit area sits.
        let mut first = ctx.run_ui(input(), |ui| {
            let _ = show(
                ui,
                crate::layout::table::row_rect(
                    Rect::from_min_size(Pos2::ZERO, vec2(900.0, crate::theme::extent::ROW_H)),
                    0,
                    crate::theme::extent::ROW_H,
                ),
                crate::layout::table::columns(900.0, crate::layout::Table::Full.columns()),
                &episode_row(),
                &Tokens::DARK,
            );
        });
        first.textures_delta.clear();
        let artist = text_rect(&first, "Lateral with Tom Scott").expect("the artist subline");
        assert_eq!(
            click(&ctx, artist.center()),
            Some(Click::Open(Page::credit("spotify:show:s"))),
            "a click on the episode's credit opens the show page"
        );
    }

    #[test]
    fn a_single_click_on_the_body_selects_and_does_not_play() {
        let ctx = context();
        let row = row_of(1);
        let answers = run(&ctx, &row, &click_events(body_point()), Modifiers::NONE);

        assert_eq!(
            answers.iter().flatten().next(),
            Some(&Click::Select(Press::Replace)),
            "a single click picks the row"
        );
        assert!(
            !answers.iter().flatten().any(|click| *click == Click::Play),
            "a single click played: {answers:?}"
        );
    }

    #[test]
    fn a_double_click_plays() {
        let ctx = context();
        let row = row_of(1);
        let point = body_point();
        let mut events = click_events(point).to_vec();
        events.extend(click_events(point));
        let answers = run(&ctx, &row, &events, Modifiers::NONE);

        assert!(
            answers.iter().flatten().any(|click| *click == Click::Play),
            "a double click did not play: {answers:?}"
        );
    }

    #[test]
    fn enter_on_a_focused_row_plays_and_does_not_select() {
        let ctx = context();
        let row = row_of(1);
        let point = body_point();
        // The click is what gives the row the keyboard, and it selects once.
        let first = run(&ctx, &row, &click_events(point), Modifiers::NONE);
        assert!(
            first
                .iter()
                .flatten()
                .any(|click| matches!(click, Click::Select(_))),
            "the row never took the keyboard: {first:?}"
        );

        let key = |pressed| Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let answers = run(&ctx, &row, &[key(true), key(false)], Modifiers::NONE);

        assert_eq!(
            answers.iter().flatten().last(),
            Some(&Click::Play),
            "Enter on a focused row plays"
        );
        assert!(
            !answers
                .iter()
                .flatten()
                .any(|click| matches!(click, Click::Select(_))),
            "Enter selected the row as well as playing it: {answers:?}"
        );
    }

    #[test]
    fn a_link_click_opens_and_does_not_select() {
        let ctx = context();
        let row = row_of(1);
        let artist = columns().at(body_rect().min).artist.center();
        let answers = run(&ctx, &row, &click_events(artist), Modifiers::NONE);

        assert_eq!(
            answers.iter().flatten().last(),
            Some(&Click::Open(Page::credit("spotify:artist:b"))),
            "a click on the artist cell opens it"
        );
        assert!(
            !answers
                .iter()
                .flatten()
                .any(|click| matches!(click, Click::Select(_))),
            "a link click selected the row: {answers:?}"
        );
    }

    #[test]
    fn a_click_on_the_like_does_not_select_or_play() {
        let ctx = context();
        let row = row_of(1);
        let point = columns().at(body_rect().min).like.center();
        // A double click, because the second click is the one that would
        // otherwise play the row underneath the control.
        let mut events = click_events(point).to_vec();
        events.extend(click_events(point));
        let answers = run(&ctx, &row, &events, Modifiers::NONE);

        assert!(
            answers.iter().flatten().next().is_none(),
            "a click on the like reported {answers:?}"
        );
    }

    /// A row whose heart knows its state, or does not.
    fn hearted(liked: Option<bool>) -> Row<'static> {
        Row { liked, ..row_of(1) }
    }

    fn like_rect() -> Rect {
        columns().at(body_rect().min).like
    }

    /// One frame of the row, with the shapes it painted.
    fn frame(ctx: &egui::Context, row: &Row<'_>, events: Vec<Event>) -> egui::FullOutput {
        let tokens = Tokens::DARK;
        let mut output = ctx.run_ui(input_with(events), |ui| {
            let _ = show(ui, body_rect(), columns(), row, &tokens);
        });
        output.textures_delta.clear();
        output
    }

    /// The sense the heart's slot was registered with this frame, if it drew.
    fn heart_sense(ctx: &egui::Context, row: &Row<'_>, events: Vec<Event>) -> Option<egui::Sense> {
        let tokens = Tokens::DARK;
        let mut output = ctx.run_ui(input_with(Vec::new()), |ui| {
            let _ = show(ui, body_rect(), columns(), row, &tokens);
        });
        output.textures_delta.clear();
        let mut sense = None;
        let mut output = ctx.run_ui(input_with(events), |ui| {
            let _ = show(ui, body_rect(), columns(), row, &tokens);
            sense = ui
                .ctx()
                .read_response(controls::slot_id(
                    ui,
                    Control::TrackLike,
                    (row.uri, row.number),
                ))
                .map(|response| response.sense);
        });
        output.textures_delta.clear();
        sense
    }

    #[test]
    fn a_liked_row_draws_its_heart_with_no_hover_or_focus() {
        let ctx = context();
        let output = frame(&ctx, &hearted(Some(true)), Vec::new());
        assert!(
            !glyph_colors(&output, like_rect()).is_empty(),
            "a liked heart is not visible at rest"
        );
    }

    #[test]
    fn an_unliked_row_draws_no_heart_until_hover_or_focus() {
        let ctx = context();
        let output = frame(&ctx, &hearted(Some(false)), Vec::new());
        assert!(
            glyph_colors(&output, like_rect()).is_empty(),
            "an unliked heart was drawn at rest"
        );

        let output = frame(
            &ctx,
            &hearted(Some(false)),
            vec![Event::PointerMoved(body_point())],
        );
        assert!(
            !glyph_colors(&output, like_rect()).is_empty(),
            "an unliked heart was not revealed under the pointer"
        );
    }

    #[test]
    fn focus_on_the_more_button_reveals_the_heart() {
        let ctx = context();
        let row = Row {
            menu: true,
            liked: Some(false),
            ..row_of(1)
        };
        // The click gives the `⋯` the keyboard; the menu it opens is not the
        // point here.
        let _ = run(
            &ctx,
            &row,
            &click_events(actions().center()),
            Modifiers::NONE,
        );
        let output = frame(&ctx, &row, vec![Event::PointerMoved(Pos2::new(1.0, 1.0))]);
        assert!(
            !glyph_colors(&output, like_rect()).is_empty(),
            "the heart vanished while the `⋯` held the keyboard"
        );
    }

    #[test]
    fn focus_on_the_heart_keeps_it_drawn_and_focused() {
        let ctx = context();
        let row = hearted(Some(false));
        let point = like_rect().center();
        let first = run(&ctx, &row, &click_events(point), Modifiers::NONE);
        assert_eq!(
            first.iter().flatten().last(),
            Some(&Click::Like(true)),
            "the heart did not take the click"
        );

        // The pointer leaves; the heart must stay drawn because it holds the
        // keyboard, and it must still answer Enter.
        let _ = run(
            &ctx,
            &row,
            &[Event::PointerMoved(Pos2::new(1.0, 1.0))],
            Modifiers::NONE,
        );
        let output = frame(&ctx, &row, Vec::new());
        assert!(
            !glyph_colors(&output, like_rect()).is_empty(),
            "the focused heart was hidden the frame after it took the keyboard"
        );
        let enter = |pressed| Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let answers = run(&ctx, &row, &[enter(true), enter(false)], Modifiers::NONE);
        assert_eq!(
            answers.iter().flatten().last(),
            Some(&Click::Like(true)),
            "the focused heart did not answer Enter"
        );
    }

    #[test]
    fn a_click_on_a_live_heart_reports_like_and_not_select_play_or_open() {
        let ctx = context();
        let row = hearted(Some(false));
        let answers = run(
            &ctx,
            &row,
            &click_events(like_rect().center()),
            Modifiers::NONE,
        );
        assert_eq!(answers.iter().flatten().last(), Some(&Click::Like(true)));
        assert!(
            !answers.iter().flatten().any(|click| matches!(
                click,
                Click::Select(_) | Click::Play | Click::Open(_) | Click::Lift
            )),
            "a heart click did something else too: {answers:?}"
        );
    }

    #[test]
    fn the_click_asks_for_the_opposite_of_the_rows_state() {
        let ctx = context();
        let row = hearted(Some(true));
        let answers = run(
            &ctx,
            &row,
            &click_events(like_rect().center()),
            Modifiers::NONE,
        );
        assert_eq!(answers.iter().flatten().last(), Some(&Click::Like(false)));
    }

    #[test]
    fn an_unknown_heart_reports_nothing() {
        let ctx = context();
        let row = hearted(None);
        let answers = run(
            &ctx,
            &row,
            &click_events(like_rect().center()),
            Modifiers::NONE,
        );
        assert!(
            answers.iter().flatten().next().is_none(),
            "an unknown heart reported {answers:?}"
        );
    }

    #[test]
    fn a_double_click_on_the_heart_does_not_play() {
        let ctx = context();
        let row = hearted(Some(false));
        let point = like_rect().center();
        let mut events = click_events(point).to_vec();
        events.extend(click_events(point));
        let answers = run(&ctx, &row, &events, Modifiers::NONE);
        assert!(
            !answers.iter().flatten().any(|click| *click == Click::Play),
            "a double click on the heart played the row: {answers:?}"
        );
    }

    #[test]
    fn a_right_click_on_the_heart_still_opens_the_menu() {
        let ctx = context();
        let row = Row {
            menu: true,
            liked: Some(false),
            ..row_of(1)
        };
        let answers = run(
            &ctx,
            &row,
            &secondary_click_events(like_rect().center()),
            Modifiers::NONE,
        );
        assert_eq!(
            door(&answers).map(|door| door.device),
            Some(Device::Pointer),
            "a right-click on the heart opened no menu: {answers:?}"
        );
    }

    #[test]
    fn enter_on_a_focused_heart_toggles_once() {
        let ctx = context();
        let row = hearted(Some(false));
        let _ = run(
            &ctx,
            &row,
            &click_events(like_rect().center()),
            Modifiers::NONE,
        );
        let enter = |repeat| Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat,
            modifiers: Modifiers::NONE,
        };
        let answers = run(&ctx, &row, &[enter(false)], Modifiers::NONE);
        assert_eq!(answers.iter().flatten().last(), Some(&Click::Like(true)));
    }

    #[test]
    fn a_held_enter_on_the_heart_toggles_once() {
        let ctx = context();
        let row = hearted(Some(false));
        let _ = run(
            &ctx,
            &row,
            &click_events(like_rect().center()),
            Modifiers::NONE,
        );
        let enter = |repeat| Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat,
            modifiers: Modifiers::NONE,
        };
        let answers = run(
            &ctx,
            &row,
            &[enter(false), enter(true), enter(true)],
            Modifiers::NONE,
        );
        assert_eq!(
            answers
                .iter()
                .flatten()
                .filter(|click| matches!(click, Click::Like(_)))
                .count(),
            1,
            "a held Enter toggled more than once: {answers:?}"
        );
    }

    #[test]
    fn the_heart_is_a_tab_stop_only_when_live() {
        let ctx = context();
        let live = heart_sense(
            &ctx,
            &hearted(Some(false)),
            vec![Event::PointerMoved(body_point())],
        );
        assert!(
            live.is_some_and(|sense| sense.is_focusable()),
            "a live heart is not a Tab stop"
        );

        let ctx = context();
        let unknown = heart_sense(
            &ctx,
            &hearted(None),
            vec![Event::PointerMoved(body_point())],
        );
        assert!(
            unknown.is_some_and(|sense| !sense.is_focusable()),
            "an unknown heart joined the Tab order"
        );
    }

    #[test]
    fn a_drag_that_starts_on_the_heart_lifts_nothing() {
        let ctx = context();
        let row = hearted(Some(false));
        let answers = run(
            &ctx,
            &row,
            &twitch(like_rect().center(), 7.0),
            Modifiers::NONE,
        );
        assert!(
            !answers.iter().flatten().any(|click| *click == Click::Lift),
            "a drag from the heart lifted the row: {answers:?}"
        );
    }

    #[test]
    fn a_drag_elsewhere_on_a_row_with_a_live_heart_still_lifts() {
        let ctx = context();
        let row = hearted(Some(false));
        let answers = run(&ctx, &row, &twitch(body_point(), 7.0), Modifiers::NONE);
        assert!(
            answers.iter().flatten().any(|click| *click == Click::Lift),
            "a live heart stopped the row's own drag: {answers:?}"
        );
    }

    #[test]
    fn a_carried_row_draws_no_heart_even_when_liked() {
        let ctx = context();
        let row = Row {
            liked: Some(true),
            look: Look {
                carrying: true,
                ..Look::default()
            },
            ..row_of(1)
        };
        let output = frame(&ctx, &row, Vec::new());
        assert!(
            glyph_colors(&output, like_rect()).is_empty(),
            "a carried row drew its heart"
        );
    }

    #[test]
    fn a_heart_stays_live_while_a_playlist_write_is_pending() {
        let ctx = context();
        // The page's gate is shut for the menu, but a heart is not the menu.
        let row = Row {
            menu: false,
            liked: Some(true),
            ..row_of(1)
        };
        let answers = run(
            &ctx,
            &row,
            &click_events(like_rect().center()),
            Modifiers::NONE,
        );
        assert_eq!(answers.iter().flatten().last(), Some(&Click::Like(false)));
    }

    #[test]
    fn the_liked_heart_is_painted_in_the_accent() {
        let ctx = context();
        let output = frame(&ctx, &hearted(Some(true)), Vec::new());
        let colors = glyph_colors(&output, like_rect());
        assert!(
            colors.contains(&Tokens::DARK.palette.accent),
            "the liked heart was painted {colors:?}"
        );
    }

    /// A playlist row whose menu may open.
    fn menu_row() -> Row<'static> {
        Row {
            menu: true,
            ..row_of(1)
        }
    }

    /// The one door a run reported, if it reported any.
    fn door(answers: &[Option<Click>]) -> Option<Door> {
        answers.iter().flatten().find_map(|click| match click {
            Click::Menu(door) => Some(*door),
            _ => None,
        })
    }

    /// The actions column of the row, which every door anchors to.
    fn actions() -> Rect {
        columns().at(body_rect().min).actions
    }

    #[test]
    fn the_more_button_opens_a_button_door() {
        let ctx = context();
        let row = menu_row();
        let answers = run(
            &ctx,
            &row,
            &click_events(actions().center()),
            Modifiers::NONE,
        );

        let door = door(&answers).expect("the more opened a menu");
        assert_eq!(door.anchor, Anchor::Button(actions()));
        assert_eq!(door.device, Device::Pointer);
        assert!(
            !answers
                .iter()
                .flatten()
                .any(|click| matches!(click, Click::Select(_) | Click::Play)),
            "the press on the more also picked or played the row: {answers:?}"
        );
    }

    #[test]
    fn a_right_click_on_the_body_a_link_cell_and_the_index_opens_at_the_pointer() {
        let columns = columns().at(body_rect().min);
        let points = [
            body_point(),
            columns.artist.center(),
            columns.album.center(),
            number_box(columns.index).center(),
        ];
        for point in points {
            let ctx = context();
            let row = menu_row();
            let answers = run(&ctx, &row, &secondary_click_events(point), Modifiers::NONE);

            let door = door(&answers).unwrap_or_else(|| panic!("no menu at {point:?}"));
            let Anchor::Pointer(at) = door.anchor else {
                panic!(
                    "a right-click at {point:?} anchored to the `⋯`: {:?}",
                    door.anchor
                );
            };
            assert!(
                (at - point).length() < 0.01,
                "a right-click at {point:?} opened at {at:?}"
            );
            assert_eq!(door.device, Device::Pointer, "at {point:?}");
            assert!(
                !answers
                    .iter()
                    .flatten()
                    .any(|click| matches!(click, Click::Select(_) | Click::Open(_) | Click::Play)),
                "a right-click at {point:?} did more than open the menu: {answers:?}"
            );
        }
    }

    #[test]
    fn a_pointer_click_on_the_open_more_posts_no_door_and_keeps_the_keyboard() {
        let ctx = context();
        let row = menu_row();
        let key = (row.uri, row.number);
        let mut more_id = None;
        let mut output = ctx.run_ui(input_with(Vec::new()), |ui| {
            more_id = Some(controls::slot_id(ui, Control::TrackMore, key));
            let _ = show(ui, body_rect(), columns(), &row, &Tokens::DARK);
        });
        output.textures_delta.clear();
        let id = more_id.expect("a frame ran");

        // The menu is up, and this `⋯` is the one that opened it.
        context_menu::note_open(&ctx, Some(id));
        let answers = run(
            &ctx,
            &row,
            &click_events(actions().center()),
            Modifiers::NONE,
        );
        assert_eq!(
            door(&answers),
            None,
            "the open `⋯` posted a fresh request instead of closing"
        );

        let mut focused = false;
        let mut output = ctx.run_ui(input_with(Vec::new()), |ui| {
            let _ = show(ui, body_rect(), columns(), &row, &Tokens::DARK);
            focused = ui.memory(|memory| memory.has_focus(id));
        });
        output.textures_delta.clear();
        assert!(focused, "the keyboard did not come back to the `⋯`");
    }

    #[test]
    fn a_held_enter_does_not_reopen_the_menu_from_the_more() {
        let ctx = context();
        let row = menu_row();
        // The click gives the `⋯` the keyboard, as a pointer does.
        let _ = run(
            &ctx,
            &row,
            &click_events(actions().center()),
            Modifiers::NONE,
        );

        let press = Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let first = run(&ctx, &row, std::slice::from_ref(&press), Modifiers::NONE);
        assert!(
            door(&first).is_some(),
            "a fresh Enter on the `⋯` opened nothing"
        );

        // Still held: egui marks a press with no release before it a repeat.
        let held = run(&ctx, &row, &[press], Modifiers::NONE);
        assert_eq!(door(&held), None, "a held Enter reopened the menu");
        assert!(
            held.iter().flatten().next().is_none(),
            "a held Enter reported {held:?}"
        );
    }

    #[test]
    fn shift_f10_and_the_menu_key_open_on_a_focused_row_but_plain_f10_does_not() {
        let ctx = context();
        let row = menu_row();
        // The click is what gives the row the keyboard.
        let first = run(&ctx, &row, &click_events(body_point()), Modifiers::NONE);
        assert!(
            first
                .iter()
                .flatten()
                .any(|click| matches!(click, Click::Select(_))),
            "the row never took the keyboard: {first:?}"
        );

        let press = |key, modifiers| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };

        let plain = run(
            &ctx,
            &row,
            &[press(Key::F10, Modifiers::NONE)],
            Modifiers::NONE,
        );
        assert_eq!(door(&plain), None, "plain F10 opened the menu");

        let shift = run(
            &ctx,
            &row,
            &[press(Key::F10, Modifiers::SHIFT)],
            Modifiers::SHIFT,
        );
        let shift = door(&shift).expect("Shift+F10 opened no menu");
        assert_eq!(shift.device, Device::Keyboard);
        assert_eq!(shift.anchor, Anchor::Button(actions()));

        let menu_key = run(
            &ctx,
            &row,
            &[press(Key::Menu, Modifiers::NONE)],
            Modifiers::NONE,
        );
        assert_eq!(
            door(&menu_key).map(|door| door.device),
            Some(Device::Keyboard),
            "the context-menu key opened no menu"
        );
    }

    #[test]
    fn enter_on_a_focused_more_opens_with_the_keyboard_device() {
        let ctx = context();
        let row = menu_row();
        let at = actions().center();
        let first = run(&ctx, &row, &click_events(at), Modifiers::NONE);
        assert_eq!(
            door(&first).map(|door| door.device),
            Some(Device::Pointer),
            "the click did not open the menu"
        );

        let enter = |pressed| Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let answers = run(&ctx, &row, &[enter(true), enter(false)], Modifiers::NONE);
        assert_eq!(
            door(&answers).map(|door| door.device),
            Some(Device::Keyboard),
            "Enter on the focused more did not open the menu: {answers:?}"
        );
    }

    #[test]
    fn a_waiting_more_and_a_carried_row_open_nothing() {
        let ctx = context();
        // The page's gate is shut: the more is a waiting control.
        let waiting = row_of(1);
        for events in [
            click_events(actions().center()).to_vec(),
            secondary_click_events(actions().center()).to_vec(),
            secondary_click_events(body_point()).to_vec(),
        ] {
            let answers = run(&ctx, &waiting, &events, Modifiers::NONE);
            assert_eq!(door(&answers), None, "a waiting row opened a menu");
        }

        // A carried row is being dragged, so nothing it says is a door.
        let carried = Row {
            look: Look {
                carrying: true,
                ..Look::default()
            },
            ..menu_row()
        };
        let answers = run(
            &ctx,
            &carried,
            &secondary_click_events(body_point()),
            Modifiers::NONE,
        );
        assert_eq!(door(&answers), None, "a carried row opened a menu");
    }

    #[test]
    fn a_right_click_under_another_layer_is_ignored() {
        let ctx = context();
        let row = menu_row();
        let point = body_point();
        let cover = Rect::from_min_size(point - vec2(40.0, 20.0), vec2(200.0, 40.0));

        let answer = |events: Vec<Event>| {
            let mut answer = None;
            let mut output = ctx.run_ui(input_with(events), |ui| {
                egui::Area::new(egui::Id::unique("cover"))
                    .order(egui::Order::Middle)
                    .fixed_pos(cover.min)
                    .show(ui.ctx(), |ui| {
                        ui.expand_to_include_rect(cover);
                        ui.painter()
                            .rect_filled(cover, 0.0, Tokens::DARK.palette.surface);
                    });
                answer = show(ui, body_rect(), columns(), &row, &Tokens::DARK);
            });
            output.textures_delta.clear();
            answer
        };

        // Two frames, so the cover's own rect is known to the layer test.
        let _ = answer(Vec::new());
        let _ = answer(Vec::new());
        let answers: Vec<Option<Click>> = secondary_click_events(point)
            .into_iter()
            .map(|event| answer(vec![event]))
            .collect();
        assert_eq!(
            door(&answers),
            None,
            "a right-click under another layer opened the row's menu"
        );
    }

    #[test]
    fn the_open_more_button_is_lit_and_announces_expanded() {
        let ctx = context();
        // The node builders only exist with AccessKit on.
        ctx.enable_accesskit();
        let row = menu_row();
        let key = (row.uri, row.number);
        let mut more_id = None;
        let mut output = ctx.run_ui(
            input_with(vec![Event::PointerMoved(Pos2::new(899.0, 799.0))]),
            |ui| {
                more_id = Some(controls::slot_id(ui, Control::TrackMore, key));
                let _ = show(ui, body_rect(), columns(), &row, &Tokens::DARK);
            },
        );
        output.textures_delta.clear();
        let id = more_id.expect("a frame ran");
        assert!(
            !glyph_colors(&output, actions()).contains(&Tokens::DARK.palette.text),
            "a resting more was drawn open"
        );

        // The menu stores its opener; the row draws before the menu, so it reads
        // the frame before. The node is read in the frame that built it.
        context_menu::note_open(&ctx, Some(id));
        let mut nodes = None;
        let mut output = ctx.run_ui(input_with(Vec::new()), |ui| {
            let _ = show(ui, body_rect(), columns(), &row, &Tokens::DARK);
            nodes = ui
                .ctx()
                .accesskit_node_builder(id, |node| (node.is_expanded(), node.has_popup()));
        });
        output.textures_delta.clear();
        assert!(
            glyph_colors(&output, actions()).contains(&Tokens::DARK.palette.text),
            "the open more is not in full ink: {:?}",
            glyph_colors(&output, actions())
        );

        let (expanded, popup) = nodes.expect("the more has an AccessKit node");
        assert_eq!(expanded, Some(true), "the open more does not say so");
        assert_eq!(popup, Some(egui::accesskit::HasPopup::Menu));
    }

    #[test]
    fn the_more_button_is_a_tab_stop_only_when_live() {
        for (menu, focusable) in [(true, true), (false, false)] {
            let ctx = context();
            let row = Row { menu, ..row_of(1) };
            let key = (row.uri, row.number);
            let mut sense = None;
            let mut output = ctx.run_ui(input_with(Vec::new()), |ui| {
                let _ = show(ui, body_rect(), columns(), &row, &Tokens::DARK);
                sense = ui
                    .ctx()
                    .read_response(controls::slot_id(ui, Control::TrackMore, key))
                    .map(|response| response.sense);
            });
            output.textures_delta.clear();
            let sense = sense.expect("the more is drawn");
            assert_eq!(sense.is_focusable(), focusable, "menu: {menu}");
            assert_eq!(sense.senses_click(), focusable, "menu: {menu}");
        }
    }

    /// The colours of the glyphs painted inside `rect`.
    fn glyph_colors(output: &egui::FullOutput, rect: Rect) -> Vec<Color32> {
        output
            .shapes
            .iter()
            .filter(|clipped| clipped.shape.visual_bounding_rect().intersects(rect))
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Path(path) => Some(path.fill),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn shift_and_ctrl_clicks_carry_their_press() {
        for (modifiers, press) in [
            (Modifiers::SHIFT, Press::Extend),
            (Modifiers::COMMAND, Press::Toggle),
            (Modifiers::ALT, Press::Replace),
        ] {
            let ctx = context();
            let row = row_of(1);
            let answers = run(&ctx, &row, &click_events(body_point()), modifiers);
            assert_eq!(
                answers.iter().flatten().last(),
                Some(&Click::Select(press)),
                "{modifiers:?}"
            );
        }
    }

    #[test]
    fn a_three_point_twitch_between_clicks_does_not_lift_but_seven_points_does() {
        let ctx = context();
        let row = row_of(1);
        let answers = run(&ctx, &row, &twitch(body_point(), 3.0), Modifiers::NONE);
        assert!(
            answers
                .iter()
                .flatten()
                .any(|click| matches!(click, Click::Select(_))),
            "a twitch is still a click: {answers:?}"
        );
        assert!(
            !answers.iter().flatten().any(|click| *click == Click::Lift),
            "a three-point twitch lifted the row: {answers:?}"
        );

        let ctx = context();
        let row = row_of(1);
        let answers = run(&ctx, &row, &twitch(body_point(), 7.0), Modifiers::NONE);
        assert!(
            answers.iter().flatten().any(|click| *click == Click::Lift),
            "a seven-point move between the press and the release is a lift: {answers:?}"
        );
        assert!(
            !answers
                .iter()
                .flatten()
                .any(|click| matches!(click, Click::Select(_))),
            "a lift also selected the row: {answers:?}"
        );
    }

    #[test]
    fn a_lifted_row_reports_a_lift_once() {
        let ctx = context();
        let row = row_of(1);
        let from = body_point();
        let mut events = vec![Event::PointerMoved(from), button(from, true)];
        for step in 1..=6 {
            events.push(Event::PointerMoved(from + vec2(step as f32 * 3.0, 0.0)));
        }
        events.push(button(from + vec2(18.0, 0.0), false));

        let answers = run(&ctx, &row, &events, Modifiers::NONE);
        assert_eq!(
            answers
                .iter()
                .flatten()
                .filter(|click| **click == Click::Lift)
                .count(),
            1,
            "one drag reported more than one lift: {answers:?}"
        );
    }

    #[test]
    fn a_hovered_row_washes_with_row_hover() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let row = row_of(1);
        let point = body_point();

        // The reveal eases in, so the pointer rests for longer than one frame.
        for _ in 0..12 {
            let _ = row_fill(&ctx, &row, point);
        }
        assert_eq!(
            row_fill(&ctx, &row, point),
            Some(tokens.palette.row_hover),
            "the hovered row washed with something other than the row hover"
        );
        // The reveal eases out too, so the pointer rests away for the same time.
        for _ in 0..12 {
            let _ = row_fill(&ctx, &row, Pos2::new(899.0, 799.0));
        }
        assert_eq!(
            row_fill(&ctx, &row, Pos2::new(899.0, 799.0)),
            None,
            "a row with the pointer away still washed"
        );
    }

    #[test]
    fn the_row_does_not_reveal_or_fade_while_carrying() {
        let ctx = context();
        let row = Row {
            look: Look {
                carrying: true,
                ..Look::default()
            },
            ..row_of(1)
        };
        let point = body_point();
        for _ in 0..12 {
            let _ = row_fill(&ctx, &row, point);
        }

        assert_eq!(
            row_fill(&ctx, &row, point),
            None,
            "a carried row washed under the pointer"
        );
        // The number stands where the play glyph would, so the row is not
        // revealed either.
        let tokens = Tokens::DARK;
        let mut output = ctx.run_ui(input_with(vec![Event::PointerMoved(point)]), |ui| {
            let _ = show(ui, body_rect(), columns(), &row, &tokens);
        });
        output.textures_delta.clear();
        assert!(
            drawn_texts(&output).contains(&"1"),
            "the number gave way to the play glyph while carrying"
        );
    }

    /// A lift hands the row the focus, so a carried row has to shed the reveal
    /// that focus would otherwise bring.
    #[test]
    fn a_carried_row_holding_the_focus_keeps_its_number_and_hides_the_like() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let row = row_of(1);
        let point = body_point();
        let like = columns().at(body_rect().min).like;

        // The press is what gives the row the keyboard.
        let answers = run(&ctx, &row, &click_events(point), Modifiers::NONE);
        assert!(
            answers
                .iter()
                .flatten()
                .any(|click| matches!(click, Click::Select(_))),
            "the row never took the keyboard: {answers:?}"
        );

        // The pointer leaves, so only the focus can reveal anything.
        let mut focused = ctx.run_ui(
            input_with(vec![Event::PointerMoved(Pos2::new(899.0, 799.0))]),
            |ui| {
                let _ = show(ui, body_rect(), columns(), &row, &tokens);
            },
        );
        focused.textures_delta.clear();
        assert!(
            painted_into(&focused, like),
            "the focused row revealed nothing, so the focus is not the row's"
        );
        assert!(
            !drawn_texts(&focused).contains(&"1"),
            "the focused row kept its number: {:?}",
            drawn_texts(&focused)
        );

        let carried = Row {
            look: Look {
                carrying: true,
                ..Look::default()
            },
            ..row_of(1)
        };
        let mut output = ctx.run_ui(input(), |ui| {
            let _ = show(ui, body_rect(), columns(), &carried, &tokens);
        });
        output.textures_delta.clear();
        assert!(
            drawn_texts(&output).contains(&"1"),
            "a carried row holding the focus gave its number to the play glyph: {:?}",
            drawn_texts(&output)
        );
        assert!(
            !painted_into(&output, like),
            "a carried row holding the focus still drew its like"
        );
    }

    #[test]
    fn a_lifted_snapshot_paints_its_own_row_and_the_block_figures() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let row = row_of(4);
        let lifted = row.lifted(columns());
        let rect = Rect::from_min_size(
            Pos2::new(120.0, 90.0),
            vec2(900.0, crate::theme::extent::ROW_H),
        );

        let mut output = ctx.run_ui(input(), |ui| {
            lifted.paint(
                ui,
                rect,
                Ghosting {
                    count: 4,
                    total: Some("21:30"),
                    flat: false,
                },
                &tokens,
            );
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);

        for label in ["Harbour Wall", "Slow Tide", "Record", "4", "×4", "21:30"] {
            assert!(
                texts.contains(&label),
                "the ghost is missing {label}: {texts:?}"
            );
        }
        assert!(
            !texts.contains(&"3:42"),
            "a block's running time replaces one row's length: {texts:?}"
        );
    }

    #[test]
    fn a_block_with_no_total_paints_no_duration() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let lifted = row_of(4).lifted(columns());
        let rect = Rect::from_min_size(
            Pos2::new(120.0, 90.0),
            vec2(900.0, crate::theme::extent::ROW_H),
        );

        let mut output = ctx.run_ui(input(), |ui| {
            lifted.paint(
                ui,
                rect,
                Ghosting {
                    count: 3,
                    total: None,
                    flat: false,
                },
                &tokens,
            );
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);

        assert!(
            !texts.contains(&"3:42"),
            "a block with no honest total paints none, not one row's length: {texts:?}"
        );
        assert!(texts.contains(&"×3"), "the block still says how many it is");
    }

    /// An absent column is a zero-width rect, and a line elided to nothing still
    /// lays out an ellipsis, so a lifted row must guard each column it paints.
    #[test]
    fn a_lifted_snapshot_paints_nothing_into_a_column_its_table_omits() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let present = crate::layout::table::Present {
            artist: false,
            album: false,
            ..crate::layout::Table::Full.columns()
        };
        let columns = crate::layout::table::columns(900.0, present);
        assert_eq!(columns.artist.width(), 0.0);
        assert_eq!(columns.album.width(), 0.0);

        let lifted = row_of(4).lifted(columns);
        let rect = Rect::from_min_size(
            Pos2::new(120.0, 90.0),
            vec2(900.0, crate::theme::extent::ROW_H),
        );
        let mut output = ctx.run_ui(input(), |ui| {
            lifted.paint(ui, rect, Ghosting::default(), &tokens);
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);

        for label in ["Harbour Wall", "4", "3:42"] {
            assert!(
                texts.contains(&label),
                "the ghost is missing {label}: {texts:?}"
            );
        }
        for absent in ["…", "Slow Tide", "Record"] {
            assert!(
                !texts.contains(&absent),
                "the ghost drew {absent} into a column its table omits: {texts:?}"
            );
        }
    }
}
