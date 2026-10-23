//! The seam between a row that pins and a row that has to be seen.
//!
//! egui's `scroll_to_rect` measures a rect against the scroll area's own clip,
//! which is the parent of the `Ui` a page draws in. A page that shrinks its clip
//! to keep rows from painting over the pinned row therefore hides the rows from
//! the pointer and from the culling test, but not from the scroll target: a row
//! whose top sits inside the pinned row's strip is "already in view" to egui and
//! is never brought out from under it.
//!
//! So the pinned row records how far below the viewport's top it reaches, and
//! every reveal asks for its rect's top edge shifted up by that much. The shift
//! is safe on a row that is already clear: egui measures the shifted rect
//! against the same clip and asks for no movement at all.

use egui::{Id, Rect, Ui, vec2};

fn id() -> Id {
    Id::unique("pinned-row-inset")
}

/// How far below the viewport's top this frame's pinned row reaches.
///
/// Zero when nothing pinned, which is every page without a pinned row and every
/// page that has not scrolled past it yet.
pub(crate) fn inset(ctx: &egui::Context) -> f32 {
    ctx.data(|data| data.get_temp(id()).unwrap_or(0.0))
}

/// Records how far the pinned rows reach below the viewport's top.
///
/// This overwrites what the frame had recorded: a row that pins under another
/// reads `inset` and adds whatever it covers, so the seam always says the whole
/// of what a reveal has to clear.
pub(crate) fn set(ctx: &egui::Context, inset: f32) {
    ctx.data_mut(|data| data.insert_temp(id(), inset.max(0.0)));
}

/// Forgets the inset, so a page without a pinned row never inherits one.
///
/// Called before any region draws, so a page that pins nothing leaves the seam at
/// zero and a row revealed on it lands where egui puts it; and again before a
/// region whose rows are not under the page's bar, so a stuck page cannot shift
/// them.
pub(crate) fn clear(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove_temp::<f32>(id()));
}

/// Brings `rect` into view, clear of whatever pins above it.
///
/// This is the only way a row asks to be seen: a reveal that does not know about
/// the inset leaves the row's top under the pinned row. Only the top edge is
/// shifted, though: the inset is what covers the viewport's top, so it must not
/// make a row below the viewport look shorter than it is.
pub(crate) fn reveal(ui: &mut Ui, rect: Rect) {
    let inset = inset(ui.ctx());
    let asked = Rect::from_min_max(rect.min - vec2(0.0, inset), rect.max);
    ui.scroll_to_rect(asked, None);
}

#[cfg(test)]
mod tests {
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
}
