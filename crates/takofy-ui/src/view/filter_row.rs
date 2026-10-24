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
mod tests {
    use egui::emath::GuiRounding;

    use super::*;
    use crate::discography::{Chip, chips};
    use crate::format::filter_label;
    use takofy_model::{Release, ReleaseKind};

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

    fn release(kind: ReleaseKind) -> Release {
        Release {
            uri: format!("spotify:album:{kind:?}"),
            name: "Release".to_owned(),
            kind,
            year: None,
            cover: None,
            tracks: Vec::new(),
        }
    }

    fn row() -> Vec<Chip> {
        chips(&[
            release(ReleaseKind::Album),
            release(ReleaseKind::Ep),
            release(ReleaseKind::Single),
            release(ReleaseKind::Compilation),
        ])
    }

    fn frame(width: f32) -> egui::FullOutput {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::Vec2::new(width, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                let _ = show_with(
                    ui,
                    &Tokens::DARK,
                    &row(),
                    Chip::All,
                    |_| Control::DiscographyFilter,
                    filter_label,
                    RowSlot::Trailing(0.0),
                );
            },
        );
        output.textures_delta.clear();
        output
    }

    /// A row laid out at `width` with `slot`, plus the chip sizes it measured.
    fn laid_at(width: f32, slot: RowSlot) -> (Vec<Vec2>, Laid<Chip>) {
        let ctx = egui::Context::default();
        let tokens = Tokens::DARK;
        let mut sizes = Vec::new();
        let mut laid = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::Vec2::new(width, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                sizes = row()
                    .iter()
                    .map(|chip| controls::chip_size(ui, &tokens, filter_label(*chip)))
                    .collect();
                laid = Some(lay_out(ui, &tokens, &row(), filter_label, slot));
            },
        );
        output.textures_delta.clear();
        (sizes, laid.expect("the row is laid out"))
    }

    /// Paints the row's chips with the row's top-left at `origin`.
    fn drawn_at(origin: Pos2) -> egui::FullOutput {
        let ctx = egui::Context::default();
        let tokens = Tokens::DARK;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::Vec2::new(1200.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                let laid = lay_out(
                    ui,
                    &tokens,
                    &row(),
                    filter_label,
                    RowSlot::Cluster(filter_row::size(Some(70.0))),
                );
                let _ = draw_at(
                    ui,
                    &tokens,
                    &laid,
                    Chip::All,
                    |_| Control::DiscographyFilter,
                    filter_label,
                    origin,
                );
            },
        );
        output.textures_delta.clear();
        output
    }

    #[test]
    fn the_trailing_slot_is_unchanged() {
        let width = 1200.0;
        let reserved = 120.0;
        let (sizes, laid) = laid_at(width, RowSlot::Trailing(reserved));
        let row_h = sizes.iter().map(|size| size.y).fold(0.0, f32::max);
        let expected = chips::wrap(&sizes, width - reserved, space::S1, row_h, space::S1);
        assert_eq!(laid.slot, None, "a control group is not a field");
        assert!(
            (laid.area.height() - expected.height.max(row_h)).abs() < 0.01,
            "the trailing slot keeps the chip row's height: {} against {}",
            laid.area.height(),
            expected.height.max(row_h)
        );
        for ((_, rect), expected) in laid.chips.iter().zip(expected.rects) {
            let expected = expected.translate(laid.area.min.to_vec2()).round_ui();
            assert_eq!(*rect, expected);
        }
    }

    #[test]
    fn a_cluster_slot_returns_its_rect_and_the_chips_clear_it() {
        let (_, laid) = laid_at(1200.0, RowSlot::Cluster(filter_row::size(Some(70.0))));
        let field = laid.slot.expect("a cluster has a field");
        assert_eq!(field.width(), cluster::FIELD_W);
        assert_eq!(field.right(), laid.area.right());
        let count = laid.count.expect("the count has room at this width");
        for (chip, rect) in &laid.chips {
            assert!(
                !rect.intersects(field) && !rect.intersects(count),
                "{chip:?} runs into the cluster: {rect:?} against {field:?} and {count:?}"
            );
        }
    }

    #[test]
    fn draw_at_moves_every_chip_by_the_origin_delta() {
        let plain = drawn_at(Pos2::ZERO);
        let shifted = drawn_at(Pos2::new(0.0, 50.0));
        let labels = ["ALL", "ALBUMS", "EPS", "SINGLES", "COMPILATIONS"];
        for label in labels {
            let before = text_rect(&plain, label).expect("the chip is painted");
            let after = text_rect(&shifted, label).expect("the chip is painted again");
            assert!(
                (after.top() - before.top() - 50.0).abs() < 0.01
                    && (after.left() - before.left()).abs() < 0.01,
                "{label} did not move with the row: {before:?} to {after:?}"
            );
        }
    }

    #[test]
    fn the_chips_and_the_cluster_land_on_the_pixel_grid() {
        let (_, laid) = laid_at(1200.0, RowSlot::Cluster(filter_row::size(Some(70.0))));
        for rect in laid
            .chips
            .iter()
            .map(|(_, rect)| *rect)
            .chain(laid.slot)
            .chain(laid.count)
        {
            for edge in [rect.min.x, rect.min.y, rect.max.x, rect.max.y] {
                assert_eq!(edge, edge.round_ui(), "an edge is off the grid: {rect:?}");
            }
        }
        let output = drawn_at(Pos2::ZERO);
        assert!(
            !output.shapes.iter().any(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => text.galley.text() == "Unaligned",
                _ => false,
            }),
            "the row paints an unaligned widget"
        );
    }

    /// The chips are a row, not a stack: each label starts to the right of the
    /// previous one, and no chip's box overlaps the next.
    #[test]
    fn the_chips_sit_side_by_side() {
        let output = frame(1200.0);
        let labels: Vec<(String, f32)> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => {
                    Some((text.galley.text().to_owned(), text.pos.x))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            labels
                .iter()
                .map(|(label, _)| label.as_str())
                .collect::<Vec<_>>(),
            ["ALL", "ALBUMS", "EPS", "SINGLES", "COMPILATIONS"]
        );
        for pair in labels.windows(2) {
            assert!(
                pair[1].1 > pair[0].1,
                "{} starts at {}, {} at {}",
                pair[0].0,
                pair[0].1,
                pair[1].0,
                pair[1].1
            );
        }

        let boxes: Vec<egui::Rect> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape) if shape.fill == Tokens::DARK.palette.pressed => {
                    Some(shape.rect)
                }
                _ => None,
            })
            .collect();
        let mut right = f32::MIN;
        for rect in &boxes {
            assert!(rect.left() >= right, "a chip overlaps the one before it");
            right = rect.right();
        }
    }

    /// A chip on a half pixel is soft, and a fractional widget rect is what
    /// egui's debug overlay flags with its orange `Unaligned` marker — which sits
    /// inside the chip and makes the row unreadable in a debug build.
    #[test]
    fn the_chips_land_on_the_pixel_grid() {
        let output = frame(1200.0);
        assert!(
            !output.shapes.iter().any(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => text.galley.text() == "Unaligned",
                _ => false,
            }),
            "the row paints an unaligned widget"
        );
        for clipped in &output.shapes {
            if let egui::epaint::Shape::Rect(shape) = &clipped.shape
                && shape.stroke.width > 0.0
            {
                let rect = shape.rect;
                // The same criterion egui's debug overlay uses: every edge on
                // the GUI rounding grid, which is what keeps the hairline crisp.
                for edge in [rect.min.x, rect.min.y, rect.max.x, rect.max.y] {
                    assert_eq!(
                        edge,
                        edge.round_ui(),
                        "a chip edge is off the rounding grid: {rect:?}"
                    );
                }
            }
        }
    }
}
