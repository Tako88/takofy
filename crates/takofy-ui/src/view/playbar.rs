//! The playbar: what is playing, the transport, and the extras.
//!
//! Everything it shows comes from the transport — the title, the state, the
//! elapsed time, the glyph — and its controls report clicks rather than change
//! state. The seek track fills from the interpolation and turns a click or an
//! arrow into a command; a track with no known duration draws the track alone
//! and a dash for the time left.

use egui::{EventFilter, FontId, Key, Pos2, Rect, Ui, Vec2};

use crate::availability::Control;
use crate::cover::Cover;
use crate::format::{format_position, format_remaining};
use crate::icon::Icon;
use crate::layout::{self, Playbar};
use crate::message::Command;
use crate::player::{SEEK_STEP_MS, Transport, TransportState};
use crate::queue::Queue;
use crate::theme::icon::Size;
use crate::theme::{MonoWeight, Tokens, Weight, extent, fonts, radius, space, text, tracking};

use super::art;
use super::controls;
use super::shell::{row_centre, row_centred};
use super::skeleton;
use super::text as recipe;

/// The one control in each column that is not an icon button.
const VOLUME: Control = Control::Volume;

/// The controls each column holds, with the glyph each draws.
///
/// The layout reserves the extras' width from the same shape, and a test here
/// pins the two together so a control added to one cannot drift from the other.
const NOW_FULL: [(Control, Icon); 2] = [
    (Control::Like, Icon::Heart),
    (Control::Miniplayer, Icon::Screen),
];
const NOW_REDUCED: [(Control, Icon); 1] = [(Control::Like, Icon::Heart)];
const EXTRAS_FULL: [(Control, Icon); 5] = [
    (Control::Queue, Icon::Queue),
    (Control::Devices, Icon::Devices),
    (Control::Mute, Icon::VolumeMute),
    (VOLUME, Icon::Volume),
    (Control::Fullscreen, Icon::Expand),
];
const EXTRAS_REDUCED: [(Control, Icon); 3] = [
    (Control::Mute, Icon::VolumeMute),
    (VOLUME, Icon::Volume),
    (Control::Fullscreen, Icon::Expand),
];

const DOWN: [(Control, Icon); 2] = [
    (Control::Shuffle, Icon::Shuffle),
    (Control::Previous, Icon::Previous),
];
const UP_FULL: [(Control, Icon); 2] =
    [(Control::Next, Icon::Next), (Control::Repeat, Icon::Repeat)];
/// Below 1000px the extras shed the queue, so it joins the transport after
/// repeat. Play stays centred: the row centres on `DOWN`.
const UP_REDUCED: [(Control, Icon); 3] = [
    (Control::Next, Icon::Next),
    (Control::Repeat, Icon::Repeat),
    (Control::Queue, Icon::Queue),
];

fn up_controls(form: Playbar) -> &'static [(Control, Icon)] {
    match form {
        Playbar::Full => &UP_FULL,
        Playbar::Reduced => &UP_REDUCED,
    }
}

fn now_controls(form: Playbar) -> &'static [(Control, Icon)] {
    match form {
        Playbar::Full => &NOW_FULL,
        Playbar::Reduced => &NOW_REDUCED,
    }
}

fn extras_controls(form: Playbar) -> &'static [(Control, Icon)] {
    match form {
        Playbar::Full => &EXTRAS_FULL,
        Playbar::Reduced => &EXTRAS_REDUCED,
    }
}

