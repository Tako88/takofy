use super::{pasted, pressed, pressed_once};
use egui::{Context, Event, Key, Modifiers, RawInput};

/// One frame of `events` on `ctx`, answering what both readers say for
/// `key`.
fn frame(ctx: &Context, events: Vec<Event>, key: Key) -> (bool, bool) {
    let input = RawInput {
        events,
        ..Default::default()
    };
    let mut answers = (false, false);
    let mut output = ctx.run_ui(input, |ui| {
        ui.input(|state| answers = (pressed(state, key), pressed_once(state, key)));
    });
    output.textures_delta.clear();
    answers
}

fn press(key: Key, repeat: bool, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat,
        modifiers,
    }
}

/// One frame of `events` on `ctx`, answering whether it holds a paste.
fn paste_frame(ctx: &Context, events: Vec<Event>) -> bool {
    let input = RawInput {
        events,
        ..Default::default()
    };
    let mut answer = false;
    let mut output = ctx.run_ui(input, |ui| {
        ui.input(|state| answer = pasted(state));
    });
    output.textures_delta.clear();
    answer
}

#[test]
fn a_repeat_counts_as_a_press_but_not_as_a_first_one() {
    let ctx = Context::default();
    assert_eq!(
        frame(
            &ctx,
            vec![press(Key::Enter, false, Modifiers::NONE)],
            Key::Enter
        ),
        (true, true),
        "the first press is a first press"
    );
    // egui decides a repeat against the pass before it, so the held key is
    // only a repeat once the key is already down.
    assert_eq!(
        frame(
            &ctx,
            vec![press(Key::Enter, true, Modifiers::NONE)],
            Key::Enter
        ),
        (true, false),
        "a held key acts once"
    );
}

#[test]
fn a_modified_key_is_somebody_elses() {
    let ctx = Context::default();
    assert_eq!(
        frame(
            &ctx,
            vec![press(Key::Escape, false, Modifiers::ALT)],
            Key::Escape
        ),
        (false, false)
    );
}

#[test]
fn a_release_is_not_a_press() {
    let ctx = Context::default();
    let release = Event::Key {
        key: Key::Enter,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    assert_eq!(frame(&ctx, vec![release], Key::Enter), (false, false));
}

#[test]
fn a_paste_event_is_a_paste() {
    let ctx = Context::default();
    assert!(paste_frame(
        &ctx,
        vec![Event::Paste("spotify:track:x".to_owned())]
    ));
}

#[test]
fn typing_and_the_v_key_are_not_a_paste() {
    let ctx = Context::default();
    assert!(!paste_frame(&ctx, vec![Event::Text("v".to_owned())]));
    assert!(!paste_frame(
        &ctx,
        vec![press(Key::V, false, Modifiers::CTRL)]
    ));
}

#[test]
fn no_events_is_not_a_paste() {
    let ctx = Context::default();
    assert!(!paste_frame(&ctx, Vec::new()));
}

/// The PRIMARY selection is a paste the pointer makes, and it reaches the
/// field as typing rather than as a key.
#[test]
fn middle_click_paste_is_not_counted() {
    let ctx = Context::default();
    assert!(!paste_frame(
        &ctx,
        vec![Event::MiddleClickPaste {
            pos: egui::Pos2::ZERO,
            text: "spotify:track:x".to_owned(),
        }]
    ));
}
