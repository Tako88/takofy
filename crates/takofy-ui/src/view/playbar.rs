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
use crate::layout::{self, PaneButton, Playbar};
use crate::message::Command;
use crate::player::{SEEK_STEP_MS, Transport, TransportState};
use crate::queue::Queue;
use crate::shuffle;
use crate::theme::icon::Size;
use crate::theme::{MonoWeight, Tokens, Weight, extent, fonts, radius, space, text, tracking};
use crate::volume::{self, Volume};

use super::art;
use super::carry;
use super::context_menu;
use super::controls;
use super::heart;
use super::shell::{row_centre, row_centred};
use super::skeleton;
use super::text as recipe;

/// The one control in each column that is not an icon button.
const VOLUME: Control = Control::Volume;

/// One control in a run: what it is, what it draws, and what it is called.
///
/// The element carries its own glyph, label and badge because the run is not
/// always static: the playbar's heart is filled and named by the track's state,
/// and shuffle and repeat are named by the queue's, so a `(Control, Icon)` pair
/// has nowhere to put either.
#[derive(Clone, Copy)]
struct Run {
    control: Control,
    icon: Icon,
    label: &'static str,
    /// Whether the glyph is the control's own state, painted in the accent.
    lit: bool,
    /// A mark at the glyph's lower right, which is how repeat-one reads.
    badge: Option<&'static str>,
}

impl Run {
    /// A control that draws the same glyph whatever the frame says.
    const fn plain(control: Control, icon: Icon) -> Self {
        Self {
            control,
            icon,
            label: "",
            lit: false,
            badge: None,
        }
    }
}

/// The controls each column holds, with the glyph each draws.
///
/// The layout reserves the extras' width from the same shape, and a test here
/// pins the two together so a control added to one cannot drift from the other.
const NOW_FULL: [Run; 2] = [
    Run::plain(Control::Like, Icon::Heart),
    Run::plain(Control::Miniplayer, Icon::Screen),
];
const NOW_REDUCED: [Run; 1] = [Run::plain(Control::Like, Icon::Heart)];
const EXTRAS_FULL: [Run; 5] = [
    Run::plain(Control::Queue, Icon::Queue),
    Run::plain(Control::Devices, Icon::Devices),
    Run::plain(Control::Mute, Icon::VolumeMute),
    Run::plain(VOLUME, Icon::Volume),
    Run::plain(Control::Fullscreen, Icon::Expand),
];
const EXTRAS_REDUCED: [Run; 3] = [
    Run::plain(Control::Mute, Icon::VolumeMute),
    Run::plain(VOLUME, Icon::Volume),
    Run::plain(Control::Fullscreen, Icon::Expand),
];

const DOWN: [Run; 2] = [
    Run::plain(Control::Shuffle, Icon::Shuffle),
    Run::plain(Control::Previous, Icon::Previous),
];
const UP_FULL: [Run; 2] = [
    Run::plain(Control::Next, Icon::Next),
    Run::plain(Control::Repeat, Icon::Repeat),
];
/// Below 1000px the extras shed the queue, so it joins the transport after
/// repeat. Play stays centred: the row centres on `DOWN`.
const UP_REDUCED: [Run; 3] = [
    Run::plain(Control::Next, Icon::Next),
    Run::plain(Control::Repeat, Icon::Repeat),
    Run::plain(Control::Queue, Icon::Queue),
];

fn up_controls(form: Playbar) -> &'static [Run] {
    match form {
        Playbar::Full => &UP_FULL,
        Playbar::Reduced => &UP_REDUCED,
    }
}

/// The transport's left-hand run with shuffle's state.
fn down_runs(queue: &Queue) -> Vec<Run> {
    let mut runs = DOWN.to_vec();
    let on = queue.shuffle();
    if let Some(first) = runs.first_mut() {
        *first = Run {
            control: Control::Shuffle,
            icon: Icon::Shuffle,
            label: shuffle::label(on),
            lit: on,
            badge: None,
        };
    }
    runs
}

/// The transport's right-hand run with repeat's and the queue's own state.
fn up_runs(form: Playbar, queue: &Queue, bar: Bar) -> Vec<Run> {
    let repeat = queue.repeat();
    let mut runs = up_controls(form).to_vec();
    for run in &mut runs {
        match run.control {
            Control::Repeat => {
                *run = Run {
                    control: Control::Repeat,
                    icon: Icon::Repeat,
                    label: repeat.label(),
                    lit: repeat.lit(),
                    badge: repeat.badge(),
                };
            }
            Control::Queue => *run = pane_run(bar.pane),
            _ => {}
        }
    }
    runs
}

/// The queue button's run: its glyph says whether the pane is on screen.
fn pane_run(pane: PaneButton) -> Run {
    Run {
        control: Control::Queue,
        icon: Icon::Queue,
        label: match pane {
            PaneButton::Unavailable => "Queue",
            PaneButton::Shown => "Queue shown",
            PaneButton::Hidden => "Queue hidden",
        },
        lit: pane == PaneButton::Shown,
        badge: None,
    }
}

/// Whether the queue button can toggle the pane this frame.
fn pane_live(pane: PaneButton, carrying: bool) -> bool {
    pane != PaneButton::Unavailable && !carrying
}

/// The fullscreen button's run: its glyph says whether the window is fullscreen.
///
/// The sprite has no collapse symbol, so the state is the tone and the label.
fn fullscreen_run(fullscreen: Option<bool>) -> Run {
    Run {
        control: Control::Fullscreen,
        icon: Icon::Expand,
        label: match fullscreen {
            Some(true) => "Full screen on",
            Some(false) => "Full screen off",
            None => "Full screen",
        },
        lit: fullscreen == Some(true),
        badge: None,
    }
}

/// Whether a transport control can act this frame.
///
/// The modes are not transport actions: they describe what the queue will do
/// when it plays, so an empty queue can still be told how to start. Shuffle is
/// inert only while a block is carried: a drop is resolved against the rows it
/// was picked from, and drawing the list again under it would refuse the user's
/// drag silently.
fn transport_live(control: Control, queue: &Queue, carrying: bool) -> bool {
    match control {
        Control::Previous => queue.can_previous(),
        Control::Next => queue.can_next(),
        Control::Shuffle => !carrying,
        Control::Repeat => true,
        _ => true,
    }
}

fn now_controls(form: Playbar) -> &'static [Run] {
    match form {
        Playbar::Full => &NOW_FULL,
        Playbar::Reduced => &NOW_REDUCED,
    }
}

