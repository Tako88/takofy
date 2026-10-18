//! The playbar: what is playing, the transport, and the extras.
//!
//! The bar is real but inert. Everything it shows comes from the transport — the
//! title, the state, the elapsed time, the glyph — and every control is drawn
//! waiting, so nothing looks live while doing nothing. The remaining time reads
//! as a dash, because a track's duration is in no metadata the app fetches.

use egui::{Align2, FontId, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2};

use crate::availability::Control;
use crate::icon::Icon;
use crate::layout::{self, Playbar};
use crate::player::{Transport, TransportState, format_position};
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

/// Draws the bar's three columns inside `rect`.
pub fn show(
    ui: &mut Ui,
    transport: &Transport,
    position_ms: u32,
    form: Playbar,
    rect: Rect,
    tokens: &Tokens,
) {
    let cols = layout::playbar_for(rect, form);
    let [now, centre, extras] = cols.columns(rect);

    row_centred(ui, "playbar-now", now, |ui| {
        now_column(ui, transport, tokens, form);
    });
    row_centred(ui, "playbar-centre", centre, |ui| {
        centre_column(ui, transport, position_ms, tokens);
    });
    row_centred(ui, "playbar-extras", extras, |ui| {
        extras_column(ui, tokens, form);
    });
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
    draw_run(ui, tokens, now_controls(form), &mut x, art.center().y);
}

/// The transport row and the progress row beneath it.
fn centre_column(ui: &mut Ui, transport: &Transport, position_ms: u32, tokens: &Tokens) {
    let top = ui.cursor().min.y;
    let down = [
        (Control::Shuffle, Icon::Shuffle),
        (Control::Previous, Icon::Previous),
    ];
    let up = [(Control::Next, Icon::Next), (Control::Repeat, Icon::Repeat)];
    let width = 2.0 * run_width(&down) + extent::BUTTON_LG;
    let mut x = ui.max_rect().center().x - width / 2.0;

    draw_run(ui, tokens, &down, &mut x, top + extent::BUTTON / 2.0);

    let play = Rect::from_min_size(Pos2::new(x, top), Vec2::splat(extent::BUTTON_LG));
    controls::round_button(
        ui,
        tokens,
        Control::PlayPause,
        play,
        play_glyph(transport.state),
        Size::MD,
        "Play",
    );
    x = play.right() + space::S1;

    draw_run(ui, tokens, &up, &mut x, top + extent::BUTTON / 2.0);

    // Elapsed, the seek track, and a remaining time there is no duration for.
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
    controls::slider(ui, tokens, Control::Seek, track, None, "Seek");

    let dash = recipe::spaced(
        ui,
        "–:––",
        FontId::new(text::XXS, fonts::mono(MonoWeight::Regular)),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XXS, tracking::SLIGHT),
    );
    ui.painter().galley(
        Pos2::new(track.right() + space::S3, elapsed.top()),
        dash,
        tokens.palette.text_subtle,
    );
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
    );
}

/// Draws a run left to right, advancing `x` past it.
fn draw_run(
    ui: &mut Ui,
    tokens: &Tokens,
    controls: &[(Control, Icon)],
    x: &mut f32,
    centre_y: f32,
) {
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
            controls::slider(ui, tokens, *control, rect, Some(0.7), "Volume");
            *x += extent::VOLUME_W + space::S2;
        } else {
            let rect = Rect::from_min_size(
                Pos2::new(*x, centre_y - extent::BUTTON / 2.0),
                Vec2::splat(extent::BUTTON),
            );
            controls::icon_button(ui, tokens, *control, rect, *glyph, Size::MD, "");
            *x += extent::BUTTON;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
