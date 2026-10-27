use super::*;
use egui::{
    Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Sense, Ui, Vec2, accesskit, pos2, vec2,
};

fn a() -> Id {
    Id::unique("a")
}

fn b() -> Id {
    Id::unique("b")
}

fn id(index: usize) -> Id {
    Id::unique(("focus-test", index))
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), Vec2::new(400.0, 200.0))),
        events,
        ..Default::default()
    }
}

fn cell(index: f32) -> Rect {
    Rect::from_min_size(pos2(10.0 + index * 60.0, 10.0), Vec2::new(50.0, 30.0))
}

fn key(key: Key, pressed: bool) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

fn button(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn anchor(node: u64) -> Event {
    Event::AccessKitActionRequest(accesskit::ActionRequest {
        action: accesskit::Action::Focus,
        target_node: accesskit::NodeId(node),
        target_tree: accesskit::TreeId::ROOT,
        data: None,
    })
}

/// Draws both widgets, answering each ring, the way a view does.
fn pass(ctx: &Context, input: RawInput) -> [bool; 2] {
    let mut rings = [false; 2];
    let mut output = ctx.run_ui(input, |ui: &mut Ui| {
        begin_frame(ui.ctx());
        for (index, ring) in rings.iter_mut().enumerate() {
            let response = ui.interact(cell(index as f32), id(index), Sense::click());
            if response.clicked() {
                response.request_focus();
            }
            *ring = visible(ui.ctx(), id(index));
        }
    });
    output.textures_delta.clear();
    rings
}

/// Three pointer moves and a release: one click on the first widget.
fn click_first(ctx: &Context) {
    let at = cell(0.0).center();
    let _ = pass(ctx, input(Vec::new()));
    let _ = pass(ctx, input(vec![Event::PointerMoved(at)]));
    let _ = pass(ctx, input(vec![button(at, PointerButton::Primary, true)]));
    let _ = pass(ctx, input(vec![button(at, PointerButton::Primary, false)]));
}

#[test]
fn a_press_hides_the_ring_even_on_the_widget_already_focused() {
    let mut seen = Seen::default();
    seen.advance(None, Some(a()));
    assert!(seen.visible, "the fresh window shows programmatic focus");

    seen.advance(Some(Input::Pointer), Some(a()));
    assert!(
        !seen.visible,
        "a press hides the ring on the widget it hits"
    );
}

#[test]
fn focus_that_arrives_after_a_key_is_visible() {
    let mut seen = Seen::default();
    seen.advance(Some(Input::Key), None);
    seen.advance(None, Some(a()));
    assert!(seen.visible);
}

#[test]
fn focus_that_arrives_after_a_press_is_hidden() {
    let mut seen = Seen::default();
    seen.advance(Some(Input::Pointer), None);
    seen.advance(None, Some(a()));
    assert!(!seen.visible);
}

#[test]
fn a_key_after_a_click_does_not_show_the_ring_without_a_focus_move() {
    let mut seen = Seen::default();
    seen.advance(Some(Input::Pointer), None);
    seen.advance(None, Some(a()));

    seen.advance(Some(Input::Key), Some(a()));
    assert!(!seen.visible, "a key alone reveals nothing");
}

#[test]
fn tab_after_a_click_shows_the_ring_on_the_next_widget() {
    let mut seen = Seen::default();
    seen.advance(Some(Input::Pointer), None);
    seen.advance(None, Some(a()));

    seen.advance(Some(Input::Key), Some(a()));
    assert!(!seen.visible, "the ring waits for the move");
    seen.advance(None, Some(b()));
    assert!(seen.visible, "Tab moved focus, so the ring follows");
}

#[test]
fn a_key_while_the_ring_is_visible_keeps_it() {
    let mut seen = Seen::default();
    seen.advance(None, Some(a()));
    seen.advance(Some(Input::Key), Some(a()));
    assert!(seen.visible);
}

#[test]
fn programmatic_focus_in_a_fresh_window_is_visible() {
    let mut seen = Seen::default();
    seen.advance(None, Some(a()));
    assert!(seen.visible);
}

#[test]
fn programmatic_focus_after_a_press_is_hidden() {
    let mut seen = Seen::default();
    seen.advance(Some(Input::Pointer), None);
    seen.advance(None, Some(a()));
    assert!(!seen.visible);
}

#[test]
fn no_focus_is_never_visible() {
    let mut seen = Seen::default();
    seen.advance(None, Some(a()));
    seen.advance(None, None);
    assert!(!seen.visible, "losing focus hides the ring");
    seen.advance(Some(Input::Key), None);
    assert!(!seen.visible, "a key with nothing focused reveals nothing");
}

#[test]
fn the_last_input_in_a_frame_wins() {
    let at = pos2(1.0, 1.0);

    let mut pressed = Seen::default();
    pressed.advance(None, Some(a()));
    pressed.advance(
        last_input(&[
            key(Key::Space, true),
            button(at, PointerButton::Primary, true),
        ]),
        Some(a()),
    );
    assert!(!pressed.visible, "a press after a key is a press");

    let mut typed = Seen::default();
    typed.advance(Some(Input::Pointer), Some(a()));
    typed.advance(
        last_input(&[
            button(at, PointerButton::Primary, true),
            key(Key::Tab, true),
        ]),
        Some(a()),
    );
    typed.advance(None, Some(b()));
    assert!(
        typed.visible,
        "a key after a press leaves the keyboard in use"
    );
}

#[test]
fn advancing_twice_with_the_same_input_changes_nothing() {
    let mut seen = Seen::default();
    seen.advance(Some(Input::Pointer), None);
    seen.advance(None, Some(a()));

    let once = seen;
    seen.advance(None, Some(a()));
    assert_eq!(seen, once);

    seen.advance(Some(Input::Key), Some(a()));
    let once = seen;
    seen.advance(Some(Input::Key), Some(a()));
    assert_eq!(seen, once);
}

#[test]
fn releases_moves_and_scrolls_are_not_input() {
    let at = pos2(1.0, 1.0);
    assert_eq!(
        last_input(&[
            Event::PointerMoved(at),
            button(at, PointerButton::Primary, false),
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, 1.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: Modifiers::NONE,
            },
            Event::ModifiersChanged(Modifiers::SHIFT),
            key(Key::Space, false),
        ]),
        None
    );
}