/// The now column's runs with the heart the track's state asks for.
///
/// The table is the width's, because a filled heart is the same size; only the
/// glyph, the label and the tone change.
fn now_runs(form: Playbar, liked: Option<bool>) -> Vec<Run> {
    let liked_now = liked == Some(true);
    let heart = Run {
        control: Control::Like,
        icon: if liked_now {
            Icon::HeartFilled
        } else {
            Icon::Heart
        },
        label: crate::verbs::heart_label(liked_now),
        lit: liked_now,
        badge: None,
    };
    let mut runs = now_controls(form).to_vec();
    if let Some(first) = runs.first_mut() {
        *first = heart;
    }
    runs
}

fn extras_controls(form: Playbar) -> &'static [Run] {
    match form {
        Playbar::Full => &EXTRAS_FULL,
        Playbar::Reduced => &EXTRAS_REDUCED,
    }
}

/// The extras' runs with the queue's and the window's own state.
fn extras_runs(form: Playbar, bar: Bar, volume: &Volume) -> Vec<Run> {
    let mut runs = extras_controls(form).to_vec();
    for run in &mut runs {
        match run.control {
            Control::Queue => *run = pane_run(bar.pane),
            Control::Fullscreen => *run = fullscreen_run(bar.fullscreen),
            Control::Mute => *run = mute_run(volume),
            _ => {}
        }
    }
    runs
}

/// The speaker: its label says which way it goes and it lights while silenced.
fn mute_run(volume: &Volume) -> Run {
    Run {
        control: Control::Mute,
        icon: Icon::VolumeMute,
        label: if volume.muted() { "Unmute" } else { "Mute" },
        lit: volume.muted(),
        badge: None,
    }
}

/// What the bar needs from the frame beyond its own data.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bar {
    /// Whether the queue's own button is the drop target this frame.
    pub queue_claimed: bool,
    /// The playing track's liked state, or `None` while the set cannot say.
    pub like: Option<bool>,
    /// What the queue button can do about the pane at this width.
    pub pane: PaneButton,
    /// Whether the window reports itself fullscreen, or `None` while unknown.
    pub fullscreen: Option<bool>,
}

impl Default for Bar {
    /// A bar told nothing draws both switches inert: an unknown pane and an
    /// unknown window are not the user's to act on.
    fn default() -> Self {
        Self {
            queue_claimed: false,
            like: None,
            pane: PaneButton::Unavailable,
            fullscreen: None,
        }
    }
}

/// What the bar's controls asked for this frame.
///
/// A transport move is a command the shell hands on; the pane and the window
/// are the shell's own to change, so their presses are reported as presses.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Reply {
    pub command: Option<Command>,
    /// Whether the queue button was pressed.
    pub pane: bool,
    /// Whether the fullscreen button was pressed: the window is the shell's to ask.
    pub fullscreen: bool,
}

/// One column's press: a command to apply, and the control whose own state the
/// shell owns.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct Pressed {
    command: Option<Command>,
    control: Option<Control>,
}

