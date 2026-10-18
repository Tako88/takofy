//! The shared control slot: interaction, accessibility and the focus ring.
//!
//! A painted control draws itself, but every control passes through this one
//! seam first. The slot is where a waiting control stops being interactive: it
//! senses hover only, never joins the tab order, and announces itself disabled
//! with its label through `WidgetInfo`.
//!
//! The focus ring is drawn here too, because every control wants the same one
//! and only the slot knows the control has focus.

use egui::{Rect, Response, Role, Sense, Stroke, StrokeKind, Ui, WidgetInfo};

use crate::availability::{Availability, Control, availability};
use crate::theme::ease;
use crate::theme::{Tokens, extent, motion, radius, stroke};

/// What a slot hands back.
pub struct Slot {
    pub response: Response,
    /// 0 at rest, 1 fully hovered, eased. Always 0 while waiting.
    pub hover: f32,
}

/// Wires one control's rect to its interaction and accessibility.
///
/// `role` is a parameter because the search field is a `Role::SearchInput` and
/// not a button. `tokens` is needed for the focus ring's accent, which is not a
/// field of egui's `Visuals`.
pub fn slot(
    ui: &mut Ui,
    control: Control,
    role: Role,
    rect: Rect,
    label: &str,
    tokens: &Tokens,
) -> Slot {
    let ready = availability(control) == Availability::Ready;
    let id = ui.scope_id().with(control);
    let sense = if ready {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(rect, id, sense);

    response.widget_info(|| WidgetInfo::labeled(role, ready, label));

    let hover = if ready {
        ui.ctx().animate_bool_with_time_and_easing(
            id.with("hover"),
            response.hovered(),
            motion::FAST,
            ease::out,
        )
    } else {
        0.0
    };

    if ready && response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(extent::FOCUS_OFFSET),
            radius::SM,
            Stroke::new(stroke::HAIRLINE, tokens.palette.accent),
            StrokeKind::Outside,
        );
    }

    if ready && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    Slot { response, hover }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Tokens;
    use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, Vec2};

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 400.0))),
            events,
            ..Default::default()
        }
    }

    /// Runs one pass and consumes the font-atlas delta it may have produced.
    fn run(ctx: &Context, input: RawInput, draw: impl FnMut(&mut Ui)) {
        let mut output = ctx.run_ui(input, draw);
        output.textures_delta.clear();
    }

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

    fn cell(index: f32) -> Rect {
        Rect::from_min_size(Pos2::new(index * 40.0, 0.0), Vec2::new(28.0, 28.0))
    }

    #[test]
    fn a_waiting_control_is_not_interactive_and_not_focusable() {
        let ctx = Context::default();
        let mut sense = None;
        run(&ctx, input(Vec::new()), |ui| {
            let slot = slot(
                ui,
                Control::NavHome,
                Role::Button,
                cell(0.0),
                "Home",
                &Tokens::DARK,
            );
            sense = Some(slot.response.sense);
        });
        let sense = sense.expect("the slot drew");
        assert!(!sense.senses_click());
        assert!(!sense.is_focusable());
    }

    #[test]
    fn a_ready_control_is_focusable() {
        let ctx = Context::default();
        let mut sense = None;
        run(&ctx, input(Vec::new()), |ui| {
            let slot = slot(
                ui,
                Control::DrawerToggle,
                Role::Button,
                cell(0.0),
                "Toggle",
                &Tokens::DARK,
            );
            sense = Some(slot.response.sense);
        });
        let sense = sense.expect("the slot drew");
        assert!(sense.senses_click());
        assert!(sense.is_focusable());
    }

    #[test]
    fn a_waiting_control_is_announced_disabled_with_its_label() {
        let ctx = Context::default();
        // AccessKit state exists only while the pass that built it is live.
        ctx.enable_accesskit();
        let mut node = None;
        run(&ctx, input(Vec::new()), |ui| {
            let slot = slot(
                ui,
                Control::NavHome,
                Role::Button,
                cell(0.0),
                "Home",
                &Tokens::DARK,
            );
            assert!(ui.ctx().has_accesskit_node(slot.response.id));
            node = ui.ctx().accesskit_node_builder(slot.response.id, |node| {
                (node.is_disabled(), node.label().map(str::to_owned))
            });
        });
        let (disabled, label) = node.expect("the slot has an AccessKit node");
        assert!(disabled, "a waiting control must be announced disabled");
        assert_eq!(label.as_deref(), Some("Home"));
    }

    #[test]
    fn tab_never_lands_on_a_waiting_control() {
        let ctx = Context::default();
        let controls = [
            (Control::DrawerToggle, "Toggle"),
            (Control::NavHome, "Home"),
            (Control::TrailPlaylists, "Playlists"),
        ];
        let tab = Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };

        // Focus traversal reads the previous pass, and one Tab pass per widget.
        run(&ctx, input(Vec::new()), |_ui| {});
        let mut focused = Vec::new();
        for _ in 0..6 {
            let focused = &mut focused;
            run(&ctx, input(vec![tab.clone()]), |ui| {
                for (index, (control, label)) in controls.iter().enumerate() {
                    let slot = slot(
                        ui,
                        *control,
                        Role::Button,
                        cell(index as f32),
                        label,
                        &Tokens::DARK,
                    );
                    if slot.response.has_focus() {
                        focused.push(*control);
                    }
                }
            });
        }

        assert!(
            !focused.contains(&Control::NavHome),
            "a waiting control took focus: {focused:?}"
        );
        assert!(focused.contains(&Control::DrawerToggle));
        assert!(focused.contains(&Control::TrailPlaylists));
    }

    #[test]
    fn a_click_on_a_waiting_control_does_nothing() {
        let ctx = Context::default();
        let waiting = Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::new(28.0, 28.0));
        let ready = Rect::from_min_size(Pos2::new(200.0, 100.0), Vec2::new(28.0, 28.0));
        let mut clicked = (false, false);

        // A press and release need several passes to register.
        let tap = |ctx: &Context, at: Pos2, clicked: &mut (bool, bool)| {
            for event in [Event::PointerMoved(at), button(at, true), button(at, false)] {
                run(ctx, input(vec![event]), |ui| {
                    let waiting_slot = slot(
                        ui,
                        Control::NavHome,
                        Role::Button,
                        waiting,
                        "Home",
                        &Tokens::DARK,
                    );
                    let ready_slot = slot(
                        ui,
                        Control::DrawerToggle,
                        Role::Button,
                        ready,
                        "Toggle",
                        &Tokens::DARK,
                    );
                    clicked.0 = waiting_slot.response.clicked();
                    clicked.1 = ready_slot.response.clicked();
                });
            }
        };

        // Warm up so hit-testing has a previous pass to read.
        run(&ctx, input(Vec::new()), |_ui| {});
        tap(&ctx, waiting.center(), &mut clicked);
        assert!(!clicked.0, "a waiting control received a click");
        assert!(!clicked.1, "the waiting rect is not the ready one");

        tap(&ctx, ready.center(), &mut clicked);
        assert!(clicked.1, "the click harness never produced a click");
    }
}
