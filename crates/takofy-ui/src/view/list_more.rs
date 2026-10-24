//! A block's reveal row: the one control that asks for the rest of a kind.
//!
//! The reference's `.list-more` is a full-width row at a block's foot, a step
//! shorter than a name: the words say how much is still hidden, a small chevron
//! trails them, and the row takes the pointer's wash and the keyboard's ring
//! like every other row. It is a request, not a switch — once taken the caller
//! shows the whole block and stops passing a label, so the row takes itself
//! away.
//!
//! The geometry is pure, so the row's rect and the place of its parts are
//! tested without a window.

use egui::{FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, WidgetInfo, vec2};

use crate::icon::{self, Icon};
use crate::theme::icon::Size;
use crate::theme::{Tokens, Weight, extent, fonts, radius, space, stroke, text};

use super::controls;
use super::text as recipe;

/// The row's rect: the block's own width, directly under it.
///
/// The caller hands the block's rect, not the block's grid: a card grid is
/// widened past the gutter so the art lines up with the headings, while the row
/// stays inside the page's column.
pub(crate) fn rect(block: Rect) -> Rect {
    Rect::from_min_size(
        Pos2::new(block.left(), block.bottom()),
        vec2(block.width(), extent::ROW_H),
    )
}

/// Where the row's two pieces go inside its rect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Parts {
    /// The label's left edge, at the row's vertical centre.
    pub label: Pos2,
    /// The glyph's box, after the label and the reference's `S2` gap.
    pub chevron: Rect,
}

/// The label at `inset` from the row's left, and the chevron after it.
pub(crate) fn parts(row: Rect, label_w: f32, inset: f32) -> Parts {
    let label = Pos2::new(row.left() + inset, row.center().y);
    let side = Size::SM.side;
    Parts {
        label,
        chevron: Rect::from_min_size(
            Pos2::new(label.x + label_w + space::S2, row.center().y - side / 2.0),
            vec2(side, side),
        ),
    }
}