/// Draws the bar's three columns inside `rect` and returns the command a
/// control in it asked for.
#[expect(
    clippy::too_many_arguments,
    reason = "the bar draws three columns from the transport, the queue, the carry and the frame"
)]
pub fn show(
    ui: &mut Ui,
    transport: &mut Transport,
    queue: &mut Queue,
    position_ms: u32,
    form: Playbar,
    queue_claimed: bool,
    rect: Rect,
    cover: Cover,
    tokens: &Tokens,
) -> Option<Command> {
    let cols = layout::playbar_for(rect, form);
    let [now, centre, extras] = cols.columns(rect);
    let claimed = queue_claimed.then_some(Control::Queue);

    row_centred(ui, "playbar-now", now, |ui| {
        now_column(ui, transport, cover, tokens, form);
    });
    let command = row_centred(ui, "playbar-centre", centre, |ui| {
        centre_column(ui, transport, queue, position_ms, form, claimed, tokens)
    });
    row_centred(ui, "playbar-extras", extras, |ui| {
        extras_column(ui, tokens, form, claimed);
    });
    command
}

/// The glyph a play control shows, given what the transport is doing.
pub fn play_glyph(state: TransportState) -> Icon {
    if state == TransportState::Playing {
        Icon::Pause
    } else {
        Icon::Play
    }
}

/// The share of the seek track the fill covers.
///
/// A loading track has no position to show: a fill would claim playback had
/// begun, and the playhead would sit at a place the player has not reached. The
/// bar draws a band that claims no total instead.
fn seek_fraction(transport: &Transport, position_ms: u32) -> Option<f32> {
    if transport.state == TransportState::Loading {
        return None;
    }
    transport.fraction(position_ms)
}

/// The width one run of controls occupies, margins and gaps included. The view
/// draws it and the layout reserves it, and both read this.
fn run_width(controls: &[(Control, Icon)]) -> f32 {
    let mut width = 0.0;
    for (index, (control, _)) in controls.iter().enumerate() {
        if index > 0 {
            width += space::S1;
        }
        width += if *control == VOLUME {
            extent::VOLUME_W + 2.0 * space::S2
        } else {
            extent::BUTTON
        };
    }
    width
}

/// The art tile, the title, the state, and the buttons beside them.
fn now_column(ui: &mut Ui, transport: &Transport, cover: Cover, tokens: &Tokens, form: Playbar) {
    let art_rect = Rect::from_min_size(
        Pos2::new(
            ui.cursor().min.x,
            row_centre(ui) - extent::PLAYBAR_ART / 2.0,
        ),
        Vec2::splat(extent::PLAYBAR_ART),
    );
    art::tile(
        ui.painter(),
        art_rect,
        radius::SM,
        tokens.palette.border_strong,
        cover,
        tokens,
    );

    let text_left = art_rect.right() + space::S3;
    let controls = run_width(now_controls(form));
    let text_right = (ui.max_rect().right() - controls - space::S3).max(text_left);

    let title = transport
        .now_playing
        .as_ref()
        .map_or("Nothing playing", |now| now.title.as_str());
    let title_line = recipe::line(
        ui,
        title,
        egui::FontId::new(text::SM, fonts::sans(Weight::Medium)),
        tokens.palette.text,
        0.0,
        (text_right - text_left).max(0.0),
    );
    // The title and the state word are one block, centred on the art like the
    // reference's flex column: stacking them from fixed offsets puts the pair
    // off-centre whenever either line's height is not the one assumed.
    let label = transport.state.label();
    let state_line = (!label.is_empty()).then(|| {
        recipe::spaced(
            ui,
            label,
            FontId::new(text::XXS, fonts::mono(MonoWeight::Regular)),
            tokens.palette.text_subtle,
            0.0,
        )
    });
    let block = title_line.size().y + state_line.as_ref().map_or(0.0, |line| line.size().y);
    let mut y = art_rect.center().y - block / 2.0;
    let title_height = title_line.size().y;
    ui.painter()
        .galley(Pos2::new(text_left, y), title_line, tokens.palette.text);
    y += title_height;
    if let Some(state_line) = state_line {
        ui.painter().galley(
            Pos2::new(text_left, y),
            state_line,
            tokens.palette.text_subtle,
        );
    }

    let mut x = ui.max_rect().right() - controls;
    draw_run(
        ui,
        tokens,
        now_controls(form),
        None,
        &mut x,
        art_rect.center().y,
        &|_| true,
    );
}

