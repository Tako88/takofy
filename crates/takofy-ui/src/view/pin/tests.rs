use egui::{Context, Pos2, RawInput, Rect, Vec2};

use super::*;
use crate::theme::{fonts, style};

const VIEWPORT: Vec2 = Vec2::new(400.0, 300.0);
const ROW_H: f32 = 40.0;
/// Enough content that a scroll is never clamped at either end.
const ROWS: usize = 50;

fn context() -> Context {
    let tokens = crate::theme::Tokens::DARK;
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&tokens));
    ctx
}

/// Where the row sits on screen once the frame's request has been applied.
struct Seen {
    row_top: f32,
    offset: f32,
}

/// One frame of a long list, scrolled to `start` and asked to reveal `row`.
///
/// The scroll is not animated, so the offset the frame ends on is the offset
/// the request asked for. The rows are allocated in the list's own flow, so
/// the rect the request is given is the rect a row is drawn into.
fn frame(ctx: &Context, start: f32, row: usize) -> Seen {
    let mut asked = None;
    let mut settled = 0.0;
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, VIEWPORT)),
            ..Default::default()
        },
        |ui| {
            let out = egui::ScrollArea::vertical()
                .id_salt("list")
                .animated(false)
                .vertical_scroll_offset(start)
                .show(ui, |ui| {
                    for index in 0..ROWS {
                        let (rect, _) = ui.allocate_exact_size(
                            Vec2::new(VIEWPORT.x, ROW_H),
                            egui::Sense::hover(),
                        );
                        if index == row {
                            reveal(ui, rect);
                            asked = Some(rect);
                        }
                    }
                });
            settled = out.state.clamped_offset().y;
        },
    );
    output.textures_delta.clear();
    let rect = asked.expect("the asked-for row is drawn");
    Seen {
        row_top: rect.top() + start - settled,
        offset: settled,
    }
}

#[test]
fn no_inset_asks_for_the_row_itself() {
    let ctx = context();
    clear(&ctx);
    // The list starts scrolled past the row, so the row is above the
    // viewport and the reveal has to bring it back down.
    let seen = frame(&ctx, 300.0, 5);
    assert!(
        seen.row_top >= 0.0 && seen.row_top < ROW_H,
        "the row lands at the viewport's top edge: {}",
        seen.row_top
    );
}

#[test]
fn an_inset_shifts_the_request_up_by_its_height() {
    let ctx = context();
    let height = 60.0;
    clear(&ctx);
    let plain = frame(&ctx, 300.0, 5);
    set(&ctx, height);
    let shifted = frame(&ctx, 300.0, 5);
    assert!(
        (shifted.row_top - plain.row_top - height).abs() < 0.01,
        "the row lands {} lower, not {height}: {} against {}",
        shifted.row_top - plain.row_top,
        shifted.row_top,
        plain.row_top
    );
}

#[test]
fn a_row_below_the_viewport_still_scrolls_in_with_an_inset() {
    let ctx = context();
    set(&ctx, 60.0);
    // Row twenty is at 800 in the content and the list is at its top, so the
    // row starts well below the viewport. Only its true bottom says how far
    // the list has to move.
    let seen = frame(&ctx, 0.0, 20);
    assert!(
        seen.row_top >= 0.0 && seen.row_top + ROW_H <= VIEWPORT.y + 0.01,
        "the whole row comes into view, not only the part above its shifted bottom: {}..{}",
        seen.row_top,
        seen.row_top + ROW_H
    );
}

#[test]
fn a_row_already_clear_of_the_inset_requests_nothing() {
    let ctx = context();
    set(&ctx, 60.0);
    // Row ten is at 400 in the content and the list is scrolled to 300, so it
    // is a hundred down the viewport: well clear of a sixty-point inset.
    let seen = frame(&ctx, 300.0, 10);
    assert!(
        (seen.offset - 300.0).abs() < 0.01,
        "a row already in view asks for no scroll: {}",
        seen.offset
    );
}

#[test]
fn clear_forgets_a_previous_frames_inset() {
    let ctx = context();
    set(&ctx, 60.0);
    assert_eq!(inset(&ctx), 60.0);
    clear(&ctx);
    assert_eq!(inset(&ctx), 0.0, "the next frame starts with no inset");
}

#[test]
fn a_page_without_a_bar_reveals_at_the_viewport_top() {
    let ctx = context();
    // A frame that pinned a row leaves its height behind; the next frame's
    // `clear` is what a page with no pinned row draws is handed.
    set(&ctx, 60.0);
    clear(&ctx);
    let seen = frame(&ctx, 300.0, 5);
    assert!(
        seen.row_top < ROW_H,
        "with nothing pinned the row lands at the top: {}",
        seen.row_top
    );
}

#[test]
fn raise_keeps_the_lowest_bottom() {
    let ctx = context();
    clear(&ctx);
    set(&ctx, 60.0);
    raise(&ctx, 120.0);
    assert_eq!(inset(&ctx), 120.0, "the lower band is the one to clear");
    raise(&ctx, 80.0);
    assert_eq!(
        inset(&ctx),
        120.0,
        "a band that ends above the lowest does not lift the inset"
    );
}

#[test]
fn raise_on_a_cleared_seam_is_the_band_alone() {
    let ctx = context();
    clear(&ctx);
    raise(&ctx, 100.0);
    assert_eq!(inset(&ctx), 100.0);
}

#[test]
fn set_after_a_raise_starts_the_frame_over() {
    let ctx = context();
    set(&ctx, 60.0);
    raise(&ctx, 120.0);
    set(&ctx, 60.0);
    assert_eq!(
        inset(&ctx),
        60.0,
        "the bar's own reach resets the seam for the frame"
    );
}

#[test]
fn a_reveal_clears_the_lowest_band() {
    let ctx = context();
    clear(&ctx);
    let plain = frame(&ctx, 300.0, 5);
    set(&ctx, 60.0);
    raise(&ctx, 120.0);
    let shifted = frame(&ctx, 300.0, 5);
    assert!(
        (shifted.row_top - plain.row_top - 120.0).abs() < 0.01,
        "the row lands clear of both bands: {} against {}",
        shifted.row_top,
        plain.row_top
    );
}
