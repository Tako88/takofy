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
//! on the words they paint, but their interaction is deliberately not focusable,
//! so they add no stop of their own: the cost is that a keyboard can reach
//! neither link yet, while Enter and Space on the row still play.
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
use egui::{Color32, FontId, Pos2, Rect, Ui, vec2};

use crate::availability::Control;
use crate::cover::Cover;
use crate::icon::{self, Icon};
use crate::layout::table::{Columns, number_box, title_parts};
use crate::menu::{Anchor, Device, Door};
use crate::player::{Transport, TransportState};
use crate::route::Page;
use crate::selection::Press;
use crate::theme::icon::Size;
use crate::theme::{Tokens, eq, radius, space, stroke, text, tracking};

use super::link::{self, Face, Target};
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
    pub artist: &'a str,
    /// The first artist's page, when labelling resolved one. The artist cell
    /// links only when this is set and `artist` is non-empty.
    pub artist_uri: Option<&'a str>,
    pub album: Option<&'a str>,
    /// The album's page, when labelling resolved one. The cell links only when
    /// this is set and `album` is non-empty.
    pub album_uri: Option<&'a str>,
    pub added: Option<&'a str>,
    pub duration: Option<&'a str>,
    pub state: State,
    /// The track's art, as the art pipeline resolves it: `None` when this list
    /// draws no per-row art box, `Some` when it does.
    pub cover: Option<Cover>,
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
    number: usize,
    title: String,
    artist: String,
    album: Option<String>,
    added: Option<String>,
    duration: Option<String>,
    cover: Option<Cover>,
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

        let parts = title_parts(columns.title, self.cover.is_some());
        if let Some(cover) = self.cover
            && drawable(parts.art)
        {
            art::tile(
                ui.painter(),
                parts.art,
                radius::XS,
                tokens.palette.border,
                cover,
                tokens,
            );
        }
        if drawable(parts.text) {
            let top = vertically_centred(ui, parts.text, &recipe::track_name());
            self.paint_title(ui, parts.text, top, ghosting, title_ink, tokens);
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
/// reports neither hover nor a page.
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
            label(row.title, row.artist, row.album),
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
    if !row.look.carrying {
        controls::focus_ring(ui, response, rect, radius::SM, tokens);
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
    title(ui, columns.title, row, tokens);
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
    for link in [artist_link, album_link].into_iter().flatten() {
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
            .or_else(|| context_menu::pointer_door(ui, rect, response.id, columns.actions))
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

/// The row's accessibility label: the title, and the artist when it has one.
///
/// A slim row carries no credit, so "X by " would name nothing; the title alone
/// is what the row says.
fn label(title: &str, artist: &str, album: Option<&str>) -> String {
    if !artist.is_empty() {
        return format!("{title} by {artist}");
    }
    match album.filter(|album| !album.is_empty()) {
        Some(album) => format!("{title} on {album}"),
        None => title.to_owned(),
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
/// A list that draws per-row art nests the cover's box inside the title and
/// draws the name beside it; a list that does not spends the whole cell on the
/// name. Nothing here is a link: the credit cell is.
fn title(ui: &Ui, rect: Rect, row: &Row<'_>, tokens: &Tokens) {
    if !drawable(rect) {
        return;
    }
    title_cell(ui, rect, row, tokens);
}

/// The name, with the art box ahead of it only when the row carries art.
fn title_cell(ui: &Ui, rect: Rect, row: &Row<'_>, tokens: &Tokens) {
    let parts = title_parts(rect, row.cover.is_some());
    if let Some(cover) = row.cover
        && drawable(parts.art)
    {
        art::tile(
            ui.painter(),
            parts.art,
            radius::XS,
            tokens.palette.border,
            cover,
            tokens,
        );
    }

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

/// The artist column: a credit link.
///
/// A playlist and an album's plays table carry it.
fn artist(ui: &Ui, rect: Rect, row: &Row<'_>, key: (&str, usize), tokens: &Tokens) -> Option<Link> {
    credit_link(
        ui,
        rect,
        row.artist,
        "track-artist",
        row.artist_uri.map(Target::Credit),
        key,
        tokens,
    )
}

/// The album column: the credit on an artist's plays table, and a plain fact
/// on a playlist's rows.
fn album(ui: &Ui, rect: Rect, row: &Row<'_>, key: (&str, usize), tokens: &Tokens) -> Option<Link> {
    credit_link(
        ui,
        rect,
        row.album?,
        "track-album",
        row.album_uri.map(Target::Album),
        key,
        tokens,
    )
}

/// One credit cell, drawn under `name`: a link when the label is non-empty and
/// `target` names a page, plain text otherwise.
///
/// The cell is a link only when it paints words, so an empty cell gets no hit
/// area either. The hit area is the painted, elided words; the rest of the cell
/// belongs to the row.
fn credit_link(
    ui: &Ui,
    rect: Rect,
    label: &str,
    name: &'static str,
    target: Option<Target<'_>>,
    key: (&str, usize),
    tokens: &Tokens,
) -> Option<Link> {
    if !drawable(rect) || label.is_empty() {
        return None;
    }
    let face = Face {
        font: recipe::track_secondary(),
        tracking: 0.0,
    };
    let placed = link::Placed {
        pos: Pos2::new(rect.left(), vertically_centred(ui, rect, &face.font)),
        galley: link::lay(ui, label, &face, rect.width()),
    };
    let shown = placed.link(
        ui,
        ui.scope_id().with((name, key)),
        rect,
        target,
        tokens.palette.text_muted,
        tokens,
    );
    Some(Link {
        hovered: shown.hovered,
        page: shown.page,
    })
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
        controls::icon_button_on(
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

/// Lays a line out against the left edge of `rect` with its top at `top`.
fn place_left(ui: &Ui, rect: Rect, top: f32, label: &str, font: FontId) -> link::Placed {
    let face = Face {
        font,
        tracking: 0.0,
    };
    link::Placed {
        pos: Pos2::new(rect.left(), top),
        galley: link::lay(ui, label, &face, rect.width()),
    }
}

/// Paints a line against the left edge of `rect` with its top at `top`.
fn left_at(ui: &Ui, rect: Rect, top: f32, label: &str, font: FontId, color: Color32) {
    place_left(ui, rect, top, label, font).paint(ui, color);
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
mod tests;
