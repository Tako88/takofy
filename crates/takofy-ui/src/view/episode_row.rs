//! One episode row: the play toggle, the name over its description, and the length.
//!
//! The row is handed its data and its rect; it fetches nothing and decides no
//! width. The reference draws an episode as a small square toggle where a track
//! row puts its art, then a text column that wraps to two rows, then the length.
//!
//! A row's identity is its URI and its position in the list. A show cannot list
//! one episode twice today, but the position costs nothing and keeps the row's
//! contract the same as every other list's.
//!
//! Its body is a click target and a drag source both. A playable row plays its
//! episode from the toggle or a double click; a single click picks the row, and a
//! drag lifts it, so scanning the list with the pointer never starts playback. A
//! row that only opens its show draws no toggle and is asked for no play.
//!
//! The byline under the description names the show, and that name is a link to
//! the show; the date beside it is plain.

use egui::emath::GuiRounding;
use egui::{FontId, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo};

use crate::cover::Cover;
use crate::format::EpisodeMeta;
use crate::icon::{self, Icon};
use crate::layout::episode::row_parts;
use crate::theme::icon::Size;
use crate::theme::{
    MonoWeight, Tokens, ease, extent, fonts, motion, radius, space, stroke, text, tracking,
};

use super::controls;
use super::link::{self, Face, Target};
use super::pin;
use super::queue_row;
use super::rows::{self, Look};
use super::text as recipe;
use super::track_row::{Click, State};

/// What one row draws.
pub struct Row<'a> {
    pub uri: &'a str,
    /// The episode's position in the list, half of its identity.
    pub index: usize,
    pub title: &'a str,
    /// The episode's description, already collapsed to one line; absent draws none.
    pub description: Option<&'a str>,
    /// The show's name, as the byline links it.
    pub show: &'a str,
    /// The show's URI, when known; without one the byline is one plain line.
    pub show_uri: Option<&'a str>,
    /// How long ago the episode was published, when known; the date stays plain.
    pub age: &'a str,
    pub duration: Option<&'a str>,
    /// Whether the row plays its own episode.
    ///
    /// A shelf whose rows only open their show passes `false`: the row then draws
    /// no toggle and answers no play, so nothing it cannot do is announced.
    pub playable: bool,
    pub state: State,
    /// Where the row stands in its list's selection, and in a carry.
    pub look: Look,
    /// Whether this row is the one a just-finished edit landed on.
    pub refocus: bool,
}

impl Row<'_> {
    /// The byline as one line, which the ghost and the spoken label print.
    fn joined(&self) -> String {
        EpisodeMeta {
            show: Some(self.show.to_owned()),
            age: Some(self.age.to_owned()),
        }
        .joined()
    }

    /// The row as it is drawn in flight: owned, and free of interaction.
    ///
    /// An episode has no cover anywhere in its row, so its ghost carries the
    /// missing-cover slot rather than a picture the page never fetched.
    pub fn lifted(&self) -> queue_row::Lifted {
        queue_row::Lifted::new(
            self.title.to_owned(),
            self.joined(),
            self.duration.map(str::to_owned),
            Cover::Missing,
        )
    }
}

/// Draws one row into `rect` and reports what the pointer or the keyboard asked
/// for.
///
/// A playable row's toggle asks for one episode, and a double click anywhere else
/// on it asks for the same; one that is not playable asks for neither. A single
/// click only picks the row, so scanning a list does not start playback by
/// accident.
pub fn show(ui: &mut Ui, rect: Rect, row: &Row<'_>, tokens: &Tokens) -> Option<Click> {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return None;
    }
    let key = (row.uri, row.index);
    let body = rows::body(ui, rect, id(ui, key), row.look);
    let response = &body.response;
    response.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, spoken(row)));
    if row.refocus {
        // A row an edit landed on takes the keyboard with it.
        response.request_focus();
    }

    let hover = rows::fade(
        ui,
        ui.scope_id().with(("episode-row-hover", key)),
        response.hovered() || response.has_focus(),
        row.look,
    );
    let before = rows::dim(ui, row.look);
    rows::wash(ui, rect, hover, row.look, tokens);
    let (played, toggle, byline) = paint(ui, rect, row, key, tokens);
    // A ring on a carried row is noise: the pointer already holds it. After the
    // hairline, so the row's own separator cannot cover the ring's bottom edge.
    if !row.look.carrying {
        controls::focus_ring(ui, response, rect, radius::SM, tokens);
    }
    ui.set_opacity(before);

    // Keyboard activation is answered before the pointer's, so a click landing in
    // the same frame can never swallow it.
    let keyboard = row.playable
        && response.has_focus()
        && ui.input(|input| input.key_pressed(egui::Key::Enter));
    if keyboard || played {
        return Some(Click::Play);
    }
    // A click on the byline's words opens the show and is never the row's own.
    let hovered = byline.as_ref().is_some_and(|shown| shown.hovered);
    if let Some(page) = byline.and_then(|shown| shown.page) {
        return Some(Click::Open(page));
    }
    // A double click is a second way to play the row, but not over the toggle:
    // the toggle has already answered its own click, and counting the same
    // press twice would restart the episode it just started. `hovered` is a
    // belt-and-braces guard, not the mechanism: egui gives the click to the hit
    // widget, so a link leaves the row no double click to answer.
    let over_toggle = toggle
        .zip(response.interact_pointer_pos())
        .is_some_and(|(rect, at)| rect.contains(at));
    if row.playable && response.double_clicked() && !over_toggle && !hovered {
        return Some(Click::Play);
    }
    if body.lift {
        return Some(Click::Lift);
    }
    if over_toggle || hovered {
        return None;
    }
    body.press.map(Click::Select)
}

