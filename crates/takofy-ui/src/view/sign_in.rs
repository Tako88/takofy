//! The card the window shows before there is a library.
//!
//! The reference draws no sign-in screen, so this is composed out of its parts
//! rather than transcribed: the words are its empty-state, the control is its
//! `.button`, and the heading is the hero's own recipe, one size down.
//!
//! It stays off the accent colour. In this design sound is the only loud thing,
//! and a green "Authorize" would compete with the play button that has to be the
//! one call to action in the window.

use egui::{Align, Frame, Layout, Margin, Rect, Ui, Vec2, vec2};

use crate::availability::Control;
use crate::icon::{self, Icon};
use crate::message::Command;
use crate::signin::{SignIn, copy};
use crate::theme::icon::Size;
use crate::theme::{Tokens, extent, radius, space, stroke, text};

use super::{controls, text as recipe};

/// Draws the sign-in card, returning a command when the user authorizes.
///
/// Every state draws inside the same frame, so the window does not change shape as
/// the flow moves between them; what changes is whether the card has words to show
/// or only a spinner.
pub fn show(ui: &mut Ui, state: &mut SignIn, tokens: &Tokens) -> Option<Command> {
    card(ui, state, tokens).0
}

/// Draws the card and hands back what one pass drew: the command, the frame's
/// painted rect and, when the state offers one, the control's own rect.
///
/// `card_width` is the content measure; only the frame's rect shows the box the
/// window actually paints, which is what the fit test has to check.
fn card(ui: &mut Ui, state: &mut SignIn, tokens: &Tokens) -> (Option<Command>, Rect, Option<Rect>) {
    let words = copy(*state);
    let card = card_width(ui);

    let mut command = None;
    let mut button_rect = None;
    let mut frame_rect = Rect::NOTHING;
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_space(space::S8);
        ui.allocate_ui_with_layout(vec2(card, 0.0), Layout::top_down(Align::Center), |ui| {
            let frame = Frame::new()
                .fill(tokens.palette.surface)
                .stroke(egui::Stroke::new(stroke::HAIRLINE, tokens.palette.border))
                .corner_radius(radius::LG)
                .shadow(tokens.palette.shadow_md)
                .inner_margin(Margin::symmetric(space::S6 as i8, space::S6 as i8))
                .show(ui, |ui| {
                    // The card centres itself in the window; its words are a
                    // left-aligned block, which is what the reference's own
                    // empty-state does and what a paragraph needs to be read.
                    ui.set_width(card - 2.0 * space::S6);
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        logo(ui, tokens);

                        match words {
                            Some(words) => {
                                ui.add_space(space::S5);
                                ui.label(heading(words.heading, tokens));
                                ui.add_space(space::S2);
                                // The reference sets its empty-state body at
                                // `--text-sm`, so the body takes `card_body`
                                // rather than egui's default face.
                                ui.label(
                                    recipe::muted(words.body, tokens).font(recipe::card_body()),
                                );
                                ui.add_space(space::S5);
                                let (pressed, rect) = button(ui, state, words.label, tokens);
                                command = pressed;
                                button_rect = Some(rect);
                            }
                            // Working, with nothing to read: the spinner is
                            // the whole message.
                            None => {
                                ui.add_space(space::S5);
                                ui.spinner();
                                ui.add_space(space::S1);
                            }
                        }
                    });
                });
            frame_rect = frame.response.rect;
        });
    });
    (command, frame_rect, button_rect)
}

/// The card's width: the reference's narrow measure, or what the window has. A
/// minimum window still leaves it a gutter on each side.
fn card_width(ui: &Ui) -> f32 {
    let room = ui.available_width() - 2.0 * space::S5;
    extent::SIGNIN_W.min(room).max(0.0)
}

/// The mark above the words.
fn logo(ui: &mut Ui, tokens: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(Size::LG.side), egui::Sense::hover());
    icon::paint(
        ui.painter(),
        Icon::Logo,
        rect,
        Size::LG,
        tokens.palette.text,
    );
}

/// The card's heading, and the wordmark's face at a larger size.
fn heading(label: &str, tokens: &Tokens) -> egui::RichText {
    egui::RichText::new(label)
        .font(recipe::card_heading())
        .extra_letter_spacing(recipe::tracking_pts(
            text::XL,
            crate::theme::tracking::TIGHT,
        ))
        .color(tokens.palette.text)
}

