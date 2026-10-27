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
