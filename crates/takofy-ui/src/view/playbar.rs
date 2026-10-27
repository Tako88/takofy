//! The playbar: what is playing, the transport, and the extras.
//!
//! Everything it shows comes from the transport — the title, the artist, the
//! elapsed time, the glyph. The seek track fills from the interpolation and
//! turns a click or an arrow into a command; a track with no known duration
//! draws the track alone and a dash for the time left.
//!
//! [`Reply`] carries every channel the bar has. `command` comes from two places
//! that do not wait on each other: a transport move — Previous, Next, PlayPause —
//! asks the queue which command it implies, while the seek track builds its own
//! from the click or the key it was given. `steer` is the same three presses and
//! the same seek asked of another device while it holds the sound; nothing there
//! reaches the local queue. `devices` is the devices button's own widget id, so
//! the shell can hang the list off the button it pressed, and `pane` and
//! `fullscreen` are the queue button's and the window's presses for the shell to
//! apply. `open` is the page a title or artist click named.
//!
//! The rest it changes in place and reports nothing about. A transport move goes
//! through the queue, which moves its row where the move means one, rewrites the
//! transport onto that track and hands back the command, and shuffle and repeat
//! toggle and cycle the queue's modes. The mute lid and the dragged or arrowed
//! level are written straight to the transport's volume, which the player reads
//! back afterwards, and a drag on the seek track writes its preview onto the
//! transport too: the seek command comes on release. A press on the heart posts the
//! new liked state through `heart::post`, a side channel of its own. While another
//! device holds the sound the local queue is left alone and so is the level, which
//! is the holder's.

use egui::{EventFilter, FontId, Key, Pos2, Rect, Ui, Vec2};

use crate::availability::Control;
use crate::cover::Cover;
use crate::devices::{Button, toggle_steer};
use crate::format::{format_position, format_remaining};
use crate::icon::Icon;
use crate::layout::{self, PaneButton, Playbar};
use crate::message::{Command, Steer};
use crate::player::{SEEK_STEP_MS, Transport, TransportState};
use crate::queue::Queue;
use crate::route::Page;
use crate::shuffle;
use crate::theme::icon::Size;
use crate::theme::{MonoWeight, Tokens, Weight, extent, fonts, radius, space, text, tracking};
use crate::volume::{self, Volume};

use super::art;
use super::carry;
use super::context_menu;
use super::controls;
use super::heart;
use super::link::{self, Face, Target};
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
/// The layout reserves the extras' width from the same shape, and the tests in
/// `playbar/tests/extras.rs` pin the two together so a control added to one
/// cannot drift from the other.
/// The now column's controls: the heart is the column's one button.
const NOW: [Run; 1] = [Run::plain(Control::Like, Icon::Heart)];
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

/// The extras' devices button: it lights while another device holds the sound.
fn devices_run(devices: Button) -> Run {
    Run {
        control: Control::Devices,
        icon: Icon::Devices,
        label: "Connect to a device",
        lit: devices == Button::Lit,
        badge: None,
    }
}

