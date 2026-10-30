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
                    // A window narrower than the card's own margins would give
                    // the block a negative width, which egui refuses.
                    ui.set_width((card - 2.0 * space::S6).max(0.0));
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
mod tests;
