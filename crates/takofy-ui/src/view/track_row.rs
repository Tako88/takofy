//! One track row.
//!
//! The row is handed its data and the rects the table solved for it; it fetches
//! nothing and decides no width. What it decides is what a state looks like: the
//! number gives way to a play glyph under the pointer, the equaliser stands in
//! while this is the transport's track, and the like appears on hover or focus.
//!
//! The row is one focus stop. The like and the more are waiting controls, so they
//! never join the tab order, and a list of sixty tracks is sixty stops rather than
//! a hundred and eighty. The album and artist cells are links that take a single
//! click anywhere on the cell, but their interaction is deliberately not
//! focusable, so they add no stop of their own: the cost is that a keyboard can
//! reach neither link yet, while Enter and Space on the row still play.
//!
//! A liked state is not drawn here because nothing can produce one yet: the track
//! model carries no such flag. The like's column is reserved and its control is
//! drawn waiting, so the row is laid out exactly where the reference puts it, and
//! the accent tone a liked heart takes arrives with the flag.

use egui::emath::GuiRounding;
use egui::{Color32, FontId, Pos2, Rect, Sense, Ui, vec2};

use crate::availability::Control;
use crate::cover::Cover;
use crate::icon::{self, Icon};
use crate::layout::table::{Columns, number_box, title_parts};
use crate::route::Page;
use crate::theme::icon::Size;
use crate::theme::{Tokens, ease, eq, motion, radius, stroke, text};

use super::{art, controls, text as recipe};

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
}

impl State {
    /// Whether the transport is on this row at all.
    fn current(self) -> bool {
        !matches!(self, Self::Resting)
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
}

/// What a click on a row asked for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Click {
    /// Start the track.
    Play,
    /// Open the page its album or first artist cell links to.
    Open(Page),
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
    let response = ui.interact(rect, ui.scope_id().with(("track-row", key)), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!("{} by {}", row.title, row.artist),
        )
    });
    if response.has_focus() {
        // A row reachable by Tab has to come into view when it is reached. Only a
        // drawn row can hold focus, so this is about a row that is clipped rather
        // than one that was culled.
        response.scroll_to_me(None);
    }
    // The reference reveals the play glyph and the like on hover *and* on
    // focus-within, because a keyboard has no pointer.
    //
    // `hovered()` is ownership, not position: the album and artist cells are
    // interacted after this row, and egui marks only the top hit widget as
    // hovered, so the row loses the hover the moment the pointer enters a link
    // cell. `contains_pointer()` asks whether the pointer is in the row's rect
    // at all, which is what the highlight, the play glyph and the like follow.
    let revealed = response.hovered() || response.contains_pointer() || response.has_focus();
    let fill = ui.ctx().animate_bool_with_time_and_easing(
        ui.scope_id().with(("track-row-hover", key)),
        revealed,
        motion::FAST,
        ease::out,
    );

    if fill > 0.0 {
        ui.painter().rect_filled(
            rect,
            radius::SM,
            tokens.palette.row_hover.gamma_multiply(fill),
        );
    }
    if response.has_focus() {
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
    title(ui, columns.title, row, tokens);
    let artist_link = artist(ui, columns.artist, row, key, tokens);
    let album_link = album(ui, columns.album, row, key, tokens);
    added(ui, columns.added, row.added, tokens);
    let on_like = like(ui, columns.like, key, revealed, tokens);
    duration(ui, columns.duration, row.duration, tokens);
    let on_more = more(ui, columns.actions, key, row, tokens);

    // The row's own keyboard activation is answered before the link cells are
    // read, so a link click landing in the same frame can never swallow it.
    // (egui surrenders the row's focus on the frame a link's click is reported,
    // so the two cannot in fact both be true today; reading the keyboard first
    // keeps the play whatever the hit test does.) A double click on a control
    // inside the row is not a double click on the row, or the like and the more
    // would start playback as a side effect. Neither can act yet, so neither
    // takes the click; they only keep it.
    let on_control = on_like || on_more;
    let keyboard = response.has_focus()
        && ui.input(|input| {
            input.key_pressed(egui::Key::Enter) || input.key_pressed(egui::Key::Space)
        });
    if keyboard && !on_control {
        return Some(Click::Play);
    }

    // The link cells are interacted after the row, so on a hit-test tie the cell
    // wins the click and the row never sees the double click that would play. A
    // click on either cell is a navigation, and takes precedence over playback.
    let mut link_hovered = false;
    let mut open = None;
    for link in [artist_link, album_link].into_iter().flatten() {
        link_hovered |= link.hovered;
        open = open.or(link.page);
    }
    if let Some(page) = open {
        return Some(Click::Open(page));
    }

    // `link_hovered` is a belt-and-braces guard, not the mechanism: when a link
    // wins the hit test, the row's `double_clicked()` is already false because
    // egui tells only the hit widget, so the second click of a double click on a
    // link rides on the first click's navigation, which has already re-laid out
    // the page. The keyboard was answered above, before any link click could
    // take it.
    (response.double_clicked() && !link_hovered && !on_control).then_some(Click::Play)
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
    } else if row.state.current() {
        equaliser(ui, rect, row.state, tokens);
    } else {
        centred_line(
            ui,
            rect,
            &row.number.to_string(),
            recipe::track_mono(text::XXS),
            tokens.palette.text_subtle,
        );
    }
}

