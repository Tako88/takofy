//! Placeholders for what has not arrived.
//!
//! A skeleton holds the shape of the thing it stands in for: the card's square
//! art and the two lines under it, the tracklist's own row with its index,
//! title, album and duration gutters. Nothing moves when the content lands,
//! because the box it lands in is the box that was reserved.
//!
//! The ink is the type's own bone at 9%, with one band of light crossing it —
//! that band is the whole animation budget. It says "still arriving" rather than
//! "frozen", and it stops with the last placeholder drawn: the walk asks for a
//! repaint only while a skeleton is on screen.

use std::time::Duration;

use egui::{Mesh, Pos2, Rect, Sense, Shape, Ui, Vec2, pos2, vec2};

use crate::layout::cards;
use crate::layout::chips;
use crate::layout::episode;
use crate::layout::table::{self, Present};
use crate::theme::{Tokens, eq, extent, motion, radius, space};

use super::media;

/// How many rows a page holds open when it does not know the count yet.
pub const SCREENFUL: usize = 8;

/// The most rows a page will hold open for a count it does know. A library
/// playlist can name hundreds, and a placeholder is not worth more than a few
/// screens of a bar that will never be read.
pub const MAX_ROWS: usize = 200;

/// How long a placeholder waits between frames: two of the animation clock's
/// own, so the sheen moves with the pulse rather than at a rate of its own.
const WAKE: Duration = Duration::from_millis((2.0 * eq::FRAME * 1_000.0) as u64);

/// The rows a loading list holds open: the count when it is known, one screenful
/// when it is not, and never past what is worth drawing.
pub fn row_count(known: Option<usize>) -> usize {
    known.unwrap_or(SCREENFUL).clamp(1, MAX_ROWS)
}

/// How long after the load began this placeholder's sheen sets off. Four apart
/// is enough that a row reads as several arriving, not as one sheet blinking.
pub fn stagger(index: usize) -> f32 {
    const STEPS: [f32; 4] = [0.0, 1.0, 2.0, 3.0];
    STEPS[index % STEPS.len()] * motion::STAGGER
}

/// Where the light sits across a placeholder at `time` seconds: `0.0` at its
/// left edge, `1.0` at its right, wrapping once per `motion::SHEEN`.
///
/// The light is off the box for part of the cycle, which is what gives the row
/// a pause rather than a continuous wipe.
pub fn sheen(time: f64, delay: f32) -> f32 {
    let elapsed = time.max(0.0) as f32 - delay;
    if elapsed < 0.0 {
        return 0.0;
    }
    let period = motion::SHEEN;
    ((elapsed % period) / period).clamp(0.0, 1.0)
}

/// The two line widths of a card at `index`, each a fraction of the line's own
/// box.
///
/// The widths vary with position in the grid rather than with the item, because
/// the item is exactly what is not known yet: a placeholder that is always the
/// same width reads as a pattern, and one that varies too much reads as content.
pub fn card_lines(index: usize) -> (f32, f32) {
    let n = index + 1;
    let name = if n.is_multiple_of(2) {
        0.54
    } else if n.is_multiple_of(3) {
        0.78
    } else {
        0.60
    };
    let sub = if n.is_multiple_of(2) {
        0.34
    } else if n.is_multiple_of(3) {
        0.46
    } else {
        0.44
    };
    (name, sub)
}

/// The title width of a row at `index`, as a fraction of the title box.
pub fn row_title(index: usize) -> f32 {
    const WIDTHS: [f32; 6] = [0.52, 0.41, 0.63, 0.34, 0.47, 0.58];
    WIDTHS[index % WIDTHS.len()]
}

/// Marks that a placeholder is on screen for one more frame.
///
/// A placeholder that did not move would be indistinguishable from a stuck
/// window, so the sheen asks for a frame about thirty times a second. It is the
/// same cost the spinner paid and it stops with the placeholder.
fn wake(ui: &Ui) {
    ui.ctx().request_repaint_after(WAKE);
}

fn time(ui: &Ui) -> f64 {
    ui.input(|input| input.time)
}

/// One bar of held ink, centred on `centre_y`.
fn bar(ui: &Ui, left: f32, width: f32, centre_y: f32, height: f32, tokens: &Tokens) {
    if width <= 0.0 {
        return;
    }
    let rect = Rect::from_min_size(pos2(left, centre_y - height / 2.0), vec2(width, height));
    ui.painter()
        .rect_filled(rect, radius::XS, tokens.palette.skeleton_rule);
}