/// The now column's runs with the heart the track's state asks for.
///
/// A filled heart is the same size as an empty one; only the glyph, the label
/// and the tone change.
fn now_runs(liked: Option<bool>) -> Vec<Run> {
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
    let mut runs = NOW.to_vec();
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

/// The extras' runs with the queue's, the window's and the devices' own state.
fn extras_runs(form: Playbar, bar: Bar, volume: &Volume) -> Vec<Run> {
    let mut runs = extras_controls(form).to_vec();
    for run in &mut runs {
        match run.control {
            Control::Queue => *run = pane_run(bar.pane),
            Control::Devices => *run = devices_run(bar.devices),
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
    /// How the devices button draws.
    pub devices: Button,
}

impl Default for Bar {
    /// A bar told nothing draws both switches inert: an unknown pane and an
    /// unknown window are not the user's to act on, and no list has arrived.
    fn default() -> Self {
        Self {
            queue_claimed: false,
            like: None,
            pane: PaneButton::Unavailable,
            fullscreen: None,
            devices: Button::Off,
        }
    }
}

/// The other device the bar draws while it holds the sound.
///
/// The line replaces the track's artist: the holder's name, or what just
/// happened to it. `steerable` decides whether the transport can act, because
/// the local queue says nothing about what another device can do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Remote<'a> {
    pub line: &'a str,
    pub steerable: bool,
}

/// What the bar's controls asked for this frame.
///
/// A transport move and the seek track are a command the shell hands on; the pane
/// and the window are the shell's own to change, so their presses are reported as
/// presses. A steer is the holder's transport and never reaches the local queue.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Reply {
    pub command: Option<Command>,
    /// The transport action another device is asked for, while it holds the
    /// sound.
    pub steer: Option<Steer>,
    /// The devices button's own widget, when it was pressed.
    pub devices: Option<egui::Id>,
    /// Whether the queue button was pressed.
    pub pane: bool,
    /// Whether the fullscreen button was pressed: the window is the shell's to ask.
    pub fullscreen: bool,
    /// The page a link click named.
    pub open: Option<Page>,
}

/// One column's press: a command to apply, a steer to send, and the control
/// whose own state the shell owns.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct Pressed {
    command: Option<Command>,
    steer: Option<Steer>,
    control: Option<Control>,
}