/// The one control, which waits when the flow is already using it.
fn button(
    ui: &mut Ui,
    state: &mut SignIn,
    label: &str,
    tokens: &Tokens,
) -> (Option<Command>, Rect) {
    let size = controls::text_button_size(ui, tokens, label);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let live = state.can_authorize();
    let slot = controls::text_button(ui, tokens, Control::Authorize, rect, label, live);
    let command = if slot.response.clicked() {
        state.authorize()
    } else {
        None
    };
    (command, rect)
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

    use super::*;
    use crate::theme::{Tokens, fonts, style};
    use crate::view::test_support::{rings_in, tab};

    /// A context with the app's fonts and style installed.
    fn context() -> Context {
        let tokens = Tokens::DARK;
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        ctx
    }

    fn run(ctx: &Context, input: RawInput, mut draw: impl FnMut(&mut Ui)) {
        let mut output = ctx.run_ui(input, |ui| {
            crate::view::focus::begin_frame(ui.ctx());
            draw(ui);
        });
        output.textures_delta.clear();
    }

    /// Runs one pass and returns what it painted.
    fn painted(ctx: &Context, input: RawInput, mut draw: impl FnMut(&mut Ui)) -> egui::FullOutput {
        let mut output = ctx.run_ui(input, |ui| {
            crate::view::focus::begin_frame(ui.ctx());
            draw(ui);
        });
        output.textures_delta.clear();
        output
    }

    fn window(width: f32, height: f32) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height))),
            ..Default::default()
        }
    }

    fn pointer_button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn a_window_that_needs_authorizing_offers_the_control_and_does_not_press_it() {
        // Nothing is clicked here, so a command would mean the card pressed its own
        // button, or advanced the state on a frame the user did not act on.
        let ctx = context();
        let mut state = SignIn::NeedsAuthorization;
        let mut command = None;
        run(&ctx, window(900.0, 600.0), |ui| {
            command = show(ui, &mut state, &Tokens::DARK);
        });
        assert_eq!(command, None);
        assert_eq!(state, SignIn::NeedsAuthorization);
    }

    #[test]
    fn the_card_fits_the_minimum_window() {
        // The narrowest window the app allows must still show the whole painted
        // card, frame and hairline included, or its words would be clipped.
        let ctx = context();
        let mut state = SignIn::NeedsAuthorization;
        let mut frame = Rect::NOTHING;
        let (width, height) = (crate::layout::WINDOW_MIN[0], crate::layout::WINDOW_MIN[1]);
        run(&ctx, window(width, height), |ui| {
            frame = card(ui, &mut state, &Tokens::DARK).1;
        });
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height));
        assert!(
            frame.width() > 0.0,
            "the card had no room at the minimum window"
        );
        assert!(
            frame.width() <= extent::SIGNIN_W + 2.0 * stroke::HAIRLINE,
            "the painted card grew past its own measure: {}",
            frame.width()
        );
        assert!(
            screen.contains_rect(frame),
            "the card spills out of the window: {frame:?}"
        );
    }

    #[test]
    fn clicking_the_control_authorizes_and_advances_the_state() {
        // A press and release need several passes, and hit-testing reads the
        // previous pass, so the first pass only records where the control landed.
        let ctx = context();
        let mut state = SignIn::NeedsAuthorization;
        let mut rect = Rect::NOTHING;
        run(&ctx, window(900.0, 600.0), |ui| {
            rect = card(ui, &mut state, &Tokens::DARK)
                .2
                .expect("the prompt offers a control");
        });

        let mut command = None;
        for event in [
            Event::PointerMoved(rect.center()),
            pointer_button(rect.center(), true),
            pointer_button(rect.center(), false),
        ] {
            let mut input = window(900.0, 600.0);
            input.events.push(event);
            run(&ctx, input, |ui| {
                command = show(ui, &mut state, &Tokens::DARK);
            });
        }

        assert_eq!(command, Some(Command::Authorize));
        assert_eq!(state, SignIn::Authorizing);
    }

    #[test]
    fn a_tabbed_sign_in_control_wears_the_ring_and_a_clicked_one_does_not() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut state = SignIn::NeedsAuthorization;
        let mut rect = Rect::NOTHING;
        run(&ctx, window(900.0, 600.0), |ui| {
            rect = card(ui, &mut state, &tokens)
                .2
                .expect("the prompt offers a control");
        });

        let mut command = None;
        let mut draw = |ui: &mut Ui| {
            if let Some(next) = show(ui, &mut state, &tokens) {
                command = Some(next);
            }
        };

        let [press, release] = tab();
        run(&ctx, window(900.0, 600.0), &mut draw);
        let mut input = window(900.0, 600.0);
        input.events.push(press);
        run(&ctx, input, &mut draw);
        let mut input = window(900.0, 600.0);
        input.events.push(release);
        let output = painted(&ctx, input, &mut draw);
        assert_eq!(
            rings_in(&output),
            [rect.expand(extent::FOCUS_OFFSET + stroke::HAIRLINE)],
            "Tab rings the sign-in control"
        );

        let ring = rect.expand(extent::FOCUS_OFFSET + stroke::HAIRLINE);
        let mut frames = Vec::new();
        for event in [
            Event::PointerMoved(rect.center()),
            pointer_button(rect.center(), true),
            pointer_button(rect.center(), false),
        ] {
            let mut input = window(900.0, 600.0);
            input.events.push(event);
            frames.push(rings_in(&painted(&ctx, input, &mut draw)));
        }
        drop(draw);

        assert!(frames[0].contains(&ring), "a pointer move is not input");
        assert!(!frames[1].contains(&ring), "the press hides the ring");
        assert!(
            !frames[2].contains(&ring),
            "a clicked sign-in control wears no ring"
        );

        assert_eq!(command, Some(Command::Authorize));
        assert_eq!(state, SignIn::Authorizing);
    }

    #[test]
    fn authorizing_draws_no_control() {
        let ctx = context();
        let mut state = SignIn::Authorizing;
        let mut button = None;
        run(&ctx, window(900.0, 600.0), |ui| {
            let (command, _, rect) = card(ui, &mut state, &Tokens::DARK);
            assert_eq!(command, None);
            button = rect;
        });
        assert!(
            button.is_none(),
            "an in-flight flow must not offer a second control"
        );
    }
}
