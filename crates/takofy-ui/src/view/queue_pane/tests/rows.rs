use std::collections::HashSet;

use egui::{Pos2, Shape};

use super::super::*;
use super::test_support::*;
use crate::message::Command;
use crate::player::Transport;

#[test]
fn an_empty_queue_draws_the_caption_and_the_empty_line() {
    let ctx = context();
    let mut art = Art::default();
    let now = Instant::now();
    let (reply, output) = pass(&ctx, PANE, &Queue::default(), &mut art, now, Vec::new());

    assert_eq!(
        reply.zone.map(|zone| (zone.first, zone.count)),
        Some((0, 0))
    );
    assert_eq!(
        (reply.play, reply.press, reply.lift, reply.focused),
        (None, None, None, None)
    );
    assert_eq!((reply.menu, reply.remove), (None, None));
    let texts = drawn_texts(&output);
    assert!(texts.contains(&"NEXT IN QUEUE"), "{texts:?}");
    assert!(texts.contains(&EMPTY), "{texts:?}");
}

#[test]
fn the_last_entry_draws_the_empty_line() {
    let ctx = context();
    let mut art = Art::default();
    let queue = queue_of(3, 2);
    let now = Instant::now();
    let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());

    let texts = drawn_texts(&output);
    assert!(texts.contains(&EMPTY), "{texts:?}");
    assert!(painted_rows(&output).is_empty(), "nothing follows it");
}

#[test]
fn the_rows_are_drawn_in_order_starting_after_the_current_entry() {
    let ctx = context();
    let mut art = Art::default();
    let queue = queue_of(6, 1);
    let now = Instant::now();
    let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());

    assert_eq!(
        painted_rows(&output),
        vec![2, 3, 4, 5],
        "the pane must list exactly what follows the current entry"
    );
    assert!(
        !drawn_texts(&output).contains(&"t1"),
        "the current entry is the panel's own meta, not a row"
    );
}

#[test]
fn a_selected_row_wears_the_selection_fill() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let mut art = Art::default();
    let now = Instant::now();
    let mut queue = queue_of(4, 0);
    queue.press(2, Press::Replace);

    let (_, output) = pass(&ctx, PANE, &queue, &mut art, now, Vec::new());
    assert!(
        output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, Shape::Rect(shape) if shape.fill == tokens.palette.row_selected)
        }),
        "a picked row must wear the selection fill"
    );
}

#[test]
fn a_row_that_only_touches_the_fold_is_not_laid_out() {
    // Two whole rows fit, so the third only shares the fold's edge. It is
    // not on screen, and a cover fetched for it would be a cover nothing
    // draws.
    let ctx = context();
    let tokens = Tokens::DARK;
    let now = Instant::now();
    let mut art = Art::default();
    let tracks: Vec<TrackEntry> = (0..20).map(track_with_cover).collect();
    let mut queue = Queue::default();
    queue.start(&tracks, 0, None, &mut Transport::default());

    let pane = Rect::from_min_size(Pos2::ZERO, Vec2::new(PANE.width(), CAPTION_H + 2.0 * ROW_H));
    art.begin_frame();
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let _ = show(ui, pane, &queue, &mut art, now, &tokens);
    });
    output.textures_delta.clear();

    assert_eq!(painted_rows(&output), vec![1, 2]);
    let requested: HashSet<usize> = art
        .requests(now)
        .into_iter()
        .filter_map(|command| match command {
            Command::Fetch { url } => Some(cover_row(&url)),
            _ => None,
        })
        .collect();
    assert_eq!(requested, HashSet::from([1, 2]));
}

#[test]
fn a_click_below_the_pane_picks_no_row() {
    // A culled row is by construction outside the pane's clip rect, so this
    // click is stopped by egui's hit testing and never reaches a row to
    // cull. That is the property pinned here; what pins the cull itself is
    // `a_row_that_only_touches_the_fold_is_not_laid_out`.
    let ctx = context();
    let queue = queue_of(40, 0);
    let at = Pos2::new(
        PANE.left() + space::S3 + 100.0,
        PANE.top() + CAPTION_H + 10.0 * ROW_H + ROW_H / 2.0,
    );
    assert!(
        at.y > PANE.bottom(),
        "the row under the click must be below the pane: {at:?}"
    );

    let reply = click(&ctx, PANE, &queue, at);
    assert_eq!(reply.press, None);
    assert_eq!(reply.play, None);
}

#[test]
fn a_pane_too_short_for_a_row_or_with_no_height_still_draws() {
    let ctx = context();
    let now = Instant::now();
    let queue = queue_of(4, 0);

    // The caption alone no longer fits, then not even the pane.
    for height in [CAPTION_H / 2.0, 0.0] {
        let pane = Rect::from_min_size(Pos2::ZERO, Vec2::new(PANE.width(), height));
        let mut art = Art::default();
        let (reply, output) = pass(&ctx, pane, &queue, &mut art, now, Vec::new());

        assert_eq!(reply.play, None, "{height}");
        assert_eq!(reply.press, None, "{height}");
        assert!(
            painted_rows(&output).is_empty(),
            "no row fits a {height}-tall pane"
        );
        for clipped in &output.shapes {
            if let Shape::Rect(rect) = &clipped.shape {
                assert!(
                    rect.rect.width() >= 0.0 && rect.rect.height() >= 0.0,
                    "an inverted rect in a {height}-tall pane: {:?}",
                    rect.rect
                );
            }
        }
    }
}