/// Draws the bar's three columns inside `rect` and reports what a control in it
/// asked for.
///
/// With `remote` the transport is the holder's display transport and the bar is
/// read-only towards the local queue: volume, mute, shuffle, repeat and the
/// queue button are inert, the caption line replaces the track's artist, and the
/// transport reports a [`Reply::steer`] instead of a command. The shell owns the
/// display transport and copies the local level into it every frame, because
/// volume is inert while remote and the holder's own level means nothing here.
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
    remote: Option<Remote<'_>>,
    rect: Rect,
    cover: Cover,
    tokens: &Tokens,
) -> Reply {
    let cols = layout::playbar_for(rect, form);
    let [now, centre, extras] = cols.columns(rect);

    let now_reply = row_centred(ui, "playbar-now", now, |ui| {
        now_column(ui, transport, cover, tokens, bar.like, remote)
    });
    if let Some(liked) = now_reply.like
        && let Some(now_playing) = transport.now_playing.as_ref()
    {
        heart::post(
            ui.ctx(),
            heart::Ask {
                tracks: vec![now_playing.uri.clone()],
                liked,
            },
        );
    }
    let pressed = row_centred(ui, "playbar-centre", centre, |ui| {
        centre_column(ui, transport, queue, position_ms, form, bar, remote, tokens)
    });
    let (extras_press, devices) = row_centred(ui, "playbar-extras", extras, |ui| {
        extras_column(
            ui,
            tokens,
            form,
            bar,
            remote.is_some(),
            &mut transport.volume,
        )
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
        steer: pressed.steer,
        devices,
        pane: control == Some(Control::Queue),
        fullscreen: control == Some(Control::Fullscreen),
        open: now_reply.open,
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

/// What the seek says to a screen reader: the time it is at, and the track's
/// length when one is known. The painted readouts either side carry no node of
/// their own, so this is the only place the pair is spoken.
fn seek_spoken(position_ms: u32, duration_ms: Option<u32>) -> String {
    match duration_ms {
        Some(duration) => format!(
            "{} of {}",
            format_position(position_ms),
            format_position(duration)
        ),
        None => format_position(position_ms),
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

/// What one run is in a row: a square button, or the level's own slot.
fn slot_of(run: &Run) -> layout::playbar::Slot {
    if run.control == VOLUME {
        layout::playbar::Slot::Volume
    } else {
        layout::playbar::Slot::Button
    }
}

/// The width one run of controls occupies, margins and gaps included. The view
/// draws it and the layout reserves it, and both read this.
fn run_width(controls: &[Run]) -> f32 {
    layout::playbar::row_width(controls.iter().map(slot_of))
}

/// What the now column asked for: the heart's new state and the page a link
/// named.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct Now {
    like: Option<bool>,
    open: Option<Page>,
}

/// The art tile, the title, the artist, and the heart beside them.
///
/// The second line is the track's artist, or — while another device holds the
/// sound — the holder's own line, which names no page of ours. Neither exists on
/// an idle bar or a track with no artist, so one line is drawn.
fn now_column(
    ui: &mut Ui,
    transport: &Transport,
    cover: Cover,
    tokens: &Tokens,
    liked: Option<bool>,
    remote: Option<Remote<'_>>,
) -> Now {
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

    let controls = run_width(&NOW);
    let title_font = egui::FontId::new(text::SM, fonts::sans(Weight::Medium));
    let second_font = FontId::new(text::XXS, fonts::mono(MonoWeight::Regular));

    let playing = transport.now_playing.as_ref();
    // A holder's push names its track only by URI, so an unnamed remote track
    // has no title to draw: the caption beside it names the device instead.
    let title = match (playing, remote) {
        (Some(now), _) => now.title.as_str(),
        (None, Some(_)) => "",
        (None, None) => "Nothing playing",
    };
    let second: Option<(&str, Option<Target<'_>>)> = match remote {
        Some(remote) => (!remote.line.is_empty()).then_some((remote.line, None)),
        None => playing.and_then(|now| {
            (!now.artists.is_empty()).then(|| {
                (
                    now.artists.as_str(),
                    now.artist_uri.as_deref().map(Target::Credit),
                )
            })
        }),
    };
    // The row needs the width each line would take unclamped, so the controls
    // sit where the words end when they fit and on the column's edge when they
    // do not. The wider of the two lines decides.
    let natural = {
        let title_w = recipe::spaced(ui, title, title_font.clone(), tokens.palette.text, 0.0)
            .size()
            .x;
        let second_w = second.map_or(0.0, |(label, _)| {
            recipe::spaced(
                ui,
                label,
                second_font.clone(),
                tokens.palette.text_subtle,
                0.0,
            )
            .size()
            .x
        });
        title_w.max(second_w)
    };
    let row = layout::playbar::now_row(
        ui.max_rect().left(),
        ui.max_rect().right(),
        art_rect.width(),
        controls,
        natural,
    );
    let title_face = Face {
        font: title_font,
        tracking: 0.0,
    };
    let second_face = Face {
        font: second_font,
        tracking: 0.0,
    };
    let title_line = link::lay(ui, title, &title_face, row.text_w);
    let second_line =
        second.map(|(label, target)| (link::lay(ui, label, &second_face, row.text_w), target));
    // The two lines are one block, centred on the art like the reference's flex
    // column: stacking them from fixed offsets puts the pair off-centre whenever
    // either line's height is not the one assumed.
    let block = title_line.size().y + second_line.as_ref().map_or(0.0, |(line, _)| line.size().y);
    let mut y = art_rect.center().y - block / 2.0;
    let clip = ui.max_rect();
    let title = link::Placed {
        pos: Pos2::new(row.text_left, y),
        galley: title_line,
    }
    .link(
        ui,
        ui.scope_id().with("playbar-title"),
        clip,
        playing
            .and_then(|now| now.album_uri.as_deref())
            .map(Target::Album),
        tokens.palette.text,
        tokens,
    );
    y += title.rect.height();
    let mut open = title.page;
    if let Some((line, target)) = second_line {
        let shown = link::Placed {
            pos: Pos2::new(row.text_left, y),
            galley: line,
        }
        .link(
            ui,
            ui.scope_id().with("playbar-artist"),
            clip,
            target,
            tokens.palette.text_subtle,
            tokens,
        );
        open = open.or(shown.page);
    }

    let mut x = row.controls_x;
    let clicked = draw_run(
        ui,
        Pen {
            tokens,
            claimed: None,
            volume: None,
            // A heart with no state to act on is inert, as a row's is.
            can_act: &|control| control != Control::Like || liked.is_some(),
        },
        &now_runs(liked),
        &mut x,
        art_rect.center().y,
    );
    let like = clicked.filter(|control| *control == Control::Like);
    // egui reports a focused button's Enter or Space as a click, repeats too, so
    // only a fresh press is the user's, as on a row.
    let pointer = ui.input(|input| input.pointer.button_clicked(egui::PointerButton::Primary));
    let held = like.is_some() && !pointer && context_menu::held_repeat(ui);
    Now {
        like: like.filter(|_| !held).map(|_| liked != Some(true)),
        open,
    }
}

/// The transport row and the progress row beneath it. The command is applied
/// after the whole column is drawn, so a transport move cannot repaint the row
/// it came from in the same frame.
#[expect(
    clippy::too_many_arguments,
    reason = "the column draws the transport, its seek and the mode it is drawn in"
)]
fn centre_column(
    ui: &mut Ui,
    transport: &mut Transport,
    queue: &mut Queue,
    position_ms: u32,
    form: Playbar,
    bar: Bar,
    remote: Option<Remote<'_>>,
    tokens: &Tokens,
) -> Pressed {
    let stack = layout::playbar::transport_stack();
    let top = row_centre(ui) - stack / 2.0;
    let carrying = carry::carrying(ui.ctx());
    let claimed = bar.queue_claimed.then_some(Control::Queue);
    // Another device answers to its own state and never to this queue: an empty
    // local queue says nothing about what the holder can do.
    let live = |control: Control| match remote {
        Some(remote) => {
            matches!(
                control,
                Control::Previous | Control::Next | Control::PlayPause
            ) && remote.steerable
        }
        None => match control {
            Control::Queue => pane_live(bar.pane, carrying),
            other => transport_live(other, queue, carrying),
        },
    };
    let down = down_runs(queue);
    let row = layout::playbar::transport_row(ui.max_rect().center().x, run_width(&down));
    let mut x = row.down_x;

    let mut clicked = draw_run(
        ui,
        Pen {
            tokens,
            claimed,
            // The centre's runs are buttons: the slider is the extras' alone.
            volume: None,
            can_act: &live,
        },
        &down,
        &mut x,
        top + extent::BUTTON / 2.0,
    );

    let play = Rect::from_min_size(Pos2::new(row.play_x, top), Vec2::splat(extent::BUTTON_LG));
    // The grey tile is the idle transport alone: an empty queue never started a row.
    let toggle_live = match remote {
        Some(remote) => remote.steerable,
        None => queue.can_toggle(),
    };
    let play_slot = controls::play_tile(
        ui,
        tokens,
        Control::PlayPause,
        play,
        play_glyph(transport.state),
        "Play",
        toggle_live,
    );
    if play_slot.response.clicked() {
        clicked = Some(Control::PlayPause);
    }
    x = row.up_x;

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

    // Elapsed, the seek track, and the time left: one row that takes the column
    // until its cap, with a readout and a gap on each side of the track.
    let centre = ui.max_rect().center().x;
    let seek = layout::playbar::seek_row(ui.max_rect().width());
    let line = layout::playbar::seek_line(centre, top, seek);
    let text_top = line.center().y - text::XXS;
    let readout_font = FontId::new(text::XXS, fonts::mono(MonoWeight::Regular));

    let elapsed = recipe::spaced(
        ui,
        &format_position(position_ms),
        readout_font.clone(),
        tokens.palette.text_muted,
        recipe::tracking_pts(text::XXS, tracking::SLIGHT),
    );
    ui.painter().galley(
        Pos2::new(line.left(), text_top),
        elapsed,
        tokens.palette.text_muted,
    );

    let track = Rect::from_min_size(
        Pos2::new(
            line.left() + extent::TIME_W + space::S3,
            line.center().y - extent::SLIDER_H / 2.0,
        ),
        Vec2::new(seek.track, extent::SLIDER_H),
    );
    // The holder's own length decides whether its seek can act; the local
    // transport's rule would ask this window's player instead.
    let scope = SeekScope::of(remote);
    let seekable = scope.enabled(transport);
    let slot = controls::slider(
        ui,
        tokens,
        Control::Seek,
        track,
        seek_fraction(transport, position_ms),
        "Seek",
        seekable,
    );
    // Both readouts are painted, so the seek is the node that says both times.
    let _ = ui.ctx().accesskit_node_builder(slot.response.id, |node| {
        node.set_value(seek_spoken(position_ms, transport.duration_ms));
    });
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
        readout_font,
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XXS, tracking::SLIGHT),
    );
    ui.painter().galley(
        Pos2::new(line.right() - readout.size().x, text_top),
        readout,
        tokens.palette.text_subtle,
    );

    let command = seek_command(ui, transport, &slot, track, position_ms, scope);
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
    let (command, steer) = match remote {
        // Nothing in remote mode reaches the local queue: the presses are the
        // holder's, and the scrubber's own command is the only other steer.
        Some(_) => (
            None,
            transport_steer(clicked, transport.state)
                .or_else(|| command.as_ref().and_then(steer_of)),
        ),
        None => (
            command.or(match clicked {
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
            None,
        ),
    };
    Pressed {
        command,
        steer,
        control: clicked,
    }
}

/// The steer one of the transport's own buttons asks for, while another device
/// holds the sound.
fn transport_steer(clicked: Option<Control>, state: TransportState) -> Option<Steer> {
    match clicked? {
        Control::Previous => Some(Steer::SkipPrev),
        Control::Next => Some(Steer::SkipNext),
        Control::PlayPause => toggle_steer(state),
        _ => None,
    }
}

/// The steer the scrubber's own output asks for.
///
/// Only the seek track's output is a steer: every other command this view builds
/// belongs to the local player.
fn steer_of(command: &Command) -> Option<Steer> {
    match command {
        Command::Seek { position_ms } => Some(Steer::SeekTo(*position_ms)),
        _ => None,
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

/// What a seek needs before it can act.
///
/// This window's player needs a track it has loaded; a holder's transport is
/// only drawn, so its known length is what the bar enables its slider on.
#[derive(Clone, Copy)]
enum SeekScope {
    Local,
    Remote { steerable: bool },
}

impl SeekScope {
    fn of(remote: Option<Remote<'_>>) -> Self {
        match remote {
            Some(remote) => Self::Remote {
                steerable: remote.steerable,
            },
            None => Self::Local,
        }
    }

    /// Whether the seek track can act. The slider is drawn from this and the
    /// scrub runs under it, so the two cannot disagree.
    fn enabled(self, transport: &Transport) -> bool {
        match self {
            Self::Local => transport.seekable(),
            Self::Remote { steerable } => steerable && transport.length_known(),
        }
    }

    /// Points the bar at the place a fraction of the track lands on, until the
    /// transport reports the seek it asks for.
    fn preview(self, transport: &mut Transport, fraction: f32) {
        match self {
            Self::Local => transport.scrub_to(fraction),
            Self::Remote { .. } => {
                transport.scrub = transport.length_target(fraction);
                transport.dragging = true;
            }
        }
    }

    /// The command a fraction of the track asks for.
    fn to(self, transport: &Transport, fraction: f32) -> Option<Command> {
        match self {
            Self::Local => transport.seek_to_fraction(fraction),
            Self::Remote { .. } => transport
                .length_target(fraction)
                .map(|position_ms| Command::Seek { position_ms }),
        }
    }

    /// The command that jumps `delta_ms` from `position_ms`.
    fn by(self, transport: &Transport, position_ms: u32, delta_ms: i64) -> Option<Command> {
        match self {
            Self::Local => transport.seek_by(delta_ms),
            Self::Remote { .. } => transport
                .length_by(position_ms, delta_ms)
                .map(|position_ms| Command::Seek { position_ms }),
        }
    }
}

/// The seek a click or a key on the focused track asks for.
fn seek_command(
    ui: &Ui,
    transport: &mut Transport,
    slot: &controls::Slot,
    track: Rect,
    position_ms: u32,
    scope: SeekScope,
) -> Option<Command> {
    // A drag follows the pointer and asks once, on release; the bar is drawn from
    // the transport's own position, so the preview comes for free.
    if slot.response.dragged() {
        if let Some(pointer) = slot.response.interact_pointer_pos() {
            let fraction = controls::fraction_at(pointer.x, track.left(), track.width());
            scope.preview(transport, fraction);
        }
        return None;
    }
    if slot.response.drag_stopped() {
        return transport.scrub_end();
    }
    if slot.response.clicked() {
        let pointer = slot.response.interact_pointer_pos()?;
        let fraction = controls::fraction_at(pointer.x, track.left(), track.width());
        return scope.to(transport, fraction);
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
            command = scope.to(transport, 0.0);
        } else if input.key_pressed(Key::End) {
            command = scope.to(transport, 1.0);
        } else {
            let step = i64::from(SEEK_STEP_MS) * if input.modifiers.shift { 4 } else { 1 };
            if input.key_pressed(Key::ArrowLeft) {
                command = scope.by(transport, position_ms, -step);
            } else if input.key_pressed(Key::ArrowRight) {
                command = scope.by(transport, position_ms, step);
            }
        }
    });
    command
}

/// The buttons the reference sheds first as the window narrows, and the volume.
///
/// Reports the control clicked and, when it was the devices button, that
/// button's own widget so the shell can open the list from it.
fn extras_column(
    ui: &mut Ui,
    tokens: &Tokens,
    form: Playbar,
    bar: Bar,
    remote: bool,
    volume: &mut Volume,
) -> (Option<Control>, Option<egui::Id>) {
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
                Control::Devices => bar.devices != Button::Off,
                // The level is this window's, which means nothing while another
                // device holds the sound, and the queue is not its list.
                Control::Mute | Control::Volume | Control::Queue if remote => false,
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
    let clicked = clicked.filter(|_| !held);
    let devices = clicked
        .filter(|control| *control == Control::Devices)
        .map(|_| controls::slot_id(ui, Control::Devices, ()));
    (clicked, devices)
}

/// Where the devices button is drawn inside the bar `rect`, or `None` when the
/// width sheds it with the reduced extras.
pub fn devices_button(rect: Rect, form: Playbar) -> Option<Rect> {
    run_rect(rect, form, Control::Devices)
}

/// Where the extras draw one of their runs, walked from the column's right edge
/// the way the draw walks. `None` when this width does not draw it.
fn run_rect(rect: Rect, form: Playbar, control: Control) -> Option<Rect> {
    let [_, _, extras] = layout::playbar_for(rect, form).columns(rect);
    let runs = extras_controls(form);
    let mut x = extras.right() - run_width(runs);
    for (index, run) in runs.iter().enumerate() {
        let slot = layout::playbar::place_run(&mut x, index == 0, slot_of(run), extras.center().y);
        if run.control == control {
            return Some(slot);
        }
    }
    None
}

/// A `BUTTON` square whose left edge is `x` and whose centre is `centre_y`.
///
/// Walked through `place_run` as the row's leading slot — it adds no gap, and
/// `x` is discarded — so the rect the shell derives and the rect the draw paints
/// are one definition rather than two.
fn square(x: f32, centre_y: f32) -> Rect {
    let mut at = x;
    layout::playbar::place_run(&mut at, true, layout::playbar::Slot::Button, centre_y)
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
            let stack = layout::playbar::transport_stack();
            let y = centre.center().y - stack / 2.0 + extent::BUTTON / 2.0;
            let row = layout::playbar::transport_row(centre.center().x, run_width(&DOWN));
            square(row.up_x + run_width(up_controls(form)) - extent::BUTTON, y)
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
        let rect = layout::playbar::place_run(x, index == 0, slot_of(run), centre_y);
        if run.control == VOLUME {
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
                    can_act(run.control),
                );
                drag_volume(ui, volume, &slot, rect);
            }
        } else {
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
        }
    }
    clicked
}

#[cfg(test)]
mod tests;
