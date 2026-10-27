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
            egui::epaint::Shape::Text(text) => Some((text.galley.text().to_owned(), text.pos.x)),
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
