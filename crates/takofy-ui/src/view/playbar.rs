//! The playbar: what is playing, the transport, and the extras.
//!
//! Everything it shows comes from the transport — the title, the state, the
//! elapsed time, the glyph — and its controls report clicks rather than change
//! state. The seek track fills from the interpolation and turns a click or an
//! arrow into a command; a track with no known duration draws the track alone
//! and a dash for the time left.

use egui::{Align2, EventFilter, FontId, Key, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2};

use crate::availability::Control;
use crate::icon::Icon;
use crate::layout::{self, Playbar};
use crate::message::Command;
use crate::player::{SEEK_STEP_MS, Transport, TransportState, format_position, format_remaining};
use crate::queue::Queue;
use crate::theme::icon::Size;
use crate::theme::{
    MonoWeight, Tokens, Weight, extent, fonts, radius, space, stroke, text, tracking,
};

use super::controls;
use super::shell::row_centred;
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
pub fn show(
    ui: &mut Ui,
    transport: &mut Transport,
    queue: &mut Queue,
    position_ms: u32,
    form: Playbar,
    rect: Rect,
    tokens: &Tokens,
) -> Option<Command> {
    let cols = layout::playbar_for(rect, form);
    let [now, centre, extras] = cols.columns(rect);

    row_centred(ui, "playbar-now", now, |ui| {
        now_column(ui, transport, tokens, form);
    });
    let command = row_centred(ui, "playbar-centre", centre, |ui| {
        centre_column(ui, transport, queue, position_ms, tokens)
    });
    row_centred(ui, "playbar-extras", extras, |ui| {
        extras_column(ui, tokens, form);
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
fn now_column(ui: &mut Ui, transport: &Transport, tokens: &Tokens, form: Playbar) {
    let left = ui.cursor().min;
    let art = Rect::from_min_size(left, Vec2::splat(extent::PLAYBAR_ART));
    ui.painter().rect(
        art,
        radius::SM,
        tokens.palette.surface_raised,
        Stroke::new(stroke::HAIRLINE, tokens.palette.border_strong),
        StrokeKind::Inside,
    );

    let text_left = art.right() + space::S3;
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
    ui.painter().galley(
        Pos2::new(text_left, art.center().y - text::SM - space::S1),
        title_line,
        tokens.palette.text,
    );
    let label = transport.state.label();
    if !label.is_empty() {
        ui.painter().text(
            Pos2::new(text_left, art.center().y + space::S1),
            Align2::LEFT_TOP,
            label,
            FontId::new(text::XXS, fonts::mono(MonoWeight::Regular)),
            tokens.palette.text_subtle,
        );
    }

    let mut x = ui.max_rect().right() - controls;
    draw_run(
        ui,
        tokens,
        now_controls(form),
        &mut x,
        art.center().y,
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
    tokens: &Tokens,
) -> Option<Command> {
    let top = ui.cursor().min.y;
    let down = [
        (Control::Shuffle, Icon::Shuffle),
        (Control::Previous, Icon::Previous),
    ];
    let up = [(Control::Next, Icon::Next), (Control::Repeat, Icon::Repeat)];
    let width = 2.0 * run_width(&down) + extent::BUTTON_LG;
    let mut x = ui.max_rect().center().x - width / 2.0;

    let mut clicked = draw_run(
        ui,
        tokens,
        &down,
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
    );
    if play_slot.response.clicked() {
        clicked = Some(Control::PlayPause);
    }
    x = play.right() + space::S1;

    if let Some(control) = draw_run(
        ui,
        tokens,
        &up,
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
        transport.fraction(position_ms),
        "Seek",
        transport.seekable(),
    );

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
fn extras_column(ui: &mut Ui, tokens: &Tokens, form: Playbar) {
    let controls = extras_controls(form);
    let width = run_width(controls);
    let mut x = ui.max_rect().right() - width;
    draw_run(
        ui,
        tokens,
        controls,
        &mut x,
        ui.cursor().min.y + extent::PLAYBAR_ART / 2.0,
        &|_| true,
    );
}

/// Draws a run left to right, advancing `x` past it, and returns the one
/// control that was clicked.
fn draw_run(
    ui: &mut Ui,
    tokens: &Tokens,
    controls: &[(Control, Icon)],
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
            let slot = controls::icon_button(
                ui,
                tokens,
                *control,
                rect,
                *glyph,
                Size::MD,
                "",
                can_act(*control),
            );
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

    use egui::{Context, Event, Modifiers, PointerButton, RawInput};

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

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

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
        for event in [Event::PointerMoved(at), button(at, true), button(at, false)] {
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