/// The transport row and the progress row beneath it. The command is applied
/// after the whole column is drawn, so a transport move cannot repaint the row
/// it came from in the same frame.
fn centre_column(
    ui: &mut Ui,
    transport: &mut Transport,
    queue: &mut Queue,
    position_ms: u32,
    form: Playbar,
    claimed: Option<Control>,
    tokens: &Tokens,
) -> Option<Command> {
    let stack = extent::BUTTON_LG + space::S1 + extent::SLIDER_H;
    let top = row_centre(ui) - stack / 2.0;
    let up = up_controls(form);
    let width = 2.0 * run_width(&DOWN) + extent::BUTTON_LG;
    let mut x = ui.max_rect().center().x - width / 2.0;

    let mut clicked = draw_run(
        ui,
        tokens,
        &DOWN,
        claimed,
        &mut x,
        top + extent::BUTTON / 2.0,
        &|control| control != Control::Previous || queue.can_previous(),
    );

    let play = Rect::from_min_size(Pos2::new(x, top), Vec2::splat(extent::BUTTON_LG));
    let play_slot = controls::round_button(
        ui,
        tokens,
        Control::PlayPause,
        play,
        play_glyph(transport.state),
        Size::MD,
        "Play",
        queue.can_toggle(),
        false,
    );
    if play_slot.response.clicked() {
        clicked = Some(Control::PlayPause);
    }
    x = play.right() + space::S1;

    if let Some(control) = draw_run(
        ui,
        tokens,
        up,
        claimed,
        &mut x,
        top + extent::BUTTON / 2.0,
        &|control| control != Control::Next || queue.can_next(),
    ) {
        clicked = Some(control);
    }

    // Elapsed, the seek track, and the time left.
    let centre = ui.max_rect().center().x;
    let room = extent::PROGRESS_MAX_W + 2.0 * (extent::TIME_W + space::S3);
    let row = Rect::from_min_size(
        Pos2::new(centre - room / 2.0, top + extent::BUTTON_LG + space::S1),
        Vec2::new(room, extent::SLIDER_H),
    );

    let elapsed = Rect::from_min_size(
        Pos2::new(row.left(), row.center().y - text::XXS),
        Vec2::new(extent::TIME_W, text::XXS * 2.0),
    );
    ui.put(
        elapsed,
        egui::Label::new(recipe::readout(&format_position(position_ms), tokens)),
    );

    let track = Rect::from_min_size(
        Pos2::new(
            elapsed.right() + space::S3,
            row.center().y - extent::SLIDER_H / 2.0,
        ),
        Vec2::new(
            (room - 2.0 * (extent::TIME_W + space::S3)).max(0.0),
            extent::SLIDER_H,
        ),
    );
    let seek = controls::slider(
        ui,
        tokens,
        Control::Seek,
        track,
        seek_fraction(transport, position_ms),
        "Seek",
        transport.seekable(),
    );
    if transport.state == TransportState::Loading {
        let span = Rect::from_center_size(
            track.center(),
            Vec2::new(track.width(), extent::SLIDER_TRACK_H),
        );
        skeleton::band(ui, span, tokens);
    }

    let remaining = match transport.duration_ms {
        Some(duration) => format_remaining(position_ms, duration),
        None => "–:––".to_owned(),
    };
    let readout = recipe::spaced(
        ui,
        &remaining,
        FontId::new(text::XXS, fonts::mono(MonoWeight::Regular)),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XXS, tracking::SLIGHT),
    );
    ui.painter().galley(
        Pos2::new(track.right() + space::S3, elapsed.top()),
        readout,
        tokens.palette.text_subtle,
    );

    let command = seek_command(ui, transport, &seek, track);
    command.or(match clicked {
        Some(Control::Previous) => queue.previous(position_ms, transport),
        Some(Control::Next) => queue.next(transport),
        Some(Control::PlayPause) => queue.toggle(transport),
        _ => None,
    })
}