/// The light, as a band whose edges fade into the placeholder.
///
/// A mesh rather than a stack of rects: the fade is what keeps a sheen from
/// looking like a second, brighter bar travelling across the first.
fn light(ui: &Ui, rect: Rect, progress: f32, tokens: &Tokens) {
    if rect.width() <= 0.0 {
        return;
    }
    let band = (rect.width() * 0.45).max(1.0);
    let travel = rect.width() + band;
    let left = rect.left() - band + travel * progress;
    let span = [left, left + band / 2.0, left + band];
    let colours = [
        tokens.palette.skeleton_sheen.gamma_multiply(0.0),
        tokens.palette.skeleton_sheen,
        tokens.palette.skeleton_sheen.gamma_multiply(0.0),
    ];

    let mut mesh = Mesh::default();
    for (x, colour) in span.iter().zip(colours) {
        let x = x.clamp(rect.left() - band, rect.right() + band);
        mesh.colored_vertex(pos2(x, rect.top()), colour);
        mesh.colored_vertex(pos2(x, rect.bottom()), colour);
    }
    for pair in 0..2u32 {
        let base = pair * 2;
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base + 1, base + 2, base + 3);
    }

    let painter = ui.painter().with_clip_rect(rect);
    painter.add(Shape::mesh(mesh));
}

/// One card: the square its cover will fill, and the two lines under it.
///
/// The box is the tile's own, so the sheen crosses the art and nothing below it
/// moves when the cover arrives.
pub fn tile(ui: &Ui, rect: Rect, index: usize, tokens: &Tokens) {
    wake(ui);
    let inner = rect.shrink(space::S2);
    let art = Rect::from_min_size(inner.min, Vec2::splat(inner.width()));
    if art.width() > 0.0 {
        ui.painter()
            .rect_filled(art, radius::SM, tokens.palette.skeleton);
        light(ui, art, sheen(time(ui), stagger(index)), tokens);
    }

    let (name, sub) = card_lines(index);
    let bottom = art.bottom() + media::extra_height(ui);
    bar(
        ui,
        inner.left(),
        inner.width() * name,
        art.bottom() + space::S2 + extent::SKELETON_LINE / 2.0,
        extent::SKELETON_LINE,
        tokens,
    );
    bar(
        ui,
        inner.left(),
        inner.width() * sub,
        bottom - extent::SKELETON_LINE_SM / 2.0,
        extent::SKELETON_LINE_SM,
        tokens,
    );
}

/// The cards a loading library holds open.
///
/// The grid is solved by the caller that owns the density, so a skeleton sits in
/// exactly the box the item will: the same columns, the same gap, the same
/// height.
pub fn tiles(ui: &Ui, solved: &cards::Grid, count: usize, tokens: &Tokens) {
    for index in 0..count {
        tile(ui, solved.card_rect(index), index, tokens);
    }
}

/// One tracklist row, cut to the same columns the resolved row will use.
///
/// `art` says whether the resolved row nests an art box in its title, so the
/// placeholder's name starts where the real name will and the page does not
/// shift when the list lands.
fn row_at(ui: &Ui, rect: Rect, index: usize, present: Present, art: bool, tokens: &Tokens) {
    wake(ui);
    let columns = table::columns(rect.width(), present).at(rect.min);
    let number = table::number_box(columns.index);
    bar(
        ui,
        number.right() - number.width() * 0.6,
        number.width() * 0.6,
        number.center().y,
        extent::SKELETON_LINE_SM,
        tokens,
    );

    // One centred line on every shape: no row stacks a credit under its name
    // any more, so a second bar would stand for a line nothing draws.
    let text = table::title_parts(columns.title, art).text;
    let title = Rect::from_min_size(
        pos2(text.left(), text.center().y - extent::SKELETON_LINE / 2.0),
        vec2(text.width() * row_title(index), extent::SKELETON_LINE),
    );
    ui.painter()
        .rect_filled(title, radius::XS, tokens.palette.skeleton_rule);
    light(ui, title, sheen(time(ui), stagger(index)), tokens);

    for (column, width) in [
        (columns.album, 0.68),
        (columns.artist, 0.54),
        (columns.added, 0.62),
        (columns.duration, 0.6),
    ] {
        bar(
            ui,
            column.left(),
            column.width() * width,
            column.center().y,
            extent::SKELETON_LINE_SM,
            tokens,
        );
    }
}

