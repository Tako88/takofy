use egui::Vec2;

use super::super::*;
use super::test_support::*;

use crate::view::test_support::click_events;

/// Wide enough that the rail stands aside, which is the only way the count
/// is drawn at all.
const WIDE: Vec2 = Vec2::new(1000.0, 400.0);

/// The field's own bottom hairline, its box and the ink it carries.
fn field_line(output: &egui::FullOutput) -> Option<(Rect, egui::Color32)> {
    let tokens = Tokens::DARK;
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape)
                if (shape.rect.height() - stroke::HAIRLINE).abs() < 0.01
                    && (shape.rect.width() - actionbar::FIELD_W).abs() < 0.01
                    && (shape.fill == tokens.palette.border
                        || shape.fill == tokens.palette.text) =>
            {
                Some((shape.rect, shape.fill))
            }
            _ => None,
        })
}

#[test]
fn the_field_is_drawn_in_the_bar_between_play_and_shuffle_in_tab_order() {
    let ctx = context();
    let mut rows = Rows::narrowed();
    let (frame, expected) =
        frame_probed(&ctx, VIEWPORT, 0.0, Vec::new(), Some(rows.filter()), |ui| {
            vec![
                controls::slot_id(ui, Control::HeroPlay, ()),
                ui.scope_id().with("playlist-filter"),
                controls::slot_id(ui, Control::HeroShuffle, ()),
                controls::slot_id(ui, Control::HeroLike, ()),
                controls::slot_id(ui, Control::HeroMore, ()),
            ]
        });

    // It is the row's own field, between the play tile and the tools.
    let tiles = placed(&frame, VIEWPORT);
    let field = tiles.field.expect("a wide row has a field");
    assert_eq!(field.width(), actionbar::FIELD_W);
    assert_eq!(field.height(), crate::theme::extent::BUTTON_LG);
    assert!(field.left() > tiles.play.right());
    assert!(field.right() <= tiles.shuffle.expect("shuffle").left());
    assert_eq!(
        text_at(&frame.output, "midnight").len(),
        1,
        "the row's one field paints the query"
    );
    assert_eq!(
        frame.reply.hits.as_deref(),
        Some(&[1][..]),
        "the drawn query is applied"
    );

    // Tab walks the row in the reference's DOM order: play, field, shuffle,
    // heart, more.
    let [press, release] = crate::view::test_support::tab();
    let mut reached = Vec::new();
    for _ in 0..expected.len() {
        let _ = frame_in(
            &ctx,
            VIEWPORT,
            0.0,
            vec![press.clone()],
            Some(rows.filter()),
        );
        reached.push(ctx.memory(|memory| memory.focused()));
        let _ = frame_in(
            &ctx,
            VIEWPORT,
            0.0,
            vec![release.clone()],
            Some(rows.filter()),
        );
    }
    let reached: Vec<&str> = reached
        .iter()
        .map(|id| match *id {
            None => "nothing",
            Some(id) if id == expected[0] => "play",
            Some(id) if id == expected[1] => "field",
            Some(id) if id == expected[2] => "shuffle",
            Some(id) if id == expected[3] => "heart",
            Some(id) if id == expected[4] => "more",
            Some(_) => "something else",
        })
        .collect();
    assert_eq!(
        reached,
        ["play", "field", "shuffle", "heart", "more"],
        "the field is a Tab stop between the play tile and the shuffle"
    );
}

#[test]
fn a_stuck_unfocused_field_paints_no_hairline_and_the_bar_edge_stands_in() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let mut rows = Rows::narrowed();

    // At rest the field is in the flow and wears its own line.
    let rested = frame_in(&ctx, VIEWPORT, 0.0, Vec::new(), Some(rows.filter()));
    let (line, ink) = field_line(&rested.output).expect("a resting field is underlined");
    assert_eq!(ink, tokens.palette.border);
    assert!(
        (line.bottom() - placed(&rested, VIEWPORT).field.expect("field").bottom()).abs() < 0.01,
        "the line is the field's bottom: {line:?}"
    );

    // Stuck and unfocused it paints none, and the row's own edge carries the
    // line instead.
    let pinned = frame_in(
        &ctx,
        VIEWPORT,
        FILLER + 50.0,
        Vec::new(),
        Some(rows.filter()),
    );
    assert_eq!(
        field_line(&pinned.output),
        None,
        "a stuck unfocused field paints no line of its own"
    );
    let bar = pinned.rect();
    let edge = fills(&pinned.output, tokens.palette.border)
        .into_iter()
        .find(|(_, rect)| {
            (rect.height() - stroke::HAIRLINE).abs() < 0.01
                && (rect.bottom() - bar.bottom()).abs() < 0.01
                && (rect.width() - bar.width()).abs() < 0.01
        })
        .map(|(_, rect)| rect)
        .expect("the stuck row wears its own edge");
    let field = placed(&pinned, VIEWPORT).field.expect("field");
    assert!(
        edge.width() > field.width(),
        "the edge is the row's, not the field's underline: {edge:?} against {field:?}"
    );

    // Under the keyboard the line returns, in full ink.
    for event in click_events(field.center()) {
        let _ = frame_in(
            &ctx,
            VIEWPORT,
            FILLER + 50.0,
            vec![event],
            Some(rows.filter()),
        );
    }
    let typed = frame_in(
        &ctx,
        VIEWPORT,
        FILLER + 50.0,
        Vec::new(),
        Some(rows.filter()),
    );
    assert_eq!(
        field_line(&typed.output).map(|(_, ink)| ink),
        Some(tokens.palette.text),
        "a focused field says so in full ink while the row is stuck"
    );
}

