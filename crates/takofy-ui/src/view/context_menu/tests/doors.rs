use super::super::*;
use super::test_support::*;
use crate::menu::{Anchor, Device, Menu};
use egui::{Event, Id, Rect, pos2, vec2};

/// One frame that reads `is_open` before it draws the menu, the order the
/// shell runs in.
fn seen_before_the_draw(ctx: &egui::Context, menu: &mut Menu) -> bool {
    let tokens = Tokens::DARK;
    let mut seen = false;
    let mut drawn = ctx.run_ui(input(Vec::new()), |ui| {
        seen = is_open(ui.ctx());
        let _ = show(
            ui,
            Frame {
                menu,
                free: true,
                tokens: &tokens,
            },
        );
    });
    drawn.textures_delta.clear();
    seen
}

#[test]
fn an_open_menu_says_so_before_it_draws() {
    let ctx = context();
    let mut menu = opened(Device::Pointer);
    warm(&ctx, &mut menu);
    assert!(
        seen_before_the_draw(&ctx, &mut menu),
        "a page drew behind an open menu it could not see"
    );

    let _ = menu.close(Reason::Escape);
    let _ = seen_before_the_draw(&ctx, &mut menu);
    assert!(
        !seen_before_the_draw(&ctx, &mut menu),
        "the frame after the close still reported an open menu"
    );
}

#[test]
fn a_pointer_door_is_the_rect_the_layer_and_the_pointer() {
    let ctx = context();
    let rect = Rect::from_min_size(pos2(100.0, 100.0), vec2(200.0, 40.0));
    let opener = Id::unique("row");
    let anchor = Rect::from_min_size(pos2(260.0, 100.0), vec2(28.0, 28.0));
    let inside = pos2(160.0, 120.0);
    let outside = pos2(400.0, 300.0);

    for (point, expected) in [(inside, true), (outside, false)] {
        let mut found = None;
        for event in crate::view::test_support::secondary_click_events(point) {
            let mut output = ctx.run_ui(input(vec![event]), |ui| {
                let _ = ui.interact(rect, opener, Sense::click());
                found = pointer_door(ui, rect, opener, anchor);
            });
            output.textures_delta.clear();
        }

        match found {
            Some(door) => {
                assert!(expected, "a right-click at {point:?} opened a door");
                assert_eq!(door.opener, opener);
                assert_eq!(door.device, Device::Pointer);
                let Anchor::Pointer(at) = door.anchor else {
                    panic!("a pointer door anchored to the button");
                };
                assert!(
                    (at - point).length() < 0.01,
                    "a right-click at {point:?} opened at {at:?}"
                );
            }
            None => assert!(!expected, "a right-click at {point:?} opened nothing"),
        }
    }
}

#[test]
fn a_pointer_door_under_another_layer_is_none() {
    let ctx = context();
    let rect = Rect::from_min_size(pos2(100.0, 100.0), vec2(200.0, 40.0));
    let opener = Id::unique("row");
    let anchor = Rect::from_min_size(pos2(260.0, 100.0), vec2(28.0, 28.0));
    let point = pos2(160.0, 120.0);
    let cover = Rect::from_min_size(point - vec2(40.0, 20.0), vec2(200.0, 40.0));

    let answer = |events: Vec<Event>| {
        let mut found = None;
        let mut output = ctx.run_ui(input(events), |ui| {
            egui::Area::new(Id::unique("cover"))
                .order(Order::Middle)
                .fixed_pos(cover.min)
                .show(ui.ctx(), |ui| {
                    ui.expand_to_include_rect(cover);
                    ui.painter()
                        .rect_filled(cover, 0.0, Tokens::DARK.palette.surface);
                });
            let _ = ui.interact(rect, opener, Sense::click());
            found = pointer_door(ui, rect, opener, anchor);
        });
        output.textures_delta.clear();
        found
    };

    // Two frames, so the cover's own rect is known to the layer test.
    let _ = answer(Vec::new());
    let _ = answer(Vec::new());
    for event in crate::view::test_support::secondary_click_events(point) {
        assert_eq!(
            answer(vec![event]),
            None,
            "a right-click under another layer opened a door"
        );
    }
}
