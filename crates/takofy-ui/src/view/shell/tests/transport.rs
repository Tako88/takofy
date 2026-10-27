use super::super::*; // the module under test — what `use super::*` gave the flat module
use super::*; // the slice's shared fixtures, from tests/mod.rs

#[test]
fn space_pauses_and_resumes_the_transport() {
    let mut transport = Transport::default();
    let mut queue = two_tracks(&mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));

    for (state, expected) in [
        (TransportState::Playing, Command::Pause),
        (TransportState::Paused, Command::Resume),
        (TransportState::Loading, Command::Pause),
    ] {
        transport.state = state;
        let (command, _) = tap(
            &mut window,
            &mut transport,
            &mut queue,
            key(egui::Key::Space, true),
        );
        assert_eq!(command, Some(expected), "a space on {state:?}");
    }
}

#[test]
fn space_with_nothing_queued_asks_for_nothing() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));

    let (command, _) = tap(
        &mut window,
        &mut transport,
        &mut queue,
        key(egui::Key::Space, true),
    );
    assert_eq!(command, None, "there is no track to start");
}

#[test]
fn a_held_space_does_not_thrash_the_transport() {
    let mut transport = Transport::default();
    let mut queue = two_tracks(&mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    transport.state = TransportState::Playing;

    let (first, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::Space, true)],
    );
    assert_eq!(first, Some(Command::Pause));

    // The same key pressed again with no release: what a held key looks like.
    let (held, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::Space, true)],
    );
    assert_eq!(held, None, "a held space must not thrash the transport");
}

#[test]
fn a_clicked_row_keeps_the_keyboard_and_lends_the_space_to_the_transport() {
    // A click leaves the keyboard where the pointer went, so Tab carries on
    // from the row that was picked. The space is still the transport's: the
    // row's own keys are Enter and a double click.
    let mut transport = playing(cover("abc"));
    let tracks = vec![
        track(0, Some(cover("abc"))),
        track(1, Some(cover("row1"))),
        track(2, Some(cover("row2"))),
    ];
    let mut queue = Queue::default();
    queue.start(&tracks, 0, None, &mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = painted_at(&frame, "t2").expect("row t2 is painted");
    let _ = window.click(&mut transport, &mut queue, at + Vec2::new(6.0, 6.0));
    assert!(
        window.ctx.memory(|memory| memory.focused().is_some()),
        "the clicked row holds the keyboard"
    );

    transport.state = TransportState::Playing;
    let (command, _) = tap(
        &mut window,
        &mut transport,
        &mut queue,
        key(egui::Key::Space, true),
    );
    assert_eq!(
        command,
        Some(Command::Pause),
        "the space is the transport's after a click on a row"
    );
}

#[test]
fn a_held_space_never_reaches_a_focused_row() {
    // A row that holds focus would take egui's space click on every repeat of
    // a held key, so the shell takes the repeats away too: one pause, and
    // then nothing at all — not to the transport, and not as a row activation.
    let mut transport = Transport::default();
    let mut queue = two_tracks(&mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    // A library with something to reach: a sidebar row that Tab can focus and
    // that a space would open, so a leaked repeat would show up as a command.
    ready_playlist(&mut window);
    window.library.to_playlists();
    transport.state = TransportState::Playing;

    let _ = window.frame(&mut transport, &mut queue, vec![key(egui::Key::Tab, true)]);
    assert!(
        window.ctx.memory(|memory| memory.focused().is_some()),
        "Tab has to land somewhere for this to mean anything"
    );

    let (first, _) = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::Space, true)],
    );
    assert_eq!(first, Some(Command::Pause));
    let page = window.library.page().cloned();

    for _ in 0..3 {
        let (held, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![key_repeat(egui::Key::Space)],
        );
        assert_eq!(held, None, "a repeat is nobody's press");
    }
    assert_eq!(
        window.library.page().cloned(),
        page,
        "a held space must not open what it holds focus on"
    );
}

#[test]
fn a_space_with_no_loaded_track_asks_for_nothing() {
    // The queue can hold entries without one of them being the transport's,
    // and the space must not pick one: it only ever toggles what is loaded.
    let mut transport = Transport::default();
    let mut queue = two_tracks(&mut transport);
    transport.now_playing = None;
    let mut window = Window::new(Vec2::new(1400.0, 900.0));

    let (command, _) = tap(
        &mut window,
        &mut transport,
        &mut queue,
        key(egui::Key::Space, true),
    );
    assert_eq!(command, None, "there is nothing loaded to pause or start");
}

#[test]
fn a_modified_space_is_somebody_elses_shortcut() {
    let mut transport = Transport::default();
    let mut queue = two_tracks(&mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    transport.state = TransportState::Playing;

    let mut event = key(egui::Key::Space, true);
    if let egui::Event::Key { modifiers, .. } = &mut event {
        *modifiers = egui::Modifiers::COMMAND;
    }
    let (command, _) = tap(&mut window, &mut transport, &mut queue, event);
    assert_eq!(command, None, "a modifier is a shortcut of somebody else's");
}

#[test]
fn a_focused_row_leaves_the_space_to_the_transport() {
    // egui turns a space into a click for whatever holds focus, so the shell
    // has to take it away before anything draws: otherwise Tab onto a row and
    // a space would start that row's track while the transport paused.
    let mut transport = Transport::default();
    let mut queue = two_tracks(&mut transport);
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    transport.state = TransportState::Playing;

    let _ = window.frame(&mut transport, &mut queue, vec![key(egui::Key::Tab, true)]);
    assert!(
        window.ctx.memory(|memory| memory.focused().is_some()),
        "Tab has to land somewhere for this to mean anything"
    );

    let (command, _) = tap(
        &mut window,
        &mut transport,
        &mut queue,
        key(egui::Key::Space, true),
    );
    assert_eq!(
        command,
        Some(Command::Pause),
        "the space is the transport's even on a focused row"
    );
}