/// The equaliser: three bars, each a little behind the one before it.
fn equaliser(ui: &Ui, rect: Rect, state: State, tokens: &Tokens) {
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

/// The title column: the cover's box, then the name.
fn title(ui: &Ui, rect: Rect, row: &Row<'_>, tokens: &Tokens) {
    if !drawable(rect) {
        return;
    }
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
        page: response.clicked().then(|| Page::Artist(uri.to_owned())),
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
fn added(ui: &Ui, rect: Rect, label: Option<&str>, tokens: &Tokens) {
    if !drawable(rect) {
        return;
    }
    if let Some(label) = label {
        left(
            ui,
            rect,
            label,
            recipe::track_mono(text::XXS),
            tokens.palette.text_subtle,
        );
    }
}

/// The like, which appears under the pointer or on focus.
///
/// Returns whether the pointer is on it, so a click here is not taken for a click
/// on the row.
fn like(ui: &mut Ui, rect: Rect, key: (&str, usize), revealed: bool, tokens: &Tokens) -> bool {
    if !drawable(rect) || !revealed {
        return false;
    }
    controls::icon_button_keyed(
        ui,
        tokens,
        Control::TrackLike,
        key,
        rect,
        Icon::Heart,
        Size::SM,
        "Save to your library",
        // Saving a track needs a write to the Web API, which this build never
        // makes, so the control stands where the reference has it and cannot act.
        false,
    )
    .response
    .hovered()
}

/// The track's length, right-aligned in its column.
fn duration(ui: &Ui, rect: Rect, label: Option<&str>, tokens: &Tokens) {
    if !drawable(rect) {
        return;
    }
    if let Some(label) = label {
        right(
            ui,
            rect,
            label,
            recipe::track_mono(text::XS),
            tokens.palette.text_subtle,
        );
    }
}

/// The more button, which a later phase opens a menu from.
///
/// Returns whether the pointer is on it, as [`like`] does.
fn more(ui: &mut Ui, rect: Rect, key: (&str, usize), row: &Row<'_>, tokens: &Tokens) -> bool {
    if !drawable(rect) {
        return false;
    }
    controls::icon_button_keyed(
        ui,
        tokens,
        Control::TrackMore,
        key,
        rect,
        Icon::More,
        Size::SM,
        &format!("More options for {}", row.title),
        false,
    )
    .response
    .hovered()
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
    use super::*;

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
}
