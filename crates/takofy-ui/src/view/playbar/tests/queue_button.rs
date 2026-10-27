use super::super::*;
use super::test_support::*;
use egui::{Event, Modifiers};

fn shown_bar() -> Bar {
    Bar {
        pane: PaneButton::Shown,
        ..Bar::default()
    }
}

#[test]
fn a_click_on_the_queue_button_reports_a_pane_press_and_no_command() {
    for pane in [PaneButton::Shown, PaneButton::Hidden] {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let mut queue = queue_of(4);
        let at = queue_button(heart_bar(), Playbar::Full).center();
        let bar = Bar {
            pane,
            ..Bar::default()
        };

        let replies = click_at(&ctx, &tokens, &mut queue, Playbar::Full, bar, at);

        assert!(
            replies.iter().any(|reply| reply.pane),
            "{pane:?}: the click did not reach the queue button: {replies:?}"
        );
        assert!(
            replies.iter().all(|reply| reply.command.is_none()),
            "{pane:?}: a pane press is the window's, so it asks for no command: {replies:?}"
        );
    }
}

#[test]
fn a_click_on_an_unavailable_queue_button_reports_nothing() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut queue = queue_of(4);
    let at = queue_button(heart_bar(), Playbar::Full).center();

    let replies = click_at(&ctx, &tokens, &mut queue, Playbar::Full, Bar::default(), at);

    assert!(
        replies.iter().all(|reply| !reply.pane),
        "an unavailable button reported a press: {replies:?}"
    );
}

#[test]
fn a_claimed_queue_button_reports_nothing() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut queue = queue_of(4);
    let at = queue_button(heart_bar(), Playbar::Full).center();
    let bar = Bar {
        queue_claimed: true,
        ..shown_bar()
    };

    let replies = click_at(&ctx, &tokens, &mut queue, Playbar::Full, bar, at);

    assert!(
        replies.iter().all(|reply| !reply.pane),
        "a claimed button toggled the pane: {replies:?}"
    );
}

/// A held key must press the pane switch once, because a toggle that read
/// every repeat would flap the window.
#[test]
fn a_held_enter_on_the_queue_button_presses_once() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut queue = queue_of(4);
    let bar = shown_bar();

    let _ = queue_frame(&ctx, &tokens, &mut queue, Playbar::Full, bar, Vec::new());
    for _ in 0..7 {
        let _ = queue_frame(
            &ctx,
            &tokens,
            &mut queue,
            Playbar::Full,
            bar,
            vec![key(Key::Tab, Modifiers::NONE)],
        );
    }

    let enter = |repeat| Event::Key {
        key: Key::Enter,
        physical_key: None,
        pressed: true,
        repeat,
        modifiers: Modifiers::NONE,
    };
    let mut presses = Vec::new();
    for repeat in [false, true, true] {
        let (reply, _) = queue_frame(
            &ctx,
            &tokens,
            &mut queue,
            Playbar::Full,
            bar,
            vec![enter(repeat)],
        );
        presses.push(reply.pane);
    }

    assert_eq!(
        presses,
        vec![true, false, false],
        "a held Enter toggled the pane more than once"
    );
}

/// The reduced form draws the queue in the transport, so the same press has
/// to reach the same channel from that column.
#[test]
fn the_reduced_form_places_and_reports_the_queue_press_the_same_way() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut queue = queue_of(4);
    let bar = shown_bar();
    let at = queue_button(heart_bar(), Playbar::Reduced).center();

    let replies = click_at(&ctx, &tokens, &mut queue, Playbar::Reduced, bar, at);

    assert!(
        replies.iter().any(|reply| reply.pane),
        "the reduced transport's queue press went nowhere: {replies:?}"
    );
}

#[test]
fn a_lit_queue_paints_the_accent_and_an_unlit_one_does_not() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut queue = queue_of(4);
    let at = queue_button(heart_bar(), Playbar::Full);

    for (bar, lit) in [
        (shown_bar(), true),
        (
            Bar {
                pane: PaneButton::Hidden,
                ..Bar::default()
            },
            false,
        ),
        (Bar::default(), false),
    ] {
        let (_, output) = queue_frame(&ctx, &tokens, &mut queue, Playbar::Full, bar, Vec::new());
        assert_eq!(
            accent_inside(&output, &tokens, at),
            lit,
            "{:?} drew the accent {lit}",
            bar.pane
        );
    }
}