/// Draws the bar's three columns inside `rect` and reports what a control in it
/// asked for.
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
    bar: Bar,
    rect: Rect,
    cover: Cover,
    tokens: &Tokens,
) -> Reply {
    let cols = layout::playbar_for(rect, form);
    let [now, centre, extras] = cols.columns(rect);

    let like = row_centred(ui, "playbar-now", now, |ui| {
        now_column(ui, transport, cover, tokens, form, bar.like)
    });
    if let Some(liked) = like
        && let Some(now) = transport.now_playing.as_ref()
    {
        heart::post(
            ui.ctx(),
            heart::Ask {
                tracks: vec![now.uri.clone()],
                liked,
            },
        );
    }
    let pressed = row_centred(ui, "playbar-centre", centre, |ui| {
        centre_column(ui, transport, queue, position_ms, form, bar, tokens)
    });
    let extras_press = row_centred(ui, "playbar-extras", extras, |ui| {
        extras_column(ui, tokens, form, bar, &mut transport.volume)
    });
    // The reduced form draws the queue in the transport, so the two columns are
    // one channel: a press is whichever of them reported it.
    let control = extras_press.or(pressed.control);
    // Mute is a lid on the level rather than a level of its own, so it asks for
    // no command of its own. The level reaches the player from its own drain
    // after the draw, because the shell keeps one command per frame and a level
    // dropped there would leave the bar showing what the player never got.
    if control == Some(Control::Mute) {
        transport.volume.toggle_mute();
    }
    Reply {
        command: pressed.command,
        pane: control == Some(Control::Queue),
        fullscreen: control == Some(Control::Fullscreen),
    }
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
fn run_width(controls: &[Run]) -> f32 {
    let mut width = 0.0;
    for (index, run) in controls.iter().enumerate() {
        if index > 0 {
            width += space::S1;
        }
        width += if run.control == VOLUME {
            extent::VOLUME_W + 2.0 * space::S2
        } else {
            extent::BUTTON
        };
    }
    width
}

/// The art tile, the title, the state, and the buttons beside them.
fn now_column(
    ui: &mut Ui,
    transport: &Transport,
    cover: Cover,
    tokens: &Tokens,
    form: Playbar,
    liked: Option<bool>,
) -> Option<bool> {
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
    let clicked = draw_run(
        ui,
        Pen {
            tokens,
            claimed: None,
            volume: None,
            // A heart with no state to act on is inert, as a row's is.
            can_act: &|control| control != Control::Like || liked.is_some(),
        },
        &now_runs(form, liked),
        &mut x,
        art_rect.center().y,
    );
    let like = clicked.filter(|control| *control == Control::Like);
    // egui reports a focused button's Enter or Space as a click, repeats too, so
    // only a fresh press is the user's, as on a row.
    let pointer = ui.input(|input| input.pointer.button_clicked(egui::PointerButton::Primary));
    let held = like.is_some() && !pointer && context_menu::held_repeat(ui);
    like.filter(|_| !held).map(|_| liked != Some(true))
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
    bar: Bar,
    tokens: &Tokens,
) -> Pressed {
    let stack = extent::BUTTON_LG + space::S1 + extent::SLIDER_H;
    let top = row_centre(ui) - stack / 2.0;
    let carrying = carry::carrying(ui.ctx());
    let claimed = bar.queue_claimed.then_some(Control::Queue);
    let live = |control: Control| match control {
        Control::Queue => pane_live(bar.pane, carrying),
        other => transport_live(other, queue, carrying),
    };
    let width = 2.0 * run_width(&DOWN) + extent::BUTTON_LG;
    let mut x = ui.max_rect().center().x - width / 2.0;

    let mut clicked = draw_run(
        ui,
        Pen {
            tokens,
            claimed,
            // The centre's runs are buttons: the slider is the extras' alone.
            volume: None,
            can_act: &live,
        },
        &down_runs(queue),
        &mut x,
        top + extent::BUTTON / 2.0,
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
        Pen {
            tokens,
            claimed,
            volume: None,
            can_act: &live,
        },
        &up_runs(form, queue, bar),
        &mut x,
        top + extent::BUTTON / 2.0,
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
    // egui reports a focused button's Enter or Space as a click and counts the
    // repeats too, so a held key must not step a mode or a toggle again. The
    // press is dropped here, before anything reads it.
    let pointer = ui.input(|input| input.pointer.button_clicked(egui::PointerButton::Primary));
    let held = matches!(
        clicked,
        Some(Control::Shuffle | Control::Repeat | Control::Queue)
    ) && !pointer
        && context_menu::held_repeat(ui);
    let clicked = clicked.filter(|_| !held);
    Pressed {
        command: command.or(match clicked {
            Some(Control::Previous) => queue.previous(position_ms, transport),
            Some(Control::Next) => queue.next(transport),
            Some(Control::PlayPause) => queue.toggle(transport),
            Some(Control::Shuffle) => {
                queue.toggle_shuffle();
                None
            }
            Some(Control::Repeat) => {
                queue.cycle_repeat();
                None
            }
            _ => None,
        }),
        control: clicked,
    }
}

/// Follows the pointer across the volume slider, and its arrow keys.
///
/// A drag sounds as it moves, so the level is written on every pass rather than
/// on release; the command that tells the player is taken once per change by
/// `Transport::take_volume`.
fn drag_volume(ui: &Ui, volume: &mut Volume, slot: &controls::Slot, rect: Rect) {
    if slot.response.dragged() || slot.response.drag_stopped() || slot.response.clicked() {
        if let Some(pointer) = slot.response.interact_pointer_pos() {
            volume.set_fraction(controls::fraction_at(pointer.x, rect.left(), rect.width()));
        }
        return;
    }
    if !slot.response.has_focus() {
        return;
    }
    // Plain arrows would otherwise move focus to the next control, so the level
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
    ui.input(|input| {
        let step = i32::from(volume::STEP);
        if input.key_pressed(Key::Home) {
            volume.set_fraction(0.0);
        } else if input.key_pressed(Key::End) {
            volume.set_fraction(1.0);
        } else if input.key_pressed(Key::ArrowLeft) {
            volume.step(-step);
        } else if input.key_pressed(Key::ArrowRight) {
            volume.step(step);
        }
    });
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
fn extras_column(
    ui: &mut Ui,
    tokens: &Tokens,
    form: Playbar,
    bar: Bar,
    volume: &mut Volume,
) -> Option<Control> {
    let runs = extras_runs(form, bar, volume);
    let width = run_width(&runs);
    let mut x = ui.max_rect().right() - width;
    let carrying = carry::carrying(ui.ctx());
    let clicked = draw_run(
        ui,
        Pen {
            tokens,
            claimed: bar.queue_claimed.then_some(Control::Queue),
            volume: Some(volume),
            can_act: &|control| match control {
                Control::Queue => pane_live(bar.pane, carrying),
                Control::Fullscreen => bar.fullscreen.is_some(),
                _ => true,
            },
        },
        &runs,
        &mut x,
        row_centre(ui),
    );
    let pointer = ui.input(|input| input.pointer.button_clicked(egui::PointerButton::Primary));
    let held = clicked.is_some() && !pointer && context_menu::held_repeat(ui);
    clicked.filter(|_| !held)
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
        EXTRAS_FULL.first().map(|run| run.control),
        Some(Control::Queue),
        "the full bar's queue button is the first of its extras"
    );
    debug_assert_eq!(
        UP_REDUCED.last().map(|run| run.control),
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

/// A state glyph's badge, at its lower right.
fn paint_badge(ui: &Ui, tokens: &Tokens, rect: Rect, badge: &str) {
    let galley = recipe::spaced(
        ui,
        badge,
        FontId::new(text::XXS, fonts::mono(MonoWeight::Regular)),
        tokens.palette.text,
        0.0,
    );
    let glyph = Rect::from_center_size(rect.center(), Vec2::splat(Size::MD.side));
    let at = Pos2::new(
        glyph.right() - galley.size().x,
        glyph.bottom() - galley.size().y,
    );
    ui.painter().galley(at, galley, tokens.palette.text);
}

/// What a run needs from the frame beyond the rows themselves.
struct Pen<'a> {
    tokens: &'a Tokens,
    /// The control that is a drop target this frame, drawn inert.
    claimed: Option<Control>,
    /// The level, where the run carries the slider.
    volume: Option<&'a mut Volume>,
    can_act: &'a dyn Fn(Control) -> bool,
}

/// Draws a run left to right, advancing `x` past it, and returns the one
/// control that was clicked.
fn draw_run(
    ui: &mut Ui,
    pen: Pen<'_>,
    controls: &[Run],
    x: &mut f32,
    centre_y: f32,
) -> Option<Control> {
    let Pen {
        tokens,
        claimed,
        mut volume,
        can_act,
    } = pen;
    let mut clicked = None;
    for (index, run) in controls.iter().enumerate() {
        if index > 0 {
            *x += space::S1;
        }
        if run.control == VOLUME {
            *x += space::S2;
            let rect = Rect::from_min_size(
                Pos2::new(*x, centre_y - extent::SLIDER_H / 2.0),
                Vec2::new(extent::VOLUME_W, extent::SLIDER_H),
            );
            // Only the extras carry the slider, and they are the only caller with
            // a level to draw, so a bar without one cannot reach here.
            if let Some(volume) = volume.as_deref_mut() {
                let slot = controls::slider(
                    ui,
                    tokens,
                    run.control,
                    rect,
                    Some(volume.fraction()),
                    "Volume",
                    true,
                );
                drag_volume(ui, volume, &slot, rect);
            }
            *x += extent::VOLUME_W + space::S2;
        } else {
            let rect = Rect::from_min_size(
                Pos2::new(*x, centre_y - extent::BUTTON / 2.0),
                Vec2::splat(extent::BUTTON),
            );
            let slot = if claimed == Some(run.control) {
                // A claimed control is inert, so the moment is not consulted.
                controls::icon_button_claimed(
                    ui,
                    tokens,
                    run.control,
                    rect,
                    run.icon,
                    Size::MD,
                    run.label,
                )
            } else {
                let live = can_act(run.control);
                if run.lit {
                    controls::icon_button_on(
                        ui,
                        tokens,
                        run.control,
                        (),
                        rect,
                        run.icon,
                        Size::MD,
                        run.label,
                        live,
                    )
                } else {
                    controls::icon_button(
                        ui,
                        tokens,
                        run.control,
                        rect,
                        run.icon,
                        Size::MD,
                        run.label,
                        live,
                    )
                }
            };
            if slot.response.clicked() {
                clicked = Some(run.control);
            }
            if let Some(badge) = run.badge {
                paint_badge(ui, tokens, rect, badge);
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
                    Bar::default(),
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
                    Bar::default(),
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
            .map(|run| run.control)
            .collect();
        let reduced: Vec<Control> = extras_controls(Playbar::Reduced)
            .iter()
            .map(|run| run.control)
            .collect();
        assert!(!reduced.contains(&Control::Queue));
        assert!(!reduced.contains(&Control::Devices));
        assert!(full.contains(&Control::Queue));
        assert!(full.contains(&Control::Devices));
        // Shed from the extras, carried by the transport.
        assert!(
            up_controls(Playbar::Reduced)
                .iter()
                .any(|run| run.control == Control::Queue)
        );
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
                .filter(|run| run.control == Control::Queue)
                .count();
            assert_eq!(count, 1, "{form:?}: the queue must be drawn once");
        }
    }

    #[test]
    fn the_reduced_transport_ends_with_the_queue_and_the_full_one_does_not() {
        assert_eq!(
            up_controls(Playbar::Reduced).last().map(|run| run.control),
            Some(Control::Queue)
        );
        assert!(
            !up_controls(Playbar::Full)
                .iter()
                .any(|run| run.control == Control::Queue)
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
                    Bar {
                        queue_claimed: true,
                        ..Bar::default()
                    },
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

    /// The bar rect the heart tests draw into: wide enough for the full form.
    fn heart_bar() -> Rect {
        Rect::from_min_size(Pos2::new(0.0, 100.0), Vec2::new(1200.0, extent::PLAYBAR_H))
    }

    /// One frame of the bar, with the playing track named.
    fn bar_frame(
        ctx: &egui::Context,
        tokens: &Tokens,
        transport: &mut Transport,
        form: Playbar,
        like: Option<bool>,
        events: Vec<Event>,
    ) -> (egui::FullOutput, [Rect; 3]) {
        let bar = heart_bar();
        let cols = layout::playbar_for(bar, form).columns(bar);
        let mut queue = Queue::default();
        let mut output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 300.0))),
                events,
                ..Default::default()
            },
            |ui| {
                let _ = show(
                    ui,
                    transport,
                    &mut queue,
                    0,
                    form,
                    Bar {
                        like,
                        ..Bar::default()
                    },
                    bar,
                    Cover::Missing,
                    tokens,
                );
            },
        );
        output.textures_delta.clear();
        (output, cols)
    }

    /// The accent-filled paths inside `rect`, which is a liked heart and nothing
    /// else in the now column.
    fn accent_paths(output: &egui::FullOutput, tokens: &Tokens, rect: Rect) -> Vec<Rect> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Path(path) if path.fill == tokens.palette.accent => {
                    Some(clipped.shape.visual_bounding_rect())
                }
                _ => None,
            })
            .filter(|found| rect.contains(found.center()))
            .collect()
    }

    fn named_playing() -> Transport {
        let mut transport = playing(0);
        transport.now_playing = Some(crate::player::NowPlaying {
            uri: "spotify:track:x".to_owned(),
            title: "Signal Decay".to_owned(),
            artists: "Nils Aker".to_owned(),
            cover: None,
        });
        transport
    }

    #[test]
    fn the_playbar_heart_reads_the_playing_tracks_state() {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let mut transport = named_playing();

        let (liked, cols) = bar_frame(
            &ctx,
            &tokens,
            &mut transport,
            Playbar::Full,
            Some(true),
            Vec::new(),
        );
        assert!(
            !accent_paths(&liked, &tokens, cols[0]).is_empty(),
            "a liked track's playbar heart is not filled"
        );

        let (unliked, cols) = bar_frame(
            &ctx,
            &tokens,
            &mut transport,
            Playbar::Full,
            Some(false),
            Vec::new(),
        );
        assert!(
            accent_paths(&unliked, &tokens, cols[0]).is_empty(),
            "an unliked track's playbar heart took the accent"
        );
    }

    #[test]
    fn a_click_on_the_playbar_heart_posts_the_opposite_for_the_playing_uri() {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let mut transport = named_playing();

        let (output, cols) = bar_frame(
            &ctx,
            &tokens,
            &mut transport,
            Playbar::Full,
            Some(true),
            Vec::new(),
        );
        let heart = accent_paths(&output, &tokens, cols[0])
            .first()
            .copied()
            .expect("the liked heart is painted");
        let at = heart.center();

        // A warm frame and a move, so the hit test knows the pointer before the
        // press.
        let _ = bar_frame(
            &ctx,
            &tokens,
            &mut transport,
            Playbar::Full,
            Some(true),
            Vec::new(),
        );
        let _ = bar_frame(
            &ctx,
            &tokens,
            &mut transport,
            Playbar::Full,
            Some(true),
            vec![Event::PointerMoved(at)],
        );
        for event in click_events(at) {
            let _ = bar_frame(
                &ctx,
                &tokens,
                &mut transport,
                Playbar::Full,
                Some(true),
                vec![event],
            );
        }

        assert_eq!(
            crate::view::heart::take(&ctx),
            Some(crate::view::heart::Ask {
                tracks: vec!["spotify:track:x".to_owned()],
                liked: false,
            })
        );
    }

    #[test]
    fn a_held_enter_on_the_playbar_heart_toggles_once() {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let mut transport = named_playing();

        // A warm pass registers the controls, then Tab takes the keyboard to
        // the now column's heart, which is the first control drawn.
        let _ = bar_frame(
            &ctx,
            &tokens,
            &mut transport,
            Playbar::Full,
            Some(true),
            Vec::new(),
        );
        let _ = bar_frame(
            &ctx,
            &tokens,
            &mut transport,
            Playbar::Full,
            Some(true),
            vec![key(Key::Tab, Modifiers::NONE)],
        );
        let _ = crate::view::heart::take(&ctx);

        let enter = |repeat| Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat,
            modifiers: Modifiers::NONE,
        };
        // The set flips after the first ask, as the shell would flip it, so a
        // repeat that reached the heart would flip it straight back.
        let mut asks = Vec::new();
        for repeat in [false, true, true] {
            let _ = bar_frame(
                &ctx,
                &tokens,
                &mut transport,
                Playbar::Full,
                Some(!repeat),
                vec![enter(repeat)],
            );
            asks.push(crate::view::heart::take(&ctx));
        }

        assert_eq!(
            asks,
            vec![
                Some(crate::view::heart::Ask {
                    tracks: vec!["spotify:track:x".to_owned()],
                    liked: false,
                }),
                None,
                None,
            ],
            "a held Enter posted more than one ask: {asks:?}"
        );
    }

    #[test]
    fn the_playbar_and_a_row_of_the_same_track_agree() {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let mut likes = crate::likes::Likes::default();
        assert!(likes.request(true));
        likes.apply(&crate::likes::Event::Set(vec![
            "spotify:track:x".to_owned(),
        ]));
        let state = likes.state("spotify:track:x");
        assert_eq!(state, Some(true), "the set did not answer");

        let mut transport = named_playing();
        let (output, cols) = bar_frame(
            &ctx,
            &tokens,
            &mut transport,
            Playbar::Full,
            state,
            Vec::new(),
        );
        assert!(
            !accent_paths(&output, &tokens, cols[0]).is_empty(),
            "the playbar disagreed with the row's own answer"
        );
    }

    #[test]
    fn the_reduced_bar_keeps_the_heart() {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let mut transport = named_playing();
        let (output, cols) = bar_frame(
            &ctx,
            &tokens,
            &mut transport,
            Playbar::Reduced,
            Some(true),
            Vec::new(),
        );
        assert!(
            !accent_paths(&output, &tokens, cols[0]).is_empty(),
            "the reduced bar shed its heart"
        );
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

    fn queue_of(count: usize) -> Queue {
        let tracks: Vec<takofy_model::TrackEntry> = (0..count)
            .map(|index| takofy_model::TrackEntry {
                uri: format!("t{index}"),
                title: format!("t{index}"),
                artists: String::new(),
                album: None,
                album_uri: None,
                artist_uris: Vec::new(),
                added_at: None,
                duration_ms: None,
                named: true,
                cover: None,
            })
            .collect();
        let mut queue = Queue::seeded(5);
        queue.start(&tracks, 0, &mut Transport::default());
        queue
    }

    /// One frame of the bar over `queue`: what it reported and what it painted.
    fn queue_frame(
        ctx: &egui::Context,
        tokens: &Tokens,
        queue: &mut Queue,
        form: Playbar,
        bar: Bar,
        events: Vec<Event>,
    ) -> (Reply, egui::FullOutput) {
        let rect = heart_bar();
        let mut transport = playing(0);
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
                    &mut transport,
                    queue,
                    0,
                    form,
                    bar,
                    rect,
                    Cover::Missing,
                    tokens,
                );
            },
        );
        output.textures_delta.clear();
        (reply, output)
    }

    /// Where one of the transport's buttons is drawn, from the shape the column
    /// lays out by.
    fn transport_slot(centre: Rect, control: Control) -> Rect {
        let stack = extent::BUTTON_LG + space::S1 + extent::SLIDER_H;
        let top = centre.center().y - stack / 2.0;
        let start = centre.center().x - (2.0 * run_width(&DOWN) + extent::BUTTON_LG) / 2.0;
        let up = start + run_width(&DOWN) + extent::BUTTON_LG + space::S1;
        let at = |left: f32, index: usize| {
            Rect::from_min_size(
                Pos2::new(left + index as f32 * (extent::BUTTON + space::S1), top),
                Vec2::splat(extent::BUTTON),
            )
        };
        match control {
            Control::Shuffle => at(start, 0),
            Control::Previous => at(start, 1),
            Control::Next => at(up, 0),
            Control::Repeat => at(up, 1),
            other => panic!("{other:?} is not one of the transport's runs"),
        }
    }

    /// Clicks `control` over the four passes a click takes, and reports what each
    /// one returned.
    fn click_control(
        ctx: &egui::Context,
        tokens: &Tokens,
        queue: &mut Queue,
        form: Playbar,
        control: Control,
    ) -> Vec<Option<Command>> {
        let bar = heart_bar();
        let centre = layout::playbar_for(bar, form).columns(bar)[1];
        let at = transport_slot(centre, control).center();
        let _ = queue_frame(ctx, tokens, queue, form, Bar::default(), Vec::new());
        let mut commands = vec![
            queue_frame(
                ctx,
                tokens,
                queue,
                form,
                Bar::default(),
                vec![Event::PointerMoved(at)],
            )
            .0
            .command,
        ];
        for event in click_events(at) {
            commands.push(
                queue_frame(ctx, tokens, queue, form, Bar::default(), vec![event])
                    .0
                    .command,
            );
        }
        commands
    }

    /// Clicks `at` over the passes egui's hit test needs, and reports every
    /// frame.
    fn click_at(
        ctx: &egui::Context,
        tokens: &Tokens,
        queue: &mut Queue,
        form: Playbar,
        bar: Bar,
        at: Pos2,
    ) -> Vec<Reply> {
        let _ = queue_frame(ctx, tokens, queue, form, bar, Vec::new());
        let mut replies =
            vec![queue_frame(ctx, tokens, queue, form, bar, vec![Event::PointerMoved(at)]).0];
        for event in click_events(at) {
            replies.push(queue_frame(ctx, tokens, queue, form, bar, vec![event]).0);
        }
        replies
    }

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

    fn controls_of(runs: &[Run]) -> Vec<Control> {
        runs.iter().map(|run| run.control).collect()
    }

    /// The colour a painted shape is drawn in, when it paints one.
    fn shape_colour(shape: &egui::Shape) -> Option<egui::Color32> {
        match shape {
            egui::Shape::Path(path) if path.fill != egui::Color32::TRANSPARENT => Some(path.fill),
            egui::Shape::Path(path) => match path.stroke.color {
                egui::epaint::ColorMode::Solid(color) => Some(color),
                egui::epaint::ColorMode::UV(_) => None,
            },
            egui::Shape::Circle(circle) => Some(circle.fill),
            egui::Shape::LineSegment { stroke, .. } => Some(stroke.color),
            _ => None,
        }
    }

    /// Whether anything inside `rect` is painted in the accent.
    fn accent_inside(output: &egui::FullOutput, tokens: &Tokens, rect: Rect) -> bool {
        output.shapes.iter().any(|clipped| {
            rect.contains(clipped.shape.visual_bounding_rect().center())
                && shape_colour(&clipped.shape) == Some(tokens.palette.accent)
        })
    }

    /// The text painted inside `rect`.
    fn text_inside(output: &egui::FullOutput, rect: Rect) -> Vec<String> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if rect.contains(text.pos) => {
                    Some(text.galley.text().to_owned())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_extras_and_up_runs_keep_the_tables_controls_and_widths() {
        let queue = queue_of(4);
        let bars = [
            Bar::default(),
            Bar {
                pane: PaneButton::Shown,
                ..Bar::default()
            },
        ];
        for form in [Playbar::Full, Playbar::Reduced] {
            let down = down_runs(&queue);
            assert_eq!(controls_of(&down), controls_of(&DOWN), "{form:?}");
            assert_eq!(run_width(&down), run_width(&DOWN), "{form:?}");

            for bar in bars {
                let up = up_runs(form, &queue, bar);
                assert_eq!(controls_of(&up), controls_of(up_controls(form)), "{form:?}");
                assert_eq!(
                    run_width(&up),
                    run_width(up_controls(form)),
                    "{form:?}: the layout reserves what the drawn runs take"
                );

                let extras = extras_runs(form, bar, &Volume::default());
                assert_eq!(
                    controls_of(&extras),
                    controls_of(extras_controls(form)),
                    "{form:?}"
                );
                assert_eq!(
                    run_width(&extras),
                    run_width(extras_controls(form)),
                    "{form:?}: the layout reserves what the drawn runs take"
                );
            }
        }
    }

    fn pane_run_in(form: Playbar, bar: Bar) -> Run {
        let mut runs = extras_runs(form, bar, &Volume::default());
        runs.extend_from_slice(&up_runs(form, &queue_of(4), bar));
        *runs
            .iter()
            .find(|run| run.control == Control::Queue)
            .expect("the queue is drawn once per form")
    }

    #[test]
    fn pane_labels_name_the_state_and_are_distinct() {
        let labels = [
            PaneButton::Unavailable,
            PaneButton::Shown,
            PaneButton::Hidden,
        ]
        .map(|pane| pane_run(pane).label);
        for label in labels {
            assert!(
                label.starts_with("Queue"),
                "{label} does not name the button"
            );
        }
        let mut unique = labels.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            labels.len(),
            "{labels:?} do not tell the states apart"
        );
    }

    #[test]
    fn the_queue_run_is_lit_only_when_the_pane_is_shown() {
        let lit = |pane| {
            pane_run_in(
                Playbar::Full,
                Bar {
                    pane,
                    ..Bar::default()
                },
            )
            .lit
        };
        assert!(lit(PaneButton::Shown), "a shown pane left the button unlit");
        assert!(!lit(PaneButton::Hidden), "a hidden pane lit the button");
        assert!(!lit(PaneButton::Unavailable), "the inert button was lit");
    }

    /// Whether the node a frame announced with `label` reads as disabled.
    fn announced_disabled(output: &egui::FullOutput, label: &str) -> bool {
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .and_then(|update| {
                update
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some(label))
                    .map(|(_, node)| node.is_disabled())
            })
            .unwrap_or_else(|| panic!("{label} is not announced"))
    }

    /// The label of the node the frame says has the keyboard.
    fn focused_label(output: &egui::FullOutput) -> Option<String> {
        let update = output.platform_output.accesskit_update.as_ref()?;
        update
            .nodes
            .iter()
            .find(|(id, _)| *id == update.focus)
            .and_then(|(_, node)| node.label().map(str::to_owned))
    }

    #[test]
    fn queue_is_live_only_when_the_pane_can_toggle_and_nothing_is_carried() {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        ctx.enable_accesskit();
        let mut queue = Queue::default();

        let live = |ctx: &egui::Context, queue: &mut Queue, bar: Bar| {
            let (_, output) = queue_frame(ctx, &tokens, queue, Playbar::Full, bar, Vec::new());
            !announced_disabled(&output, pane_run(bar.pane).label)
        };

        assert!(
            live(
                &ctx,
                &mut queue,
                Bar {
                    pane: PaneButton::Shown,
                    ..Bar::default()
                }
            ),
            "a shown pane left the queue button inert"
        );
        assert!(
            !live(&ctx, &mut queue, Bar::default()),
            "an unavailable pane button still acted"
        );

        egui::DragAndDrop::set_payload(&ctx, egui::Vec2::ZERO);
        assert!(
            !live(
                &ctx,
                &mut queue,
                Bar {
                    pane: PaneButton::Shown,
                    ..Bar::default()
                }
            ),
            "a block in flight left the pane switch live"
        );
    }

    #[test]
    fn shuffle_and_repeat_runs_follow_the_queue() {
        let mut queue = queue_of(4);

        let shuffle = down_runs(&queue)[0];
        assert_eq!(shuffle.control, Control::Shuffle);
        assert_eq!(shuffle.icon, Icon::Shuffle);
        assert_eq!(shuffle.label, "Shuffle off");
        assert!(!shuffle.lit);
        assert_eq!(shuffle.badge, None);

        let repeat = up_runs(Playbar::Full, &queue, Bar::default())[1];
        assert_eq!(repeat.control, Control::Repeat);
        assert_eq!(repeat.icon, Icon::Repeat);
        assert_eq!(repeat.label, "Repeat off");
        assert!(!repeat.lit, "off is not the mode's own state");
        assert_eq!(repeat.badge, None);

        queue.toggle_shuffle();
        queue.cycle_repeat();
        let shuffle = down_runs(&queue)[0];
        let repeat = up_runs(Playbar::Full, &queue, Bar::default())[1];
        assert_eq!(shuffle.label, "Shuffle on");
        assert!(shuffle.lit);
        assert_eq!(repeat.label, "Repeat all");
        assert!(repeat.lit);
        assert_eq!(repeat.badge, None);

        queue.cycle_repeat();
        let repeat = up_runs(Playbar::Full, &queue, Bar::default())[1];
        assert_eq!(repeat.label, "Repeat one");
        assert!(repeat.lit, "one is an on state");
        assert_eq!(repeat.badge, Some("1"));
    }

    /// The label a control's slot announces this frame.
    fn announced(ui: &Ui, control: Control) -> String {
        let id = controls::slot_id(ui, control, ());
        ui.ctx()
            .accesskit_node_builder(id, |node| node.label().map(str::to_owned))
            .flatten()
            .unwrap_or_else(|| panic!("{control:?} has no AccessKit node"))
    }

    /// The two mode buttons' announced labels, drawn into the current `ui`.
    fn mode_labels(ui: &mut Ui, tokens: &Tokens, queue: &Queue) -> Vec<String> {
        let can_act = |_| true;
        let pen = || Pen {
            tokens,
            claimed: None,
            volume: None,
            can_act: &can_act,
        };
        let mut x = 0.0;
        let _ = draw_run(ui, pen(), &down_runs(queue), &mut x, 20.0);
        let mut x = 100.0;
        let _ = draw_run(
            ui,
            pen(),
            &up_runs(Playbar::Full, queue, Bar::default()),
            &mut x,
            20.0,
        );
        [Control::Shuffle, Control::Repeat]
            .map(|control| announced(ui, control))
            .to_vec()
    }

    #[test]
    fn the_labels_are_announced_per_state() {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        ctx.enable_accesskit();
        let mut queue = queue_of(4);
        let labels = |queue: &mut Queue| {
            let mut announced = Vec::new();
            run(&ctx, input(Vec::new()), |ui| {
                announced = mode_labels(ui, &tokens, queue);
            });
            announced
        };

        assert_eq!(labels(&mut queue), ["Shuffle off", "Repeat off"]);
        queue.toggle_shuffle();
        assert_eq!(labels(&mut queue), ["Shuffle on", "Repeat off"]);
        queue.cycle_repeat();
        assert_eq!(labels(&mut queue), ["Shuffle on", "Repeat all"]);
        queue.cycle_repeat();
        assert_eq!(labels(&mut queue), ["Shuffle on", "Repeat one"]);
    }

    /// A mode is how the queue will play, so it can be set before there is
    /// anything to play.
    #[test]
    fn the_modes_are_live_on_an_empty_queue() {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let mut queue = Queue::default();
        assert!(!queue.can_toggle(), "arrange: nothing is loaded");

        let commands = click_control(&ctx, &tokens, &mut queue, Playbar::Full, Control::Shuffle);
        assert!(queue.shuffle(), "shuffle was inert with nothing playing");
        assert!(commands.iter().all(Option::is_none));

        let commands = click_control(&ctx, &tokens, &mut queue, Playbar::Full, Control::Repeat);
        assert_eq!(
            queue.repeat(),
            crate::repeat::Repeat::All,
            "repeat was inert with nothing playing"
        );
        assert!(commands.iter().all(Option::is_none));
    }

    #[test]
    fn a_click_on_shuffle_toggles_the_queue_and_sends_no_command() {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let mut queue = queue_of(4);
        assert!(!queue.shuffle());

        let commands = click_control(&ctx, &tokens, &mut queue, Playbar::Full, Control::Shuffle);

        assert!(queue.shuffle(), "the click did not reach the queue");
        assert!(
            commands.iter().all(Option::is_none),
            "a mode is the window's own state, so it asks for nothing: {commands:?}"
        );
    }

    #[test]
    fn three_clicks_on_repeat_cycle_off_all_one_off() {
        use crate::repeat::Repeat;

        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let mut queue = queue_of(4);
        let mut modes = Vec::new();

        for _ in 0..3 {
            let _ = click_control(&ctx, &tokens, &mut queue, Playbar::Full, Control::Repeat);
            modes.push(queue.repeat());
        }

        assert_eq!(modes, [Repeat::All, Repeat::One, Repeat::Off]);
    }

    #[test]
    fn a_held_enter_on_repeat_and_on_shuffle_changes_state_once() {
        use crate::repeat::Repeat;

        let enter = |repeat| Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat,
            modifiers: Modifiers::NONE,
        };
        let cases = [
            (Control::Shuffle, 1, (true, Repeat::Off)),
            (Control::Repeat, 5, (false, Repeat::All)),
        ];
        for (control, tabs, expected) in cases {
            let tokens = Tokens::DARK;
            let ctx = context(&tokens);
            let mut queue = queue_of(4);

            let _ = queue_frame(
                &ctx,
                &tokens,
                &mut queue,
                Playbar::Full,
                Bar::default(),
                Vec::new(),
            );
            for _ in 0..tabs {
                let _ = queue_frame(
                    &ctx,
                    &tokens,
                    &mut queue,
                    Playbar::Full,
                    Bar::default(),
                    vec![key(Key::Tab, Modifiers::NONE)],
                );
            }

            let mut states = Vec::new();
            for repeat in [false, true, true] {
                let _ = queue_frame(
                    &ctx,
                    &tokens,
                    &mut queue,
                    Playbar::Full,
                    Bar::default(),
                    vec![enter(repeat)],
                );
                states.push((queue.shuffle(), queue.repeat()));
            }

            assert_eq!(
                states, [expected; 3],
                "{control:?}: a held key stepped the mode more than once"
            );
        }
    }

    #[test]
    fn shuffle_is_inert_while_a_block_is_carried_and_repeat_is_not() {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let mut queue = queue_of(4);
        egui::DragAndDrop::set_payload(&ctx, egui::Vec2::ZERO);
        assert!(crate::view::carry::carrying(&ctx));

        let commands = click_control(&ctx, &tokens, &mut queue, Playbar::Full, Control::Shuffle);
        assert!(
            !queue.shuffle(),
            "a shuffle under a drag would refuse the drop"
        );
        assert!(
            commands.iter().all(Option::is_none),
            "an inert button asks for nothing: {commands:?}"
        );

        let _ = click_control(&ctx, &tokens, &mut queue, Playbar::Full, Control::Repeat);
        assert_eq!(
            queue.repeat(),
            crate::repeat::Repeat::All,
            "repeat moves no row"
        );
    }

    #[test]
    fn the_reduced_bar_keeps_both_live() {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let mut queue = queue_of(4);

        let _ = click_control(
            &ctx,
            &tokens,
            &mut queue,
            Playbar::Reduced,
            Control::Shuffle,
        );
        assert!(queue.shuffle(), "the reduced bar shed shuffle");

        let _ = click_control(&ctx, &tokens, &mut queue, Playbar::Reduced, Control::Repeat);
        assert_eq!(
            queue.repeat(),
            crate::repeat::Repeat::All,
            "the reduced bar shed repeat"
        );
    }

    #[test]
    fn a_lit_shuffle_paints_the_accent_and_an_unlit_one_does_not() {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let mut queue = queue_of(4);
        let bar = heart_bar();
        let centre = layout::playbar_for(bar, Playbar::Full).columns(bar)[1];
        let shuffle = transport_slot(centre, Control::Shuffle);

        let (_, output) = queue_frame(
            &ctx,
            &tokens,
            &mut queue,
            Playbar::Full,
            Bar::default(),
            Vec::new(),
        );
        assert!(
            !accent_inside(&output, &tokens, shuffle),
            "an off shuffle took the accent"
        );

        queue.toggle_shuffle();
        let (_, output) = queue_frame(
            &ctx,
            &tokens,
            &mut queue,
            Playbar::Full,
            Bar::default(),
            Vec::new(),
        );
        assert!(
            accent_inside(&output, &tokens, shuffle),
            "an on shuffle did not take the accent"
        );
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

    #[test]
    fn repeat_one_paints_a_one_and_the_other_states_do_not() {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let mut queue = queue_of(4);
        let bar = heart_bar();
        let centre = layout::playbar_for(bar, Playbar::Full).columns(bar)[1];
        let repeat = transport_slot(centre, Control::Repeat);

        for _ in 0..2 {
            let (_, output) = queue_frame(
                &ctx,
                &tokens,
                &mut queue,
                Playbar::Full,
                Bar::default(),
                Vec::new(),
            );
            assert!(
                !text_inside(&output, repeat).contains(&"1".to_owned()),
                "{:?} carried the badge",
                queue.repeat()
            );
            queue.cycle_repeat();
        }

        let (_, output) = queue_frame(
            &ctx,
            &tokens,
            &mut queue,
            Playbar::Full,
            Bar::default(),
            Vec::new(),
        );
        assert_eq!(queue.repeat(), crate::repeat::Repeat::One);
        assert!(
            text_inside(&output, repeat).contains(&"1".to_owned()),
            "repeat one carries no badge"
        );
    }

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

    /// The last button of a form's extras, which is fullscreen in both.
    fn last_extra(rect: Rect, form: Playbar) -> Rect {
        let [_, _, extras] = layout::playbar_for(rect, form).columns(rect);
        Rect::from_min_size(
            Pos2::new(
                extras.right() - extent::BUTTON,
                extras.center().y - extent::BUTTON / 2.0,
            ),
            Vec2::splat(extent::BUTTON),
        )
    }

    fn fullscreen_run_in(form: Playbar, bar: Bar) -> Run {
        *extras_runs(form, bar, &Volume::default())
            .iter()
            .find(|run| run.control == Control::Fullscreen)
            .expect("both forms draw the fullscreen button")
    }

    #[test]
    fn fullscreen_labels_name_the_state_and_are_distinct() {
        let labels = [None, Some(false), Some(true)].map(|state| fullscreen_run(state).label);
        for label in labels {
            assert!(
                label.starts_with("Full screen"),
                "{label} does not name the button"
            );
        }
        let mut unique = labels.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            labels.len(),
            "{labels:?} do not tell the states apart"
        );
    }

    #[test]
    fn the_fullscreen_run_is_lit_only_when_the_window_is() {
        assert!(fullscreen_run(Some(true)).lit);
        for state in [None, Some(false)] {
            assert!(!fullscreen_run(state).lit, "{state:?} lit the button");
        }
        for form in [Playbar::Full, Playbar::Reduced] {
            let bar = Bar {
                fullscreen: Some(true),
                ..Bar::default()
            };
            assert!(fullscreen_run_in(form, bar).lit, "{form:?}");
        }
    }

    #[test]
    fn fullscreen_is_live_only_when_the_window_state_is_known() {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        ctx.enable_accesskit();
        let mut queue = queue_of(4);

        let live = |ctx: &egui::Context, queue: &mut Queue, fullscreen| {
            let bar = Bar {
                fullscreen,
                ..Bar::default()
            };
            let (_, output) = queue_frame(ctx, &tokens, queue, Playbar::Full, bar, Vec::new());
            let label = fullscreen_run(fullscreen).label;
            !announced_disabled(&output, label)
        };

        assert!(
            live(&ctx, &mut queue, Some(false)),
            "a windowed window left the fullscreen button inert"
        );
        assert!(
            live(&ctx, &mut queue, Some(true)),
            "a fullscreen window left the button inert"
        );
        assert!(
            !live(&ctx, &mut queue, None),
            "a button with no state to act on still acted"
        );
    }

    #[test]
    fn a_click_on_fullscreen_reports_a_press_only_when_the_state_is_known() {
        for (fullscreen, pressed) in [(Some(false), true), (Some(true), true), (None, false)] {
            let tokens = Tokens::DARK;
            let ctx = context(&tokens);
            let mut queue = queue_of(4);
            let bar = Bar {
                fullscreen,
                ..Bar::default()
            };
            let at = last_extra(heart_bar(), Playbar::Full).center();

            let replies = click_at(&ctx, &tokens, &mut queue, Playbar::Full, bar, at);

            assert_eq!(
                replies.iter().any(|reply| reply.fullscreen),
                pressed,
                "{fullscreen:?}: the click reached the wrong answer: {replies:?}"
            );
            assert!(
                replies.iter().all(|reply| reply.command.is_none()),
                "{fullscreen:?}: a window request is not a command: {replies:?}"
            );
        }
    }

    #[test]
    fn a_held_enter_on_the_fullscreen_button_presses_once() {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        ctx.enable_accesskit();
        let mut queue = queue_of(4);
        let bar = Bar {
            fullscreen: Some(false),
            ..Bar::default()
        };
        let label = fullscreen_run(Some(false)).label;

        let _ = queue_frame(&ctx, &tokens, &mut queue, Playbar::Full, bar, Vec::new());
        let mut focused = None;
        for _ in 0..12 {
            let (_, output) = queue_frame(
                &ctx,
                &tokens,
                &mut queue,
                Playbar::Full,
                bar,
                vec![key(Key::Tab, Modifiers::NONE)],
            );
            focused = focused_label(&output);
            if focused.as_deref() == Some(label) {
                break;
            }
        }
        assert_eq!(
            focused.as_deref(),
            Some(label),
            "the fullscreen button is not in the tab order"
        );

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
            presses.push(reply.fullscreen);
        }

        assert_eq!(
            presses,
            vec![true, false, false],
            "a held Enter asked for fullscreen more than once"
        );
    }

    #[test]
    fn a_lit_fullscreen_paints_the_accent_and_an_unlit_one_does_not() {
        let tokens = Tokens::DARK;
        let ctx = context(&tokens);
        let mut queue = queue_of(4);
        let at = last_extra(heart_bar(), Playbar::Full);

        for (fullscreen, lit) in [(Some(true), true), (Some(false), false), (None, false)] {
            let bar = Bar {
                fullscreen,
                ..Bar::default()
            };
            let (_, output) =
                queue_frame(&ctx, &tokens, &mut queue, Playbar::Full, bar, Vec::new());
            assert_eq!(
                accent_inside(&output, &tokens, at),
                lit,
                "{fullscreen:?} drew the accent {lit}"
            );
        }
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
            let (_, output) =
                queue_frame(&ctx, &tokens, &mut queue, Playbar::Full, bar, Vec::new());
            assert_eq!(
                accent_inside(&output, &tokens, at),
                lit,
                "{:?} drew the accent {lit}",
                bar.pane
            );
        }
    }
}
