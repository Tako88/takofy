use egui::{Event, FontId, Pos2, Rect, Sense, Vec2};

use super::super::*;
use super::test_support::*;

#[test]
fn the_bar_alone_reports_its_own_reach() {
    let ctx = context();
    pin::clear(&ctx);
    let text = text_bar(kind(VIEWPORT));

    let _ = frame_of(&ctx, &text, VIEWPORT, 0.0, Vec::new(), None);
    assert_eq!(
        pin::inset(&ctx),
        0.0,
        "a row in its place reaches over nothing"
    );

    let _ = frame_of(&ctx, &text, VIEWPORT, FILLER + 50.0, Vec::new(), None);
    assert_eq!(
        pin::inset(&ctx),
        actionbar::STUCK_H,
        "a pinned row reports its own reach"
    );

    let _ = frame_of(&ctx, &text, VIEWPORT, FILLER + 50.0, Vec::new(), None);
    assert_eq!(
        pin::inset(&ctx),
        actionbar::STUCK_H,
        "the second frame reports the same reach, not twice it"
    );
}

#[test]
fn rows_never_paint_over_the_pinned_bar() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let bar = bar(kind(VIEWPORT));
    let mut output = ctx.run_ui(input(VIEWPORT, Vec::new()), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("bar")
            .vertical_scroll_offset(FILLER + 50.0)
            .show(ui, |ui| {
                ui.add_space(FILLER);
                let _ = show(ui, &bar, None, &tokens);
                // A row where the list would start, which the pinned row now
                // covers.
                ui.painter().text(
                    Pos2::new(0.0, 4.0),
                    egui::Align2::LEFT_TOP,
                    "a row",
                    FontId::default(),
                    tokens.palette.text,
                );
            });
    });
    output.textures_delta.clear();
    let (_, painted) = text_at(&output, "a row")
        .first()
        .copied()
        .expect("the row is painted");
    let clipped = output
        .shapes
        .iter()
        .find(|clipped| {
            matches!(&clipped.shape, egui::epaint::Shape::Text(text) if text.galley.text() == "a row")
        })
        .map(|clipped| clipped.clip_rect)
        .expect("the row is painted");
    assert!(
        clipped.top() >= actionbar::STUCK_H - 0.01,
        "a row under the pinned row is clipped to its bottom: {clipped:?} for {painted:?}"
    );
}

#[test]
fn a_press_in_the_pinned_strip_reaches_no_row() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let bar = bar(kind(VIEWPORT));
    let id = egui::Id::unique("row-under-the-bar");
    let at = Pos2::new(400.0, 20.0);
    let mut clicked = false;
    for events in [
        vec![Event::PointerMoved(at)],
        vec![crate::view::test_support::button(at, true)],
        vec![crate::view::test_support::button(at, false)],
    ] {
        let mut output = ctx.run_ui(input(VIEWPORT, events), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("bar")
                .vertical_scroll_offset(FILLER + 50.0)
                .show(ui, |ui| {
                    ui.add_space(FILLER);
                    let _ = show(ui, &bar, None, &tokens);
                    let body = ui.interact(
                        Rect::from_min_size(Pos2::ZERO, Vec2::new(VIEWPORT.x, actionbar::STUCK_H)),
                        id,
                        Sense::click(),
                    );
                    clicked |= body.clicked();
                });
        });
        output.textures_delta.clear();
    }
    assert!(!clicked, "a press inside the pinned strip reached a row");
}
