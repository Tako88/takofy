use super::settings_view;

use egui::{Event, Pos2, Rect, Vec2};

use crate::library::Library;
use crate::message::{
    Command, Facts, Freshness, KeyState, SetupFault, SoloistAsk, SoloistUpdate, Update,
};
use crate::prefs::SoundChoice;
use crate::queue::Queue;
use crate::theme::{Tokens, extent, fonts, style};

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx
}

fn input(events: Vec<Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 1200.0))),
        events,
        ..Default::default()
    }
}

fn frame(
    ctx: &egui::Context,
    events: Vec<Event>,
    library: &mut Library,
    queue: &mut Queue,
) -> egui::FullOutput {
    let tokens = Tokens::DARK;
    let mut output = ctx.run_ui(input(events), |ui| {
        crate::view::focus::begin_frame(ui.ctx());
        let _ = settings_view(ui, library, queue, &tokens);
    });
    output.textures_delta.clear();
    output
}

fn painted_text(output: &egui::FullOutput, label: &str) -> (Pos2, Vec2) {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some((text.pos, text.galley.size()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{label:?} is drawn"))
}

fn painted(output: &egui::FullOutput, label: &str) -> bool {
    output.shapes.iter().any(|clipped| {
        matches!(&clipped.shape,
            egui::epaint::Shape::Text(text) if text.galley.text() == label)
    })
}

/// The box a line of text is drawn in, so a press can land on the control that
/// carries it.
fn at_text(output: &egui::FullOutput, label: &str) -> Pos2 {
    let (pos, size) = painted_text(output, label);
    Pos2::new(pos.x + size.x / 2.0, pos.y + size.y / 2.0)
}

fn click(
    ctx: &egui::Context,
    at: Pos2,
    library: &mut Library,
    queue: &mut Queue,
) -> egui::FullOutput {
    let mut last = frame(ctx, Vec::new(), library, queue);
    for event in crate::view::test_support::click_events(at) {
        last = frame(ctx, vec![event], library, queue);
    }
    last
}

/// The autoplay switch's track, found by the one size it is drawn at.
fn switch_track(output: &egui::FullOutput) -> Rect {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(rect)
                if (rect.rect.width() - extent::SWITCH_W).abs() < 0.01
                    && (rect.rect.height() - extent::SWITCH_H).abs() < 0.01 =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
        .expect("the switch draws its own track")
}

fn enter(pressed: bool) -> Event {
    Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

/// A state whose program is found and whose key is not stored, which is the only
/// frame the key field is drawn in.
fn found_program() -> Update {
    Update::Soloist(SoloistUpdate::Facts(Facts {
        program: Some("~/Downloads/soloist".to_owned()),
        build: Freshness::Fresh,
        built: None,
        expires: None,
        key: KeyState::Absent,
        looked_in_downloads: true,
    }))
}

#[test]
fn a_pressed_engine_row_chooses_it() {
    let ctx = context();
    let mut library = Library::default();
    let mut queue = Queue::default();
    let output = frame(&ctx, Vec::new(), &mut library, &mut queue);

    let soloist = at_text(&output, "Spotify Soloist");
    let _ = click(&ctx, soloist, &mut library, &mut queue);
    assert_eq!(library.settings.chosen(), SoundChoice::Soloist);
    assert!(
        library.settings.panel(),
        "choosing Soloist opens the setup it still needs"
    );

    let output = frame(&ctx, Vec::new(), &mut library, &mut queue);
    let built_in = at_text(&output, "Takofy");
    let _ = click(&ctx, built_in, &mut library, &mut queue);
    assert_eq!(library.settings.chosen(), SoundChoice::BuiltIn);
}

#[test]
fn the_switch_moves_the_queue() {
    let ctx = context();
    let mut library = Library::default();
    let mut queue = Queue::default();
    assert!(queue.autoplay(), "the queue starts playing on");

    let output = frame(&ctx, Vec::new(), &mut library, &mut queue);
    let _ = click(
        &ctx,
        switch_track(&output).center(),
        &mut library,
        &mut queue,
    );
    assert!(!queue.autoplay(), "the press turns autoplay off");

    let output = frame(&ctx, Vec::new(), &mut library, &mut queue);
    let _ = click(
        &ctx,
        switch_track(&output).center(),
        &mut library,
        &mut queue,
    );
    assert!(queue.autoplay(), "and the next press turns it back on");
}

#[test]
fn tab_reaches_every_live_control() {
    let ctx = context();
    let mut library = Library::default();
    library.settings.reveal_setup();
    let mut queue = Queue::default();
    let _ = frame(&ctx, Vec::new(), &mut library, &mut queue);

    // The two engine rows, the program step's three, the key step's two, and the
    // switch. Pairing is not live while the key is missing.
    let live = 8;
    let mut seen: Vec<egui::Id> = Vec::new();
    for _ in 0..live {
        let [press, release] = crate::view::test_support::tab();
        let _ = frame(&ctx, vec![press], &mut library, &mut queue);
        let _ = frame(&ctx, vec![release], &mut library, &mut queue);
        let focused = ctx.memory(|memory| memory.focused());
        let id = focused.expect("every Tab press stops on a control");
        assert!(
            !seen.contains(&id),
            "no control takes two Tab stops: {id:?}"
        );
        seen.push(id);
    }
}

#[test]
fn the_key_field_sends_one_key_and_keeps_none() {
    let ctx = context();
    let mut library = Library::default();
    library.settings.reveal_setup();
    library.settings.apply(&found_program());
    let mut queue = Queue::default();
    let output = frame(&ctx, Vec::new(), &mut library, &mut queue);
    assert!(painted(&output, "Get your key"), "the key step is drawn");

    let _ = click(
        &ctx,
        at_text(&output, "Paste the key"),
        &mut library,
        &mut queue,
    );
    let key = "soloist-key-that-must-not-be-logged";
    let _ = frame(
        &ctx,
        vec![Event::Text(key.to_owned())],
        &mut library,
        &mut queue,
    );
    assert_eq!(library.settings.key_field_mut().as_str(), key);

    let _ = frame(&ctx, vec![enter(true)], &mut library, &mut queue);
    let _ = frame(&ctx, vec![enter(false)], &mut library, &mut queue);

    let drained = library.settings.take_outbox();
    assert_eq!(drained.len(), 1, "one submit carries the key out once");
    assert!(matches!(
        &drained[0],
        Command::Soloist(SoloistAsk::StoreKey(_))
    ));
    assert!(!format!("{drained:?}").contains(key), "the command redacts");
    assert!(
        library.settings.key_field_mut().is_empty(),
        "the field is emptied by the submit"
    );
    assert!(
        !format!("{:?}", library.settings).contains(key),
        "the page's state redacts"
    );
}

#[test]
fn an_empty_key_is_refused_with_its_own_reason() {
    let ctx = context();
    let mut library = Library::default();
    library.settings.reveal_setup();
    library.settings.apply(&found_program());
    let mut queue = Queue::default();
    let output = frame(&ctx, Vec::new(), &mut library, &mut queue);

    let _ = click(
        &ctx,
        at_text(&output, "Paste the key"),
        &mut library,
        &mut queue,
    );
    let _ = frame(&ctx, vec![enter(true)], &mut library, &mut queue);
    let _ = frame(&ctx, vec![enter(false)], &mut library, &mut queue);

    assert!(library.settings.take_outbox().is_empty());
    assert_eq!(library.settings.fault(), Some(SetupFault::KeyEmpty));
    let output = frame(&ctx, Vec::new(), &mut library, &mut queue);
    assert!(painted(&output, "Paste the key first."));
}

#[test]
fn the_page_draws_no_sound_colour() {
    let ctx = context();
    let mut library = Library::default();
    library.settings.reveal_setup();
    let mut queue = Queue::default();
    let output = frame(&ctx, Vec::new(), &mut library, &mut queue);
    let tokens = Tokens::DARK;
    let green = output.shapes.iter().any(|clipped| match &clipped.shape {
        egui::epaint::Shape::Rect(rect) => rect.fill == tokens.palette.accent,
        _ => false,
    });
    assert!(!green, "no row of this page is drawn in the accent");
}

#[test]
fn a_pasted_key_is_saved_without_pressing_enter() {
    let ctx = context();
    let mut library = Library::default();
    library.settings.reveal_setup();
    library.settings.apply(&found_program());
    let mut queue = Queue::default();
    let output = frame(&ctx, Vec::new(), &mut library, &mut queue);

    let _ = click(
        &ctx,
        at_text(&output, "Paste the key"),
        &mut library,
        &mut queue,
    );
    let key = "soloist-key-pasted-not-typed";
    let _ = frame(
        &ctx,
        vec![Event::Text(key.to_owned())],
        &mut library,
        &mut queue,
    );

    // A paste is not a keystroke ending in Enter, so the page has to offer the
    // action the field cannot: this press is the whole point.
    let output = frame(&ctx, Vec::new(), &mut library, &mut queue);
    let _ = click(
        &ctx,
        at_text(&output, "SAVE THE KEY"),
        &mut library,
        &mut queue,
    );

    let drained = library.settings.take_outbox();
    assert_eq!(drained.len(), 1, "the press carries the key out once");
    assert!(matches!(
        &drained[0],
        Command::Soloist(SoloistAsk::StoreKey(_))
    ));
    assert!(!format!("{drained:?}").contains(key), "the command redacts");
}

#[test]
fn saving_is_inert_with_nothing_to_save() {
    let ctx = context();
    let mut library = Library::default();
    library.settings.reveal_setup();
    library.settings.apply(&found_program());
    let mut queue = Queue::default();
    let output = frame(&ctx, Vec::new(), &mut library, &mut queue);

    let _ = click(
        &ctx,
        at_text(&output, "SAVE THE KEY"),
        &mut library,
        &mut queue,
    );

    assert!(
        library.settings.take_outbox().is_empty(),
        "an empty field sends nothing"
    );
}