/// Draws the row under `block` and reports whether it was taken.
///
/// `None` is a block with nothing hidden: no row is drawn and the cursor does
/// not move, which is what makes the row disappear once the rest is shown.
pub(crate) fn show(
    ui: &mut Ui,
    block: Rect,
    label: Option<&str>,
    inset: f32,
    tokens: &Tokens,
) -> bool {
    let Some(label) = label else {
        return false;
    };
    let row = rect(block);
    if row.width() <= 0.0 || row.height() <= 0.0 {
        return false;
    }
    let response = ui.allocate_rect(row, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, label));

    // The reference lights the row on hover and on a visible keyboard focus,
    // and the words step up from muted to the page's text with it.
    let lit = response.hovered() || response.has_focus();
    let color = if lit {
        tokens.palette.text
    } else {
        tokens.palette.text_muted
    };
    if lit {
        ui.painter()
            .rect_filled(row, radius::SM, tokens.palette.row_hover);
        ui.painter().rect_stroke(
            row,
            radius::SM,
            Stroke::new(stroke::HAIRLINE, tokens.palette.border),
            StrokeKind::Inside,
        );
    }

    let line = recipe::line(
        ui,
        label,
        FontId::new(text::SM, fonts::sans(Weight::Regular)),
        color,
        0.0,
        (row.width() - inset * 2.0).max(0.0),
    );
    let parts = parts(row, line.size().x, inset);
    ui.painter().galley(
        Pos2::new(parts.label.x, parts.label.y - line.size().y / 2.0),
        line,
        color,
    );
    icon::paint(
        ui.painter(),
        Icon::ChevronDown,
        parts.chevron,
        Size::SM,
        color,
    );

    controls::focus_ring(ui, &response, row, radius::SM, tokens);
    // The keyboard's Enter is answered beside the pointer's click, so a focused
    // row works with no mouse. The pressed row takes the keyboard with it, so
    // Tab continues from the row that was picked.
    let keyboard = response.has_focus() && ui.input(|input| input.key_pressed(Key::Enter));
    let taken = response.clicked() || keyboard;
    if taken {
        response.request_focus();
    }
    taken
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, FullOutput, Modifiers, RawInput, Shape, Vec2};

    use super::*;
    use crate::theme::style;

    /// The block the row hangs under: a card block's own allocated width, not
    /// the widened grid.
    fn block() -> Rect {
        Rect::from_min_size(Pos2::new(40.0, 20.0), Vec2::new(600.0, 200.0))
    }

    fn context() -> Context {
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&Tokens::DARK));
        ctx.enable_accesskit();
        ctx
    }

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 400.0))),
            events,
            ..Default::default()
        }
    }

    /// One frame of the row: what it reported, what it painted, and how far it
    /// moved the cursor.
    ///
    /// A quiet pass comes first, so the row is already laid out when the events
    /// arrive and egui has had a pass to turn accesskit on.
    fn frame(ctx: &Context, label: Option<&str>, events: Vec<Event>) -> (bool, FullOutput, f32) {
        let _ = pass(ctx, label, Vec::new());
        pass(ctx, label, events)
    }

    /// One pass of the row.
    fn pass(ctx: &Context, label: Option<&str>, events: Vec<Event>) -> (bool, FullOutput, f32) {
        let tokens = Tokens::DARK;
        let mut taken = false;
        let mut moved = 0.0;
        let mut output = ctx.run_ui(input(events), |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let top = ui.cursor().top();
            taken = show(ui, block(), label, space::S3, &tokens);
            moved = ui.cursor().top() - top;
        });
        output.textures_delta.clear();
        (taken, output, moved)
    }

    /// The fills painted on the row itself.
    fn fills(output: &FullOutput) -> Vec<egui::Color32> {
        let row = rect(block());
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                Shape::Rect(shape) if shape.rect == row => Some(shape.fill),
                _ => None,
            })
            .collect()
    }

    /// One press of `key`.
    fn press(key: Key) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn the_row_is_the_blocks_width_and_a_row_tall_directly_under_it() {
        let row = rect(block());
        assert_eq!(
            row.left(),
            block().left(),
            "the row starts where the block does"
        );
        assert_eq!(row.width(), block().width(), "and is as wide as it");
        assert_eq!(row.top(), block().bottom(), "the row hangs off its foot");
        assert_eq!(row.height(), extent::ROW_H);
    }

    #[test]
    fn the_label_is_inset_and_the_chevron_trails_it_by_one_gap() {
        let row = rect(block());
        let parts = parts(row, 100.0, space::S3);
        assert_eq!(parts.label.x, row.left() + space::S3);
        assert_eq!(
            parts.label.y,
            row.center().y,
            "the label sits on the centre"
        );
        assert_eq!(
            parts.chevron.left(),
            parts.label.x + 100.0 + space::S2,
            "the chevron follows the label after one gap"
        );
        assert_eq!(parts.chevron.size(), Vec2::splat(Size::SM.side));
        assert_eq!(parts.chevron.center().y, row.center().y);
    }

    #[test]
    fn a_row_with_nothing_hidden_draws_nothing_and_takes_no_room() {
        let ctx = context();
        let (taken, output, moved) = frame(&ctx, None, Vec::new());
        assert!(!taken);
        assert_eq!(moved, 0.0, "an absent row leaves the cursor where it was");
        assert!(output.shapes.is_empty(), "and paints nothing");
    }

    #[test]
    fn the_row_washes_only_while_it_is_lit() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let at = rect(block()).center();
        let (_, lit, _) = frame(
            &ctx,
            Some("Show 10 more tracks"),
            vec![Event::PointerMoved(at)],
        );
        assert!(
            fills(&lit).contains(&tokens.palette.row_hover),
            "a hovered row washes with the row hover: {:?}",
            fills(&lit)
        );

        let ctx = context();
        let (_, dark, _) = frame(&ctx, Some("Show 10 more tracks"), Vec::new());
        assert!(
            !fills(&dark).contains(&tokens.palette.row_hover),
            "a row with the pointer away and no focus washed"
        );
    }

    #[test]
    fn a_click_is_taken_once() {
        let ctx = context();
        let at = rect(block()).center();
        let mut taken = Vec::new();
        for event in crate::view::test_support::click_events(at) {
            taken.push(frame(&ctx, Some("Show 10 more tracks"), vec![event]).0);
        }
        assert_eq!(
            taken.iter().filter(|taken| **taken).count(),
            1,
            "one click is one request: {taken:?}"
        );
    }

    #[test]
    fn a_focused_row_answers_the_keyboards_enter() {
        let ctx = context();
        let at = rect(block()).center();
        for event in crate::view::test_support::click_events(at) {
            let _ = frame(&ctx, Some("Show 10 more tracks"), vec![event]);
        }
        let (taken, _, _) = frame(&ctx, Some("Show 10 more tracks"), vec![press(Key::Enter)]);
        assert!(taken, "Enter on the focused row asks for the rest");
    }

    #[test]
    fn the_rows_node_is_a_button_labelled_with_its_words() {
        let ctx = context();
        let (_, output, _) = frame(&ctx, Some("Show 10 more tracks"), Vec::new());
        let nodes = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("accesskit is on")
            .nodes
            .iter()
            .filter(|(_, node)| node.role() == egui::accesskit::Role::Button)
            .filter_map(|(_, node)| node.label().map(str::to_owned))
            .collect::<Vec<_>>();
        assert_eq!(nodes, ["Show 10 more tracks"]);
    }
}
