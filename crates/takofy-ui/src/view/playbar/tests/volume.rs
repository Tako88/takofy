use super::super::*;
use super::test_support::*;
use egui::{Event, RawInput};

/// The rect the extras draw `control` into, walked the way the draw walks.
fn extras_slot(extras: Rect, form: Playbar, control: Control) -> Rect {
    let runs = extras_controls(form);
    let mut x = extras.right() - run_width(runs);
    for (index, run) in runs.iter().enumerate() {
        if index > 0 {
            x += space::S1;
        }
        let rect = if run.control == VOLUME {
            x += space::S2;
            let rect = Rect::from_min_size(
                Pos2::new(x, extras.center().y - extent::SLIDER_H / 2.0),
                Vec2::new(extent::VOLUME_W, extent::SLIDER_H),
            );
            x += extent::VOLUME_W + space::S2;
            rect
        } else {
            let rect = Rect::from_min_size(
                Pos2::new(x, extras.center().y - extent::BUTTON / 2.0),
                Vec2::splat(extent::BUTTON),
            );
            x += extent::BUTTON;
            rect
        };
        if run.control == control {
            return rect;
        }
    }
    panic!("{control:?} is not one of the extras' runs");
}

/// One frame of the bar over this transport, returning what it asked for.
fn volume_frame(
    ctx: &egui::Context,
    tokens: &Tokens,
    transport: &mut Transport,
    form: Playbar,
    events: Vec<Event>,
) -> Reply {
    let rect = heart_bar();
    let mut queue = Queue::default();
    let mut reply = Reply::default();
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 300.0))),
            events,
            ..Default::default()
        },
        |ui| {
            reply = show(
                ui,
                transport,
                &mut queue,
                0,
                form,
                Bar::default(),
                None,
                rect,
                crate::cover::Cover::Missing,
                tokens,
            );
        },
    );
    output.textures_delta.clear();
    reply
}

/// Clicks `at` over the passes a click takes, collecting the levels the
/// frames left owed to the player.
fn click_volume(
    ctx: &egui::Context,
    tokens: &Tokens,
    transport: &mut Transport,
    form: Playbar,
    at: Pos2,
    asked: &mut Vec<Command>,
) {
    let _ = volume_frame(ctx, tokens, transport, form, Vec::new());
    let _ = volume_frame(ctx, tokens, transport, form, vec![Event::PointerMoved(at)]);
    let _ = transport.take_volume();
    for event in crate::view::test_support::click_events(at) {
        let _ = volume_frame(ctx, tokens, transport, form, vec![event]);
        if let Some(command) = transport.take_volume() {
            asked.push(command);
        }
    }
}

#[test]
fn the_volume_slider_steps_with_its_arrow_keys() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut volume = crate::volume::Volume::at(50);
    let rect = Rect::from_min_size(Pos2::new(0.0, 100.0), Vec2::new(120.0, extent::SLIDER_H));
    let input = |events: Vec<Event>| RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 300.0))),
        events,
        ..Default::default()
    };

    // Only Tab focuses a control and a test has no pointer to Tab with, so
    // the slider is focused by hand.
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let slot = controls::slider(
            ui,
            &tokens,
            VOLUME,
            rect,
            Some(volume.fraction()),
            "Volume",
            true,
        );
        ui.memory_mut(|memory| memory.request_focus(slot.response.id));
        drag_volume(ui, &mut volume, &slot, rect);
    });
    output.textures_delta.clear();

    for _ in 0..2 {
        let mut output = ctx.run_ui(
            input(vec![Event::Key {
                key: Key::ArrowRight,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]),
            |ui| {
                let slot = controls::slider(
                    ui,
                    &tokens,
                    VOLUME,
                    rect,
                    Some(volume.fraction()),
                    "Volume",
                    true,
                );
                drag_volume(ui, &mut volume, &slot, rect);
            },
        );
        output.textures_delta.clear();
    }

    assert_eq!(
        volume.level(),
        50 + 2 * crate::volume::STEP,
        "two steps of the arrow key did not move the level"
    );
}

#[test]
fn clicking_the_volume_slider_sets_the_level_and_asks_for_it() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut transport = playing(0);
    let extras = layout::playbar_for(heart_bar(), Playbar::Full).columns(heart_bar())[2];
    let slider = extras_slot(extras, Playbar::Full, VOLUME);
    // A quarter across the track, which is 25% of the way up.
    let at = Pos2::new(slider.left() + slider.width() * 0.25, slider.center().y);

    let mut asked = Vec::new();
    click_volume(&ctx, &tokens, &mut transport, Playbar::Full, at, &mut asked);

    assert_eq!(transport.volume.level(), 25);
    assert!(
        asked.contains(&Command::Volume { percent: 25 }),
        "the slider did not ask the player for the level: {asked:?}"
    );
}

#[test]
fn dragging_the_volume_slider_sounds_as_it_moves() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut transport = playing(0);
    let extras = layout::playbar_for(heart_bar(), Playbar::Full).columns(heart_bar())[2];
    let slider = extras_slot(extras, Playbar::Full, VOLUME);
    let at =
        |fraction: f32| Pos2::new(slider.left() + slider.width() * fraction, slider.center().y);

    let _ = volume_frame(&ctx, &tokens, &mut transport, Playbar::Full, Vec::new());
    let _ = volume_frame(
        &ctx,
        &tokens,
        &mut transport,
        Playbar::Full,
        vec![Event::PointerMoved(at(0.8))],
    );
    let press = |pos: Pos2, pressed: bool| Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let mut asked = Vec::new();
    // A press on its own is not yet a drag, so the first level asked for is
    // the one the pointer moved to.
    for event in [
        press(at(0.8), true),
        Event::PointerMoved(at(0.4)),
        Event::PointerMoved(at(0.1)),
        press(at(0.1), false),
    ] {
        let _ = volume_frame(&ctx, &tokens, &mut transport, Playbar::Full, vec![event]);
        if let Some(command) = transport.take_volume() {
            asked.push(command);
        }
    }

    assert_eq!(
        asked,
        vec![
            Command::Volume { percent: 40 },
            Command::Volume { percent: 10 },
        ],
        "a drag asked for something other than the levels it passed"
    );
    assert_eq!(transport.volume.level(), 10, "and stopped where it let go");
}

#[test]
fn the_mute_button_silences_and_restores_and_says_which_it_is() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut transport = playing(0);
    transport.volume = crate::volume::Volume::at(70);
    let extras = layout::playbar_for(heart_bar(), Playbar::Full).columns(heart_bar())[2];
    let mute = extras_slot(extras, Playbar::Full, Control::Mute).center();

    let mut asked = Vec::new();
    click_volume(
        &ctx,
        &tokens,
        &mut transport,
        Playbar::Full,
        mute,
        &mut asked,
    );
    assert!(transport.volume.muted(), "the speaker did not silence it");
    assert!(
        asked.contains(&Command::Volume { percent: 0 }),
        "silencing did not reach the player: {asked:?}"
    );
    assert_eq!(mute_run(&transport.volume).label, "Unmute");
    assert!(mute_run(&transport.volume).lit);

    asked.clear();
    click_volume(
        &ctx,
        &tokens,
        &mut transport,
        Playbar::Full,
        mute,
        &mut asked,
    );
    assert!(!transport.volume.muted());
    assert!(
        asked.contains(&Command::Volume { percent: 70 }),
        "unmuting did not bring the level back: {asked:?}"
    );
    assert_eq!(mute_run(&transport.volume).label, "Mute");
    assert!(!mute_run(&transport.volume).lit);
}
