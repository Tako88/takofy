use super::super::*;
use super::test_support::*;
use crate::view::test_support::{button, click_events};
use egui::{Context, Event, Modifiers, RawInput};
use std::time::Duration;

/// A transport playing a track whose length is known, which is the only state
/// the seek can act in.
fn seekable() -> Transport {
    Transport {
        state: crate::player::TransportState::Playing,
        duration_ms: Some(100_000),
        ..Default::default()
    }
}

fn track_rect() -> Rect {
    Rect::from_min_size(Pos2::new(80.0, 0.0), Vec2::new(200.0, extent::SLIDER_H))
}

#[test]
fn a_loading_transport_fills_none_of_the_seek() {
    let mut transport = seekable();
    assert!(
        seek_fraction(&transport, 0).is_some(),
        "a playing track has a position to show"
    );
    transport.state = TransportState::Loading;
    assert_eq!(
        seek_fraction(&transport, 0),
        None,
        "a track still loading has no position, so the bar claims none"
    );
}

/// Two ready neighbours and the seek, so focus has somewhere else to go if
/// an arrow is not locked to the track.
fn seek_controls(
    ui: &mut Ui,
    transport: &mut Transport,
    tokens: &Tokens,
) -> (Option<Command>, bool) {
    let neighbour = |ui: &mut Ui, x: f32, control, icon, label| {
        controls::icon_button(
            ui,
            tokens,
            control,
            Rect::from_min_size(Pos2::new(x, 0.0), Vec2::splat(28.0)),
            icon,
            Size::MD,
            label,
            true,
        )
    };
    neighbour(ui, 0.0, Control::Previous, Icon::Previous, "Previous");
    neighbour(ui, 40.0, Control::Next, Icon::Next, "Next");

    let track = track_rect();
    let seek = controls::slider(
        ui,
        tokens,
        Control::Seek,
        track,
        transport.fraction(0),
        "Seek",
        true,
    );
    if !seek.response.has_focus() {
        seek.response.request_focus();
    }
    let command = seek_command(ui, transport, &seek, track, 0, SeekScope::Local);
    (command, seek.response.has_focus())
}

fn click(ctx: &Context, at: Pos2, transport: &mut Transport, tokens: &Tokens) -> Option<Command> {
    run(ctx, input(Vec::new()), |ui| {
        let _ = seek_controls(ui, &mut *transport, tokens);
    });
    let mut command = None;
    for event in click_events(at) {
        let command = &mut command;
        run(ctx, input(vec![event]), |ui| {
            *command = seek_controls(ui, &mut *transport, tokens).0;
        });
    }
    command
}

#[test]
fn a_drag_previews_under_the_pointer_and_asks_once_it_is_let_go() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let track = track_rect();
    let at = |fraction: f32| Pos2::new(track.left() + fraction * track.width(), track.center().y);
    let mut transport = seekable();

    // A pointer has to exist for a frame before it can press anything.
    run(&ctx, input(Vec::new()), |ui| {
        let _ = seek_controls(ui, &mut transport, &tokens);
    });

    let mut command = None;
    for event in [
        Event::PointerMoved(at(0.2)),
        button(at(0.2), true),
        Event::PointerMoved(at(0.7)),
        Event::PointerMoved(at(0.8)),
    ] {
        let command = &mut command;
        run(&ctx, input(vec![event]), |ui| {
            *command = seek_controls(ui, &mut transport, &tokens).0;
        });
    }
    assert_eq!(command, None, "a drag asks for nothing while it runs");
    assert_eq!(
        transport.shown_position(Duration::ZERO),
        80_000,
        "the bar follows the pointer, not the track",
    );

    run(&ctx, input(vec![button(at(0.8), false)]), |ui| {
        let command = &mut command;
        *command = seek_controls(ui, &mut transport, &tokens).0;
    });
    assert_eq!(
        command,
        Some(Command::Seek {
            position_ms: 80_000
        }),
        "and asks once, where it was let go",
    );
}

#[test]
fn a_click_at_a_fraction_of_the_track_asks_for_that_position() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let track = track_rect();
    let at = Pos2::new(track.left() + 0.25 * track.width(), track.center().y);

    assert_eq!(
        click(&ctx, at, &mut playing(0), &tokens),
        Some(Command::Seek {
            position_ms: 55_500
        })
    );
}

#[test]
fn a_click_on_an_unseekable_track_asks_for_nothing() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let track = track_rect();
    let at = Pos2::new(track.left() + 0.25 * track.width(), track.center().y);
    let mut transport = Transport {
        state: TransportState::Playing,
        ..Transport::default()
    };

    assert_eq!(click(&ctx, at, &mut transport, &tokens), None);
}

#[test]
fn the_seek_keys_ask_for_their_steps_and_keep_focus() {
    let cases: [(Key, Modifiers, u32); 5] = [
        (Key::ArrowRight, Modifiers::NONE, 15_000),
        (Key::ArrowLeft, Modifiers::NONE, 5_000),
        (Key::ArrowRight, Modifiers::SHIFT, 30_000),
        (Key::Home, Modifiers::NONE, 0),
        (Key::End, Modifiers::NONE, 221_999),
    ];
    for (pressed, modifiers, expected) in cases {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let mut transport = playing(10_000);

        // First pass gives the seek focus; the second installs the arrow
        // lock, which only applies to the frame after it is set.
        run(&ctx, input(Vec::new()), |ui| {
            let _ = seek_controls(ui, &mut transport, &tokens);
        });
        run(&ctx, input(Vec::new()), |ui| {
            let _ = seek_controls(ui, &mut transport, &tokens);
        });

        let mut command = None;
        let mut focused = false;
        run(
            &ctx,
            input(vec![
                Event::ModifiersChanged(modifiers),
                key(pressed, modifiers),
            ]),
            |ui| {
                let (next, has_focus) = seek_controls(ui, &mut transport, &tokens);
                command = next;
                focused = has_focus;
            },
        );

        assert_eq!(
            command,
            Some(Command::Seek {
                position_ms: expected
            }),
            "{pressed:?} {modifiers:?}"
        );
        assert!(focused, "the arrow moved focus off the seek: {pressed:?}");
    }
}