/// The id one episode's body is drawn under, so a page can find the row the
/// keyboard is on before the clip culls it.
pub(super) fn id(ui: &Ui, key: (&str, usize)) -> Id {
    ui.scope_id().with(("episode-row", key))
}

/// Draws the row's hairline, its length, its toggle and its text, and reports
/// whether the toggle was clicked with the rect it answered in, and what the
/// byline's link drew. A row that is not playable draws no toggle and reports
/// none, so a press there is the row's own.
fn paint(
    ui: &mut Ui,
    rect: Rect,
    row: &Row<'_>,
    key: (&str, usize),
    tokens: &Tokens,
) -> (bool, Option<Rect>, Option<link::Shown>) {
    // The reference closes every row with a hairline rather than drawing a box.
    ui.painter().rect_filled(
        Rect::from_min_size(
            Pos2::new(rect.left(), rect.bottom() - stroke::HAIRLINE),
            Vec2::new(rect.width(), stroke::HAIRLINE),
        ),
        0.0,
        tokens.palette.border,
    );

    let duration = row.duration.map(|label| {
        recipe::spaced(
            ui,
            &label.to_uppercase(),
            FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
            tokens.palette.text_subtle,
            recipe::tracking_pts(text::XXS, tracking::WIDE),
        )
    });
    let duration_w = duration.as_ref().map_or(0.0, |line| line.size().x);
    let parts = row_parts(rect, duration_w);

    let toggle = row.playable.then_some(parts.toggle);
    let played = toggle.is_some_and(|at| play_toggle(ui, at, row, key, tokens));

    let text_width = parts.body.width().min(measure(ui));
    let byline = if text_width > 0.0 {
        text_block(ui, parts.body, row, key, text_width, tokens)
    } else {
        None
    };
    if let Some(line) = duration {
        ui.painter().galley(
            Pos2::new(parts.duration.right() - line.size().x, parts.duration.top()),
            line,
            tokens.palette.text_subtle,
        );
    }
    (played, toggle, byline)
}

/// The bordered square that starts one episode.
fn play_toggle(
    ui: &mut Ui,
    rect: Rect,
    row: &Row<'_>,
    key: (&str, usize),
    tokens: &Tokens,
) -> bool {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return false;
    }
    let response = ui.interact(
        rect,
        ui.scope_id().with(("episode-toggle", key)),
        Sense::click(),
    );
    response.widget_info(|| {
        let label = if row.state == State::Playing {
            format!("Pause {}", row.title)
        } else {
            format!("Play {}", row.title)
        };
        WidgetInfo::labeled(egui::Role::Button, true, label)
    });
    if response.has_focus() {
        pin::reveal(ui, rect);
    }

    let hover = ui.ctx().animate_bool_with_time_and_easing(
        ui.scope_id().with(("episode-toggle-hover", key)),
        response.hovered() || response.has_focus(),
        motion::FAST,
        ease::out,
    );
    let current = row.state != State::Resting;
    let border = if current {
        tokens.palette.accent
    } else if hover > 0.0 {
        tokens.palette.text
    } else {
        tokens.palette.border_strong
    };
    ui.painter().rect_stroke(
        rect,
        radius::SM,
        Stroke::new(stroke::HAIRLINE, border),
        StrokeKind::Inside,
    );
    let glyph = if row.state == State::Playing {
        Icon::Pause
    } else {
        Icon::Play
    };
    let color = if current || hover > 0.0 {
        tokens.palette.accent
    } else {
        tokens.palette.text
    };
    icon::paint(
        ui.painter(),
        glyph,
        Rect::from_center_size(rect.center(), Vec2::splat(Size::BASE.side))
            .round_to_pixels(ui.pixels_per_point()),
        Size::BASE,
        color,
    );
    response.clicked()
}

