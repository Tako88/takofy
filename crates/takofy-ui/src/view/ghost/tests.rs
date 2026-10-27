use egui::{Color32, Pos2, RawInput, Rect, Shape, Vec2, pos2, vec2};

use super::*;
use crate::cover::Cover;
use crate::landing::{Landing, Slot};
use crate::theme::{extent, style};
use crate::view::queue_row;

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(crate::theme::fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx
}

fn carry() -> Carry {
    Carry {
        source: super::super::carry::Source::Queue,
        rows: vec![0],
        entries: Vec::new(),
        size: vec2(200.0, extent::ROW_H),
        ghost: Ghost::Queue(queue_row::Lifted::new(
            "A Title".to_owned(),
            "A Band".to_owned(),
            None,
            Cover::Missing,
        )),
        playlist: None,
    }
}

fn slot() -> Slot {
    Slot {
        row: 1,
        rect: Rect::from_min_size(pos2(40.0, 100.0), vec2(200.0, extent::ROW_H)),
    }
}

fn landing() -> Landing {
    Landing {
        boundary: 2,
        marker: Rect::from_min_size(pos2(40.0, 160.0), vec2(200.0, stroke::RULE)),
    }
}

/// Paints one frame of the ghost and answers what it drew.
fn painted(target: Option<Target>) -> Vec<Shape> {
    let ctx = context();
    let tokens = Tokens::DARK;
    let carry = carry();
    let pointer = pos2(20.0, 60.0);
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 400.0))),
            ..Default::default()
        },
        |ui| {
            paint(
                ui,
                Paint {
                    carry: &carry,
                    target,
                    pointer,
                    tokens: &tokens,
                },
            );
        },
    );
    output.textures_delta.clear();
    output
        .shapes
        .into_iter()
        .map(|clipped| clipped.shape)
        .collect()
}

fn fills(shapes: &[Shape], color: Color32) -> Vec<Rect> {
    shapes
        .iter()
        .filter_map(|shape| match shape {
            Shape::Rect(rect) if rect.fill == color => Some(rect.rect),
            _ => None,
        })
        .collect()
}

fn rules(shapes: &[Shape]) -> Vec<Rect> {
    let tokens = Tokens::DARK;
    shapes
        .iter()
        .filter_map(|shape| match shape {
            Shape::Rect(rect)
                if rect.fill == tokens.palette.mark
                    && (rect.rect.height() - stroke::RULE).abs() < 0.01 =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn an_add_paints_the_row_fill_and_no_rule() {
    let tokens = Tokens::DARK;
    let shapes = painted(Some(Target::Add(slot())));

    assert!(
        fills(&shapes, tokens.palette.row_selected).contains(&slot().rect),
        "the row under the pointer takes the fill"
    );
    assert!(rules(&shapes).is_empty(), "an add draws no rule");
}

#[test]
fn a_rootlist_move_paints_the_rule_and_no_fill() {
    let tokens = Tokens::DARK;
    let shapes = painted(Some(Target::Rootlist(landing())));

    assert_eq!(rules(&shapes), vec![landing().marker]);
    assert!(
        fills(&shapes, tokens.palette.row_selected).is_empty(),
        "a reorder draws no fill"
    );
}

#[test]
fn a_refused_target_paints_neither() {
    let tokens = Tokens::DARK;
    let shapes = painted(None);

    assert!(rules(&shapes).is_empty());
    assert!(fills(&shapes, tokens.palette.row_selected).is_empty());
    assert!(
        fills(&shapes, tokens.palette.surface)
            .iter()
            .any(|rect| *rect == ghost_box(pos2(20.0, 60.0), carry().size)),
        "a refused target draws the card flat"
    );
}

/// The box the card is drawn in, offset from the pointer.
fn ghost_box(pointer: Pos2, size: Vec2) -> Rect {
    Rect::from_min_size(
        pointer + vec2(space::S3, space::S4),
        vec2((size.x - space::S3).max(0.0), size.y),
    )
}