/// The seek a click or a key on the focused track asks for.
fn seek_command(
    ui: &Ui,
    transport: &mut Transport,
    slot: &controls::Slot,
    track: Rect,
) -> Option<Command> {
    // A drag follows the pointer and asks once, on release; the bar is drawn from
    // the transport's own position, so the preview comes for free.
    if slot.response.dragged() {
        if let Some(pointer) = slot.response.interact_pointer_pos() {
            let fraction = controls::fraction_at(pointer.x, track.left(), track.width());
            transport.scrub_to(fraction);
        }
        return None;
    }
    if slot.response.drag_stopped() {
        return transport.scrub_end();
    }
    if slot.response.clicked() {
        let pointer = slot.response.interact_pointer_pos()?;
        let fraction = controls::fraction_at(pointer.x, track.left(), track.width());
        return transport.seek_to_fraction(fraction);
    }
    if !slot.response.has_focus() {
        return None;
    }
    // Plain arrows would otherwise move focus to the next control, and the seek
    // would step and lose the keyboard at once.
    ui.memory_mut(|memory| {
        memory.set_focus_lock_filter(
            slot.response.id,
            EventFilter {
                horizontal_arrows: true,
                ..Default::default()
            },
        );
    });

    let mut command = None;
    ui.input(|input| {
        if input.key_pressed(Key::Home) {
            command = transport.seek_to_fraction(0.0);
        } else if input.key_pressed(Key::End) {
            command = transport.seek_to_fraction(1.0);
        } else {
            let step = i64::from(SEEK_STEP_MS) * if input.modifiers.shift { 4 } else { 1 };
            if input.key_pressed(Key::ArrowLeft) {
                command = transport.seek_by(-step);
            } else if input.key_pressed(Key::ArrowRight) {
                command = transport.seek_by(step);
            }
        }
    });
    command
}

/// The buttons the reference sheds first as the window narrows, and the volume.
fn extras_column(ui: &mut Ui, tokens: &Tokens, form: Playbar, claimed: Option<Control>) {
    let controls = extras_controls(form);
    let width = run_width(controls);
    let mut x = ui.max_rect().right() - width;
    draw_run(
        ui,
        tokens,
        controls,
        claimed,
        &mut x,
        row_centre(ui),
        &|_| true,
    );
}

/// A `BUTTON` square whose left edge is `x` and whose centre is `centre_y`.
fn square(x: f32, centre_y: f32) -> Rect {
    Rect::from_min_size(
        Pos2::new(x, centre_y - extent::BUTTON / 2.0),
        Vec2::splat(extent::BUTTON),
    )
}

/// Where the queue button is drawn inside the bar `rect`: the shell's drop area
/// is built around it, so it is computed from the same tables the draw walks.
pub fn queue_button(rect: Rect, form: Playbar) -> Rect {
    debug_assert_eq!(
        EXTRAS_FULL.first().map(|(control, _)| *control),
        Some(Control::Queue),
        "the full bar's queue button is the first of its extras"
    );
    debug_assert_eq!(
        UP_REDUCED.last().map(|(control, _)| *control),
        Some(Control::Queue),
        "the reduced bar's queue button is the last of the transport"
    );

    let [_, centre, extras] = layout::playbar_for(rect, form).columns(rect);
    match form {
        Playbar::Full => square(
            extras.right() - run_width(extras_controls(form)),
            extras.center().y,
        ),
        Playbar::Reduced => {
            let stack = extent::BUTTON_LG + space::S1 + extent::SLIDER_H;
            let y = centre.center().y - stack / 2.0 + extent::BUTTON / 2.0;
            let start = centre.center().x - (2.0 * run_width(&DOWN) + extent::BUTTON_LG) / 2.0;
            let up = start + run_width(&DOWN) + extent::BUTTON_LG + space::S1;
            square(up + run_width(up_controls(form)) - extent::BUTTON, y)
        }
    }
}

