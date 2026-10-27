use egui::{Event, Modifiers};

use super::super::*;
use super::test_support::*;
use crate::theme::Tokens;
use crate::view::test_support::{button, click_events};

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
    let present = crate::layout::Table::Full.columns().for_slim();
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
