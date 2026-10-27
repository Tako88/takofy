//! The discography's filter row: the chips the artist's releases allow.
//!
//! The row is one control, not one per chip: `Control::DiscographyFilter` says
//! the row can act, and each chip is drawn inside its own id scope so that the
//! chips do not share one hover, one focus node or one AccessKit node — the
//! same reason a release block scopes itself.
//!
//! The row wraps with [`layout::chips::wrap`], the sidebar's rule, and reports
//! the height its rows took so the blocks below move down by exactly that much.

use egui::emath::GuiRounding;
use egui::{Pos2, Rect, Sense, Ui, Vec2};

use crate::availability::Control;
use crate::layout::chips;
use crate::layout::filter_row::{self, Beside};
use crate::theme::{Tokens, cluster, space};

use super::controls;

/// What sits at a row's trailing edge, and how much of the row it takes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RowSlot {
    /// A control group of `width` points, flush right, so the chips wrap before
    /// they reach it.
    Trailing(f32),
    /// A count and field cluster, whose reservation is `size`, flush right. The
    /// chips wrap in the room the field leaves.
    Cluster(Vec2),
}

/// A row's geometry: the slot it takes in the page's flow, and where each chip
/// lands inside it.
///
/// The row is laid out where it belongs and drawn wherever the page puts it, so
/// a row that pins can keep its slot reserved in the flow while its chips are
/// painted at the floor. Rects are in the page's coordinates, which is what the
/// page's own hit test and its debug grid already speak.
pub struct Laid<T> {
    /// The slot the row takes in the flow: every line of chips and the cluster.
    pub area: Rect,
    /// Each chip with the rect it is drawn into.
    pub chips: Vec<(T, Rect)>,
    /// The cluster's field, when the row has one.
    pub slot: Option<Rect>,
    /// The cluster's count, when it has room for one.
    pub count: Option<Rect>,
}

/// What drawing a row reported: the chip a press chose, the slot it took, and
/// the cluster's field when it has one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown<T> {
    /// The chip a press chose, if any.
    pub picked: Option<T>,
    /// The row's slot in the page's flow.
    pub area: Rect,
    /// The cluster's field, when the row has a cluster.
    pub slot: Option<Rect>,
}

/// The same row over any chip set: the copy, the control and the label are the
/// caller's, so the root's filter row is this one path and not a second row.
///
/// `label_of` returns a borrowed label; the caller owns the text and decides
/// whether it is upper-cased by the chip.
pub fn lay_out<T: Copy + PartialEq>(
    ui: &mut Ui,
    tokens: &Tokens,
    row: &[T],
    label_of: impl Fn(T) -> &'static str,
    slot: RowSlot,
) -> Laid<T> {
    let sizes: Vec<Vec2> = row
        .iter()
        .map(|chip| controls::chip_size(ui, tokens, label_of(*chip)))
        .collect();
    // A row is as tall as its tallest chip: unlike the sidebar, the page's row
    // is the chips and nothing else.
    let row_h = sizes.iter().map(|size| size.y).fold(0.0, f32::max);
    let width = ui.available_width();
    let (wrapped, cluster, height) = match slot {
        RowSlot::Cluster(size) => {
            let count_w = (size.x > cluster::FIELD_W + cluster::GAP)
                .then_some(size.x - cluster::FIELD_W - cluster::GAP);
            let Beside {
                chips: rects,
                cluster,
                height,
            } = filter_row::beside(&sizes, width, row_h, count_w);
            let wrapped = chips::Wrapped {
                rects,
                height: height.max(row_h),
            };
            (wrapped, Some(cluster), height.max(row_h))
        }
        RowSlot::Trailing(reserved) => {
            let wrapped = chips::wrap(
                &sizes,
                (width - reserved).max(0.0),
                space::S1,
                row_h,
                space::S1,
            );
            let height = wrapped.height.max(row_h);
            (wrapped, None, height)
        }
    };
    let (area, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());

    // Snapped to the pixel grid: a hairline on a half pixel is soft, and an
    // unrounded widget rect is what egui's debug overlay flags.
    let chips = row
        .iter()
        .copied()
        .zip(wrapped.rects)
        .map(|(chip, rect)| (chip, rect.translate(area.min.to_vec2()).round_ui()))
        .collect();
    let (slot, count) = match cluster {
        Some(cluster) => (
            Some(cluster.field.translate(area.min.to_vec2()).round_ui()),
            cluster
                .count
                .map(|count| count.translate(area.min.to_vec2()).round_ui()),
        ),
        None => (None, None),
    };
    Laid {
        area,
        chips,
        slot,
        count,
    }
}

/// Draws a laid-out row with its top-left corner at `origin`.
///
/// A pinned row keeps its slot in the flow and moves only its ink, so every chip
/// shifts by the same delta and the scroll extent does not change because the
/// row pinned.
pub fn draw_at<T: Copy + PartialEq>(
    ui: &mut Ui,
    tokens: &Tokens,
    laid: &Laid<T>,
    active: T,
    control_of: impl Fn(T) -> Control,
    label_of: impl Fn(T) -> &'static str,
    origin: Pos2,
) -> Option<T> {
    let shift = origin - laid.area.min;
    let mut picked = None;
    for (chip, rect) in &laid.chips {
        let rect = rect.translate(shift).round_ui();
        let label = label_of(*chip);
        let slot = controls::chip_in_keyed(
            ui,
            tokens,
            control_of(*chip),
            label,
            rect,
            label,
            *chip == active,
        );
        if slot.response.clicked() {
            picked = Some(*chip);
        }
    }
    picked
}

/// Draws the row in its own slot and leaves the step the reference keeps under
/// it.
pub fn show_with<T: Copy + PartialEq>(
    ui: &mut Ui,
    tokens: &Tokens,
    row: &[T],
    active: T,
    control_of: impl Fn(T) -> Control,
    label_of: impl Fn(T) -> &'static str,
    slot: RowSlot,
) -> Shown<T> {
    let laid = lay_out(ui, tokens, row, &label_of, slot);
    let picked = draw_at(
        ui,
        tokens,
        &laid,
        active,
        &control_of,
        &label_of,
        laid.area.min,
    );
    ui.add_space(space::S4);
    Shown {
        picked,
        area: laid.area,
        slot: laid.slot,
    }
}

#[cfg(test)]
mod tests;