#[test]
fn the_seek_row_measures_512_end_to_end() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let (_, output) = bar_at(&ctx, &tokens, 1200.0, Playbar::Full, &mut queue_of(4));
    let elapsed = painted_text(&output, "0:00");
    let remaining = painted_text(&output, "-3:42");
    let measured = remaining.right() - elapsed.left();
    assert!(
        (measured - 512.0).abs() < 0.01,
        "the row measures {measured} from the first readout's left edge to the last one's right"
    );
}

#[test]
fn the_seek_row_stays_inside_the_centre_column_at_every_supported_width() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    for width in 640..=2000 {
        let width = width as f32;
        let form = layout::regions_for(width).playbar;
        let (cols, output) = bar_at(&ctx, &tokens, width, form, &mut queue_of(4));
        let column = cols[1];
        for text in ["0:00", "-3:42"] {
            let rect = painted_text(&output, text);
            assert!(
                rect.left() >= column.left() - 1e-3 && rect.right() <= column.right() + 1e-3,
                "{width}: {form:?} paints {text} at {rect:?} outside {column:?}"
            );
        }
        let track = painted_track(&output, column);
        assert!(
            track.left() >= column.left() - 1e-3 && track.right() <= column.right() + 1e-3,
            "{width}: {form:?} paints the track at {track:?} outside {column:?}"
        );
    }
}

/// One frame of the bar at `position_ms`, and the value its seek announced.
fn seek_announced(
    ctx: &egui::Context,
    tokens: &Tokens,
    transport: &mut Transport,
    position_ms: u32,
) -> Option<String> {
    let mut queue = queue_of(4);
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 300.0))),
            ..Default::default()
        },
        |ui| {
            let _ = show(
                ui,
                transport,
                &mut queue,
                position_ms,
                Playbar::Full,
                Bar::default(),
                None,
                heart_bar(),
                Cover::Missing,
                tokens,
            );
        },
    );
    output.textures_delta.clear();
    announced_value(&output, "Seek")
}

/// Both readouts are painted galleys, so the seek's value is the only place a
/// screen reader hears the time.
#[test]
fn the_seek_announces_both_readouts() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    ctx.enable_accesskit();
    let mut transport = playing(42_000);

    assert_eq!(
        seek_announced(&ctx, &tokens, &mut transport, 42_000).as_deref(),
        Some("0:42 of 3:42"),
        "the seek does not say where it is and how long the track is"
    );

    transport.duration_ms = None;
    assert_eq!(
        seek_announced(&ctx, &tokens, &mut transport, 42_000).as_deref(),
        Some("0:42"),
        "a track of unknown length announced a total it does not know"
    );
}

/// One frame of the bar over `transport`, so what a drag recorded survives
/// into the frame that releases it.
fn seek_pass(
    ctx: &egui::Context,
    tokens: &Tokens,
    transport: &mut Transport,
    event: Event,
) -> Reply {
    let bar = heart_bar();
    let mut queue = queue_of(4);
    let mut reply = Reply::default();
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 300.0))),
            events: vec![event],
            ..Default::default()
        },
        |ui| {
            reply = show(
                ui,
                transport,
                &mut queue,
                0,
                Playbar::Full,
                Bar::default(),
                None,
                bar,
                Cover::Missing,
                tokens,
            );
        },
    );
    output.textures_delta.clear();
    reply
}

#[test]
fn a_seek_drag_across_the_new_track_maps_the_endpoints_to_zero_and_the_duration() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut transport = playing(0);
    let (_, output) = bar_at(&ctx, &tokens, 1200.0, Playbar::Full, &mut queue_of(4));
    let track = painted_track(
        &output,
        layout::playbar_for(heart_bar(), Playbar::Full).columns(heart_bar())[1],
    );

    // The left edge is the start of the track.
    let _ = seek_pass(
        &ctx,
        &tokens,
        &mut transport,
        Event::PointerMoved(track.left_center()),
    );
    let _ = seek_pass(
        &ctx,
        &tokens,
        &mut transport,
        button(track.left_center(), true),
    );
    let left = seek_pass(
        &ctx,
        &tokens,
        &mut transport,
        button(track.left_center(), false),
    );
    assert_eq!(
        left.command,
        Some(Command::Seek { position_ms: 0 }),
        "a press at the left edge did not ask for the start"
    );

    // A drag from the left edge to the right one lands on the last position
    // the track can hold.
    let _ = seek_pass(
        &ctx,
        &tokens,
        &mut transport,
        Event::PointerMoved(track.left_center()),
    );
    let _ = seek_pass(
        &ctx,
        &tokens,
        &mut transport,
        button(track.left_center(), true),
    );
    let _ = seek_pass(
        &ctx,
        &tokens,
        &mut transport,
        Event::PointerMoved(track.right_center()),
    );
    let end = seek_pass(
        &ctx,
        &tokens,
        &mut transport,
        button(track.right_center(), false),
    );
    assert_eq!(
        end.command,
        Some(Command::Seek {
            position_ms: 221_999
        }),
        "a drag to the right edge did not ask for the end"
    );
}

/// The value a frame announced for the node labelled `label`.
fn announced_value(output: &egui::FullOutput, label: &str) -> Option<String> {
    output
        .platform_output
        .accesskit_update
        .as_ref()?
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(label))
        .and_then(|(_, node)| node.value().map(str::to_owned))
}
