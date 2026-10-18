//! One track row.
//!
//! The row is handed its data and the rects the table solved for it; it fetches
//! nothing and decides no width. What it decides is what a state looks like: the
//! number gives way to a play glyph under the pointer, the equaliser stands in
//! while this is the transport's track, and the like appears on hover or focus.
//!
//! The row is one focus stop. The like and the more are waiting controls, so they
//! never join the tab order, and a list of sixty tracks is sixty stops rather than
//! a hundred and eighty.
//!
//! A liked state is not drawn here because nothing can produce one yet: the track
//! model carries no such flag. The like's column is reserved and its control is
//! drawn waiting, so the row is laid out exactly where the reference puts it, and
//! the accent tone a liked heart takes arrives with the flag.

use egui::emath::GuiRounding;
use egui::{Color32, FontId, Pos2, Rect, Sense, Ui, vec2};

use crate::availability::Control;
use crate::icon::{self, Icon};
use crate::layout::table::{Columns, title_parts};
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
    pub album: Option<&'a str>,
    pub added: Option<&'a str>,
    pub duration: Option<&'a str>,
    pub state: State,
}

/// Draws one row into `rect`, and reports a double click on it.
///
/// `rect` is `layout::table::row_rect` for this row and `columns` is what
/// `layout::table::columns` solved for the same width, so a header and its rows
/// can never disagree about where a column is.
pub fn show(ui: &mut Ui, rect: Rect, columns: Columns, row: &Row<'_>, tokens: &Tokens) -> bool {
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
    let revealed = response.hovered() || response.has_focus();
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

    index(ui, columns.index, row, revealed, tokens);
    title(ui, columns.title, row, tokens);
    album(ui, columns.album, row.album, tokens);
    added(ui, columns.added, row.added, tokens);
    let on_like = like(ui, columns.like, key, revealed, tokens);
    duration(ui, columns.duration, row.duration, tokens);
    let on_more = more(ui, columns.actions, key, row, tokens);

    // A double click on a control inside the row is not a double click on the row,
    // or the like and the more would start playback as a side effect. Neither can
    // act yet, so neither takes the click; they only keep it.
    let on_control = on_like || on_more;
    let keyboard = response.has_focus()
        && ui.input(|input| {
            input.key_pressed(egui::Key::Enter) || input.key_pressed(egui::Key::Space)
        });
    (response.double_clicked() || keyboard) && !on_control
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
        right(
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

/// The title column: the cover's box, then the name over the artist.
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
        tokens,
    );

    let text_rect = parts.text;
    if !drawable(text_rect) {
        return;
    }
    let name = recipe::track_name();
    let name_h = line_height(ui, &name);
    let stacked = name_h + line_height(ui, &recipe::track_secondary());
    let top = text_rect.center().y - stacked / 2.0;
    left_at(ui, text_rect, top, row.title, name, tokens.palette.text);
    left_at(
        ui,
        text_rect,
        top + name_h,
        row.artist,
        recipe::track_secondary(),
        tokens.palette.text_muted,
    );
}

/// The album column, which an album's own tracklist does not carry.
fn album(ui: &Ui, rect: Rect, label: Option<&str>, tokens: &Tokens) {
    if !drawable(rect) {
        return;
    }
    if let Some(label) = label {
        left(
            ui,
            rect,
            label,
            recipe::track_secondary(),
            tokens.palette.text_muted,
        );
    }
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