/// Draws a run left to right, advancing `x` past it, and returns the one
/// control that was clicked.
fn draw_run(
    ui: &mut Ui,
    tokens: &Tokens,
    controls: &[(Control, Icon)],
    claimed: Option<Control>,
    x: &mut f32,
    centre_y: f32,
    can_act: &dyn Fn(Control) -> bool,
) -> Option<Control> {
    let mut clicked = None;
    for (index, (control, glyph)) in controls.iter().enumerate() {
        if index > 0 {
            *x += space::S1;
        }
        if *control == VOLUME {
            *x += space::S2;
            let rect = Rect::from_min_size(
                Pos2::new(*x, centre_y - extent::SLIDER_H / 2.0),
                Vec2::new(extent::VOLUME_W, extent::SLIDER_H),
            );
            let slot = controls::slider(ui, tokens, *control, rect, Some(0.7), "Volume", true);
            if slot.response.clicked() {
                clicked = Some(*control);
            }
            *x += extent::VOLUME_W + space::S2;
        } else {
            let rect = Rect::from_min_size(
                Pos2::new(*x, centre_y - extent::BUTTON / 2.0),
                Vec2::splat(extent::BUTTON),
            );
            let slot = if claimed == Some(*control) {
                // A claimed control is inert, so the moment is not consulted.
                controls::icon_button_claimed(ui, tokens, *control, rect, *glyph, Size::MD, "")
            } else {
                let live = can_act(*control);
                controls::icon_button(ui, tokens, *control, rect, *glyph, Size::MD, "", live)
            };
            if slot.response.clicked() {
                clicked = Some(*control);
            }
            *x += extent::BUTTON;
        }
    }
    clicked
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use egui::{Context, Event, Modifiers, RawInput};

    use super::*;
    use crate::theme::style;

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 200.0))),
            events,
            ..Default::default()
        }
    }

    /// Runs one pass and consumes the font-atlas delta it may have produced.
    fn run(ctx: &Context, input: RawInput, draw: impl FnMut(&mut Ui)) {
        let mut output = ctx.run_ui(input, draw);
        output.textures_delta.clear();
    }

    use crate::view::test_support::{button, click_events};

    fn key(key: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn context(tokens: &Tokens) -> Context {
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(tokens));
        ctx
    }

    fn playing(position_ms: u32) -> Transport {
        Transport {
            state: TransportState::Playing,
            position_ms,
            duration_ms: Some(222_000),
            ..Transport::default()
        }
    }

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
        let command = seek_command(ui, transport, &seek, track);
        (command, seek.response.has_focus())
    }

    fn click(
        ctx: &Context,
        at: Pos2,
        transport: &mut Transport,
        tokens: &Tokens,
    ) -> Option<Command> {
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
        let at =
            |fraction: f32| Pos2::new(track.left() + fraction * track.width(), track.center().y);
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

    /// The title and the state word are one block, centred on the art: stacking
    /// them from fixed offsets left the pair sitting low.
    #[test]
    fn the_now_text_is_centred_on_the_art() {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let bar = Rect::from_min_size(Pos2::new(0.0, 100.0), Vec2::new(1200.0, extent::PLAYBAR_H));
        let mut transport = playing(0);
        transport.now_playing = Some(crate::player::NowPlaying {
            uri: "spotify:track:signal-decay".to_owned(),
            title: "Signal Decay".to_owned(),
            artists: "Nils Aker".to_owned(),
            cover: None,
        });

        let mut output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 300.0))),
                ..Default::default()
            },
            |ui| {
                let mut queue = Queue::default();
                let _ = show(
                    ui,
                    &mut transport,
                    &mut queue,
                    0,
                    Playbar::Full,
                    false,
                    bar,
                    Cover::Missing,
                    &tokens,
                );
            },
        );
        output.textures_delta.clear();

        let painted = |label: &str| {
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text() == label => {
                        Some(Rect::from_min_size(text.pos, text.galley.size()))
                    }
                    _ => None,
                })
        };
        let title = painted("Signal Decay").expect("the bar paints the title");
        let state = painted(transport.state.label()).expect("the bar paints the state");
        let block = title.union(state);
        let centre = bar.center().y;
        assert!(
            (block.center().y - centre).abs() < 0.01,
            "the title and state block centres at {} rather than {centre}",
            block.center().y
        );
    }

    /// Every column centres on the bar, which is what `align-items: center` does
    /// in the reference and what a row's cursor does not do on its own.
    #[test]
    fn every_column_sits_on_the_bars_centre() {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let bar = Rect::from_min_size(Pos2::new(0.0, 100.0), Vec2::new(1200.0, extent::PLAYBAR_H));
        let mut transport = playing(0);

        let mut output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 300.0))),
                ..Default::default()
            },
            |ui| {
                let mut queue = Queue::default();
                let _ = show(
                    ui,
                    &mut transport,
                    &mut queue,
                    0,
                    Playbar::Full,
                    false,
                    bar,
                    Cover::Missing,
                    &tokens,
                );
            },
        );
        output.textures_delta.clear();

        let centre = bar.center().y;
        let boxes = |height: f32| {
            output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(shape) if (shape.rect.height() - height).abs() < 0.01 => {
                        Some(shape.rect)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };

        // The art is the now column's tallest box, so it holds the column's
        // height: its centre is the bar's.
        let art = boxes(extent::PLAYBAR_ART);
        assert!(!art.is_empty(), "the now column draws its art");
        assert!(
            (art[0].center().y - centre).abs() < 0.01,
            "the art at {:?} is not on the bar's centre {centre}",
            art[0].center()
        );

        // The transport and the seek row are one stack, centred together, so the
        // play button's top is half the stack above the bar's centre.
        let stack = extent::BUTTON_LG + space::S1 + extent::SLIDER_H;
        let play = boxes(extent::BUTTON_LG);
        assert!(!play.is_empty(), "the centre column draws the play button");
        assert!(
            (play[0].top() - (centre - stack / 2.0)).abs() < 0.01,
            "the transport stack starts at {} rather than {}",
            play[0].top(),
            centre - stack / 2.0
        );

        // The extras' volume is the one box there whose width is its own token,
        // so it identifies the run without guessing a coordinate.
        let volume = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if (shape.rect.width() - extent::VOLUME_W).abs() < 0.01 =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .next()
            .expect("the extras draw the volume");
        assert!(
            (volume.center().y - centre).abs() < 0.01,
            "the volume at {:?} is not on the bar's centre {centre}",
            volume.center()
        );
    }

    #[test]
    fn the_drawn_extras_fill_the_width_the_layout_reserves() {
        for form in [Playbar::Full, Playbar::Reduced] {
            assert!(
                (run_width(extras_controls(form)) - layout::playbar_extras(form)).abs() < 1e-3,
                "{form:?}: the view draws {} but the layout reserves {}",
                run_width(extras_controls(form)),
                layout::playbar_extras(form)
            );
        }
    }

    #[test]
    fn the_reduced_bar_sheds_the_two_buttons_the_reference_sheds() {
        let full: Vec<Control> = extras_controls(Playbar::Full)
            .iter()
            .map(|(control, _)| *control)
            .collect();
        let reduced: Vec<Control> = extras_controls(Playbar::Reduced)
            .iter()
            .map(|(control, _)| *control)
            .collect();
        assert!(!reduced.contains(&Control::Queue));
        assert!(!reduced.contains(&Control::Devices));
        assert!(full.contains(&Control::Queue));
        assert!(full.contains(&Control::Devices));
        // Shed from the extras, carried by the transport.
        assert!(up_controls(Playbar::Reduced).contains(&(Control::Queue, Icon::Queue)));
    }

    #[test]
    fn the_queue_control_is_in_exactly_one_run_in_each_form() {
        for form in [Playbar::Full, Playbar::Reduced] {
            let runs = [
                now_controls(form),
                DOWN.as_slice(),
                up_controls(form),
                extras_controls(form),
            ];
            let count = runs
                .iter()
                .flat_map(|run| run.iter())
                .filter(|(control, _)| *control == Control::Queue)
                .count();
            assert_eq!(count, 1, "{form:?}: the queue must be drawn once");
        }
    }

    #[test]
    fn the_reduced_transport_ends_with_the_queue_and_the_full_one_does_not() {
        assert_eq!(
            up_controls(Playbar::Reduced)
                .last()
                .map(|(control, _)| *control),
            Some(Control::Queue)
        );
        assert!(
            !up_controls(Playbar::Full)
                .iter()
                .any(|(control, _)| *control == Control::Queue)
        );
    }

    #[test]
    fn the_queue_button_and_its_drop_area_fit_at_every_supported_width() {
        for width in 640..=2000 {
            let width = width as f32;
            let rect =
                Rect::from_min_size(Pos2::new(0.0, 100.0), Vec2::new(width, extent::PLAYBAR_H));
            let form = layout::regions_for(width).playbar;
            let [_, centre, extras] = layout::playbar_for(rect, form).columns(rect);
            let column = match form {
                Playbar::Full => extras,
                Playbar::Reduced => centre,
            };

            let button = queue_button(rect, form);
            assert_eq!(button.size(), Vec2::splat(extent::BUTTON), "{width}");
            assert!(
                column.contains(button.min) && column.contains(button.max),
                "{width}: {form:?} draws the queue button at {button:?} outside {column:?}"
            );

            let area = crate::landing::Area::around(button).hit;
            // The reduced transport sits 12pt above the centre of a 64pt bar and
            // the 44pt area reaches 22pt above a 28pt button, so its top laps the
            // bar's edge by exactly 2pt. That is still the panel above it.
            assert!(
                area.left() >= rect.left()
                    && area.right() <= rect.right()
                    && area.top() >= rect.top() - 2.0
                    && area.bottom() <= rect.bottom(),
                "{width}: {form:?} puts the drop area at {area:?} outside {rect:?}"
            );
        }
    }

    /// The shell builds the queue's drop area around `queue_button`, so that
    /// function has to return the rect the bar actually paints.
    #[test]
    fn the_claimed_queue_button_is_painted_where_queue_button_says() {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let bar = Rect::from_min_size(Pos2::new(0.0, 100.0), Vec2::new(900.0, extent::PLAYBAR_H));
        let mut transport = playing(0);

        let mut output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 300.0))),
                ..Default::default()
            },
            |ui| {
                let mut queue = Queue::default();
                let _ = show(
                    ui,
                    &mut transport,
                    &mut queue,
                    0,
                    Playbar::Reduced,
                    true,
                    bar,
                    Cover::Missing,
                    &tokens,
                );
            },
        );
        output.textures_delta.clear();

        let claimed: Vec<Rect> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape) if shape.fill == tokens.palette.row_selected => {
                    Some(shape.rect)
                }
                _ => None,
            })
            .collect();
        assert_eq!(claimed, vec![queue_button(bar, Playbar::Reduced)]);
    }

    #[test]
    fn the_play_glyph_follows_the_transport() {
        assert_eq!(play_glyph(TransportState::Playing), Icon::Pause);
        for state in [
            TransportState::Idle,
            TransportState::Loading,
            TransportState::Paused,
            TransportState::Stopped,
            TransportState::Unavailable,
        ] {
            assert_eq!(play_glyph(state), Icon::Play, "{state:?}");
        }
    }
}