/// The rows a loading tracklist holds open.
///
/// One allocation for the whole list, so the page below it stays where it is
/// and the scrollbar is the length of what is coming rather than of a bar. `art`
/// is the table's own: a list that will draw per-row covers reserves their box
/// in its title, and one that will not spends the whole title on the name.
pub fn rows(ui: &mut Ui, count: usize, present: Present, art: bool, tokens: &Tokens) {
    if count == 0 {
        return;
    }
    wake(ui);
    let width = ui.available_width();
    let (list, _) =
        ui.allocate_exact_size(vec2(width, count as f32 * extent::ROW_H), Sense::hover());
    for index in 0..count {
        let rect = Rect::from_min_size(
            Pos2::new(list.left(), list.top() + index as f32 * extent::ROW_H),
            vec2(width, extent::ROW_H),
        );
        row_at(ui, rect, index, present, art, tokens);
    }
}

/// The episode rows a loading show holds open.
///
/// An episode row is not a track row: a square toggle, a text column and a
/// length. The placeholder keeps all three, so the page does not re-flow when
/// the episodes land.
pub fn episode_rows(ui: &mut Ui, count: usize, tokens: &Tokens) {
    if count == 0 {
        return;
    }
    wake(ui);
    let width = ui.available_width();
    let (list, _) =
        ui.allocate_exact_size(vec2(width, count as f32 * episode::ROW_H), Sense::hover());
    for index in 0..count {
        let row = Rect::from_min_size(
            Pos2::new(list.left(), list.top() + index as f32 * episode::ROW_H),
            vec2(width, episode::ROW_H),
        );
        let parts = episode::row_parts(row, extent::TIME_W);
        let side = parts.toggle.width().min(parts.toggle.height());
        let toggle = Rect::from_min_size(parts.toggle.min, Vec2::splat(side));
        ui.painter()
            .rect_filled(toggle, radius::SM, tokens.palette.skeleton);

        let stack = extent::SKELETON_LINE + space::S2 + extent::SKELETON_LINE_SM;
        let top = parts.body.center().y - stack / 2.0;
        let title = Rect::from_min_size(
            pos2(parts.body.left(), top),
            vec2(parts.body.width() * row_title(index), extent::SKELETON_LINE),
        );
        ui.painter()
            .rect_filled(title, radius::XS, tokens.palette.skeleton_rule);
        light(ui, title, sheen(time(ui), stagger(index)), tokens);
        bar(
            ui,
            parts.body.left(),
            parts.body.width() * 0.34,
            top + extent::SKELETON_LINE + space::S2 + extent::SKELETON_LINE_SM / 2.0,
            extent::SKELETON_LINE_SM,
            tokens,
        );
        bar(
            ui,
            parts.duration.left(),
            parts.duration.width() * 0.6,
            parts.duration.center().y,
            extent::SKELETON_LINE_SM,
            tokens,
        );
    }
}

/// A band of compact rows: the boxes Home's counted tiles and station rows fill.
///
/// The placeholders are the boxes themselves, because that is what the shelf will
/// occupy: a waiting shelf has to hold the room its content will take or the
/// shelves under it move when the content lands.
pub fn compact_rows(ui: &mut Ui, count: usize, columns: usize, row_h: f32, tokens: &Tokens) {
    if count == 0 {
        return;
    }
    wake(ui);
    let width = ui.available_width();
    let placed = chips::grid(count, width, columns.max(1), row_h, space::S2);
    let (list, _) = ui.allocate_exact_size(vec2(width, placed.height), Sense::hover());
    for (index, at) in placed.rects.iter().enumerate() {
        let rect = at.translate(list.min.to_vec2());
        ui.painter()
            .rect_filled(rect, radius::SM, tokens.palette.skeleton);
        let left = rect.left() + space::S2 + extent::QUEUE_ART + space::S3;
        let bar = Rect::from_min_size(
            pos2(left, rect.center().y - extent::SKELETON_LINE_SM / 2.0),
            vec2(
                (rect.right() - space::S2 - left).max(0.0) * row_title(index),
                extent::SKELETON_LINE_SM,
            ),
        );
        ui.painter()
            .rect_filled(bar, radius::XS, tokens.palette.skeleton_rule);
    }
}

/// The band that crosses a track whose total is unknown.
///
/// Used where a span is being filled and the client cannot say by how much. It
/// claims no numerator on purpose: a progress bar that invents a total is a
/// claim the user can catch.
pub fn band(ui: &Ui, rect: Rect, tokens: &Tokens) {
    wake(ui);
    let progress = (time(ui) as f32 / motion::SHEEN).fract();
    let painter = ui.painter().with_clip_rect(rect);
    let width = rect.width() * 0.22;
    let left = rect.left() - width + (rect.width() + width) * progress;
    let band = Rect::from_min_size(pos2(left, rect.top()), vec2(width, rect.height()));
    painter.rect_filled(band, radius::PILL, tokens.palette.skeleton_rule);
}

#[cfg(test)]
mod tests;