#[test]
fn a_right_click_counts_as_a_press() {
    let at = pos2(1.0, 1.0);
    assert_eq!(
        last_input(&[button(at, PointerButton::Secondary, true)]),
        Some(Input::Pointer)
    );
}

#[test]
fn an_accesskit_action_counts_as_a_key() {
    assert_eq!(last_input(&[anchor(1)]), Some(Input::Key));
}

#[test]
fn a_modifier_key_then_a_press_in_one_frame_is_a_press() {
    let at = pos2(1.0, 1.0);
    assert_eq!(
        last_input(&[
            key(Key::Space, true),
            button(at, PointerButton::Primary, true)
        ]),
        Some(Input::Pointer)
    );
}

#[test]
fn a_click_leaves_no_visible_focus_and_tab_restores_it() {
    let ctx = Context::default();
    click_first(&ctx);
    assert_eq!(
        ctx.memory(|memory| memory.focused()),
        Some(id(0)),
        "the click took the keyboard"
    );

    let after_click = pass(&ctx, input(Vec::new()));
    assert_eq!(after_click, [false, false], "a click shows no ring");

    let _ = pass(&ctx, input(vec![key(Key::Tab, true)]));
    let after_tab = pass(&ctx, input(vec![key(Key::Tab, false)]));
    assert_eq!(
        ctx.memory(|memory| memory.focused()),
        Some(id(1)),
        "Tab moved on"
    );
    assert_eq!(after_tab, [false, true], "the ring follows the keyboard");
}

#[test]
fn an_unfocused_id_is_never_visible_and_takes_no_lock() {
    let ctx = Context::default();
    let mut hidden = true;
    let mut unchanged = false;
    let mut output = ctx.run_ui(input(Vec::new()), |ui: &mut Ui| {
        begin_frame(ui.ctx());
        let before = peek(ui.ctx()).expect("the frame left its state");
        hidden = !visible(ui.ctx(), id(1));
        unchanged = peek(ui.ctx()) == Some(before);
    });
    output.textures_delta.clear();

    assert!(hidden, "an unfocused widget never wears a ring");
    assert!(unchanged, "the unfocused path wrote no state");
}

#[test]
fn a_consumed_key_is_still_seen_when_begin_frame_ran_first() {
    let ctx = Context::default();
    click_first(&ctx);

    // A consumer takes the key out before any view reads the events.
    let mut output = ctx.run_ui(input(vec![key(Key::Tab, true)]), |ui: &mut Ui| {
        begin_frame(ui.ctx());
        ui.ctx().input_mut(|input| {
            input
                .events
                .retain(|event| !matches!(event, Event::Key { .. }));
        });
        for index in 0..2 {
            ui.interact(cell(index as f32), id(index), Sense::click());
        }
    });
    output.textures_delta.clear();

    let after = pass(&ctx, input(Vec::new()));
    assert_eq!(
        ctx.memory(|memory| memory.focused()),
        Some(id(1)),
        "Tab moved on"
    );
    assert_eq!(
        after,
        [false, true],
        "begin_frame read the key before it was consumed"
    );
}