/// The title, the clamped description and the byline, stacked from the top.
///
/// Returns what the byline's link drew, or `None` when it was painted plain.
fn text_block(
    ui: &Ui,
    rect: Rect,
    row: &Row<'_>,
    key: (&str, usize),
    width: f32,
    tokens: &Tokens,
) -> Option<link::Shown> {
    let mut top = rect.top();
    let title = recipe::line(
        ui,
        row.title,
        recipe::episode_title(),
        tokens.palette.text,
        0.0,
        width,
    );
    ui.painter().galley(
        Pos2::new(rect.left(), top),
        title.clone(),
        tokens.palette.text,
    );
    top += title.size().y;

    if let Some(description) = row.description.filter(|text| !text.is_empty()) {
        let galley = recipe::clamped(
            ui,
            description,
            recipe::episode_text(),
            tokens.palette.text_muted,
            0.0,
            width,
            2,
        );
        top += space::S2;
        ui.painter().galley(
            Pos2::new(rect.left(), top),
            galley.clone(),
            tokens.palette.text_muted,
        );
        top += galley.size().y;
    }

    let show = row.show.trim();
    let age = row.age.trim();
    // The show is linked even though this row only ever sits on that show's own
    // page: opening it changes nothing, because the history refuses an equal page.
    let uri = row
        .show_uri
        .filter(|_| !show.is_empty() && !row.look.carrying);
    let Some(uri) = uri else {
        let joined = row.joined();
        if joined.is_empty() {
            return None;
        }
        let line = recipe::line(
            ui,
            &joined.to_uppercase(),
            FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
            tokens.palette.text_subtle,
            recipe::tracking_pts(text::XXS, tracking::WIDE),
            width,
        );
        top += space::S2;
        ui.painter().galley(
            Pos2::new(rect.left(), top),
            line,
            tokens.palette.text_subtle,
        );
        return None;
    };

    let font = FontId::new(text::XXS, fonts::mono(MonoWeight::Medium));
    let spacing = recipe::tracking_pts(text::XXS, tracking::WIDE);
    let face = Face {
        font,
        tracking: spacing,
    };
    let galley = link::lay(ui, &show.to_uppercase(), &face, width);
    top += space::S2;
    let shown = link::Placed {
        pos: Pos2::new(rect.left(), top),
        galley,
    }
    .link(
        ui,
        ui.scope_id().with(("episode-show", key)),
        rect,
        Some(Target::Credit(uri)),
        tokens.palette.text_subtle,
        tokens,
    );
    // The date is plain and takes its room from what the name left: a date that
    // does not fit is dropped rather than left as a stump after an ellipsis.
    if !age.is_empty() {
        let date = recipe::spaced(
            ui,
            &format!(" · {age}").to_uppercase(),
            FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
            tokens.palette.text_subtle,
            spacing,
        );
        // The space before `·` is the leading step epaint adds to a non-first
        // glyph; a fresh galley's first glyph gets none, so it is added here.
        if shown.rect.right() + spacing + date.size().x <= rect.right() {
            ui.painter().galley(
                Pos2::new(shown.rect.right() + spacing, top),
                date,
                tokens.palette.text_subtle,
            );
        }
    }
    Some(shown)
}

/// The measure a description is allowed, `EPISODE_TEXT_CH` characters wide.
fn measure(ui: &Ui) -> f32 {
    let font = recipe::episode_text();
    let width = ui.ctx().fonts_mut(|fonts| fonts.glyph_width(&font, '0'));
    width * extent::EPISODE_TEXT_CH
}

/// What a screen reader says for the whole row.
fn spoken(row: &Row<'_>) -> String {
    let joined = row.joined();
    if joined.is_empty() {
        row.title.to_owned()
    } else {
        format!("{}, {joined}", row.title)
    }
}

#[cfg(test)]
mod tests;