#[test]
fn the_count_sits_one_tools_gap_left_of_the_field() {
    let ctx = context();
    let mut rows = Rows::narrowed();
    let frame = frame_in(&ctx, WIDE, 0.0, Vec::new(), Some(rows.filter()));
    assert_eq!(
        frame.reply.hits.as_deref(),
        Some(&[1][..]),
        "one of the three rows matches"
    );

    let field = placed(&frame, WIDE).field.expect("a wide row has a field");
    let count = text_rect(&frame.output, "1 OF 3 TRACKS").expect("the count is painted");
    assert!(
        (count.right() - (field.left() - actionbar::TOOLS_GAP)).abs() < 0.01,
        "the count sits one tools gap left of the field: {count:?} against {field:?}"
    );
    assert!(
        (count.center().y - field.center().y).abs() < 0.01,
        "the count is on the field's own line: {count:?} against {field:?}"
    );

    // With no query the count goes, and the field says so with its
    // placeholder instead.
    let mut quiet = Rows::new();
    let frame = frame_in(&ctx, WIDE, 0.0, Vec::new(), Some(quiet.filter()));
    assert_eq!(frame.reply.hits, None);
    assert!(text_rect(&frame.output, "FILTER").is_some());
    assert_eq!(
        text_at(&frame.output, "3 OF 3 TRACKS").len(),
        0,
        "an unfiltered list paints no count"
    );
}

#[test]
fn the_status_node_is_polite_carries_the_unshortened_label_and_exists_only_while_filtering() {
    let ctx = context();
    ctx.enable_accesskit();
    let mut rows = Rows::narrowed();
    let (filtered, read) = frame_probed(&ctx, WIDE, 0.0, Vec::new(), Some(rows.filter()), |ui| {
        ui.ctx()
            .accesskit_node_builder(ui.scope_id().with("track-count"), |node| {
                (node.role(), node.live(), node.label().map(str::to_owned))
            })
            .expect("the count has a status node")
    });
    assert_eq!(read.0, egui::Role::Status);
    assert_eq!(read.1, Some(egui::accesskit::Live::Polite));
    assert_eq!(
        read.2.as_deref(),
        Some("1 of 3 tracks"),
        "the node says the count in the field's own words, not the painted glyphs"
    );
    assert!(
        text_rect(&filtered.output, "1 OF 3 TRACKS").is_some(),
        "and the painted count is the upper-cased one"
    );

    let mut quiet = Rows::new();
    let (_, stray) = frame_probed(&ctx, WIDE, 0.0, Vec::new(), Some(quiet.filter()), |ui| {
        ui.ctx()
            .has_accesskit_node(ui.scope_id().with("track-count"))
    });
    assert!(!stray, "an unfiltered list announces nothing");
}

#[test]
fn a_bar_with_no_room_for_the_field_applies_no_query() {
    let ctx = context();
    // Narrow enough that the field has less than its floor between the play
    // tile and the tools, so it is not drawn at all.
    let size = Vec2::new(310.0, 400.0);
    let mut rows = Rows::narrowed();
    let frame = frame_in(&ctx, size, 0.0, Vec::new(), Some(rows.filter()));

    assert!(
        placed(&frame, size).field.is_none(),
        "the field is not drawn in a row this narrow"
    );
    assert_eq!(
        frame.reply.hits, None,
        "a query nobody can see never narrows the list"
    );
    assert!(
        text_at(&frame.output, "midnight").is_empty()
            && text_at(&frame.output, "FILTER").is_empty(),
        "nor is there a field to paint it in: {:?}",
        text_at(&frame.output, "midnight")
    );
}
