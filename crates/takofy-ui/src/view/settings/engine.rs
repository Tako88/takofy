//! The Sound section: the note over it, and the two engine rows.
//!
//! An engine row carries the whole choice: a marker, the name, the line that
//! says what the engine is, and the word for its state. The chosen row is the
//! one the next start plays through, which is why the row that is not chosen
//! reads "Switch to it" rather than a status of its own.

use egui::{Pos2, Role, Sense, Stroke, StrokeKind, Ui, Vec2, vec2};

use crate::availability::Control;
use crate::format::{status_word, switch_word};
use crate::prefs::SoundChoice;
use crate::settings::RowStatus;
use crate::theme::{Tokens, Weight, extent, fonts, radius, space, stroke, text, tracking};
use crate::view::{controls, section};

use super::{
    Presses, Reading, block, body_face, caps, held, line, measure, name_face, paint, word,
};

/// The note over the section: the promise that the choice is about sound alone.
const NOTE: &str = "Where the sound comes out. Everything you see — the library, the pages, search — comes from Takofy either way.";

const BUILT_IN: &str = "Plays on this machine.";

/// The line the Soloist row carries until it is set up and paired.
const NOT_READY: &str =
    "Spotify's own player. Needed when Spotify will not unlock the audio for this app.";

/// The line the Soloist row carries once it is running as this window's device.
const READY: &str = "Spotify's own player, running beside Takofy as ";

/// The note a choice that is not the running one carries.
const RESTART: &str = "Takes effect the next time Takofy starts.";

/// Draws the note and both rows.
pub(super) fn sound(ui: &mut Ui, reading: &Reading, tokens: &Tokens, presses: &mut Presses) {
    section::head(ui, "Sound", "", tokens);
    let font = super::note_face();
    let width = ui
        .available_width()
        .min(measure(ui, &font, extent::SETTINGS_TEXT_CH));
    let note = block(ui, NOTE, font, tokens.palette.text_subtle, width, 3);
    word(ui, note, tokens.palette.text_subtle);
    ui.add_space(space::S4);

    let chosen = reading.status;
    let ready = reading.chosen == SoundChoice::Soloist && chosen == RowStatus::ReadyPaired;
    let soloist = if ready {
        Copy::Device(&reading.name)
    } else {
        Copy::Plain(NOT_READY)
    };

    row(
        ui,
        tokens,
        Engine {
            name: "Takofy",
            line: Copy::Plain(BUILT_IN),
            status: if reading.chosen == SoundChoice::BuiltIn {
                status_word(chosen)
            } else {
                switch_word()
            },
            chosen: reading.chosen == SoundChoice::BuiltIn,
            choice: SoundChoice::BuiltIn,
            control: Control::EngineBuiltIn,
            restart: reading.chosen == SoundChoice::BuiltIn && reading.chosen != reading.running,
        },
        presses,
    );
    ui.add_space(space::S2);
    row(
        ui,
        tokens,
        Engine {
            name: "Spotify Soloist",
            line: soloist,
            status: if reading.chosen == SoundChoice::Soloist {
                status_word(chosen)
            } else {
                switch_word()
            },
            chosen: reading.chosen == SoundChoice::Soloist,
            choice: SoundChoice::Soloist,
            control: Control::EngineSoloist,
            restart: reading.chosen == SoundChoice::Soloist && reading.chosen != reading.running,
        },
        presses,
    );
}

/// The sentence under an engine's name.
enum Copy<'a> {
    /// One line, all in the same face.
    Plain(&'a str),
    /// The ready sentence, whose device name is set apart.
    Device(&'a str),
}

/// One engine row.
struct Engine<'a> {
    name: &'a str,
    line: Copy<'a>,
    status: &'static str,
    chosen: bool,
    choice: SoundChoice,
    control: Control,
    restart: bool,
}

fn row(ui: &mut Ui, tokens: &Tokens, row: Engine<'_>, presses: &mut Presses) {
    let width = ui.available_width();
    let pad = space::S3;
    let gap = space::S3;
    let status_color = if row.chosen {
        tokens.palette.text
    } else {
        tokens.palette.text_subtle
    };
    let status = caps(ui, row.status, status_color);
    let text_w = (width - 2.0 * pad - extent::ENGINE_MARKER - 2.0 * gap - status.size().x).max(0.0);
    let name = line(
        ui,
        row.name,
        name_face(),
        tokens.palette.text,
        tracking::TIGHT,
        text_w,
    );
    let body = match row.line {
        Copy::Plain(text) => block(ui, text, body_face(), tokens.palette.text_muted, text_w, 3),
        Copy::Device(name) => held(
            ui,
            &[
                (READY, body_face()),
                (
                    name,
                    egui::FontId::new(text::SM, fonts::sans(Weight::Semibold)),
                ),
                (".", body_face()),
            ],
            tokens.palette.text_muted,
            text_w,
            3,
        ),
    };
    let restart = row.restart.then(|| {
        block(
            ui,
            RESTART,
            body_face(),
            tokens.palette.text_subtle,
            text_w,
            2,
        )
    });
    let content = name.size().y
        + space::S1
        + body.size().y
        + restart
            .as_ref()
            .map_or(0.0, |line| space::S1 + line.size().y);
    let height = (2.0 * pad + content).max(2.0 * pad + status.size().y);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());

    let (fill, border) = if row.chosen {
        (tokens.palette.surface_raised, tokens.palette.border_strong)
    } else {
        (tokens.palette.surface, tokens.palette.border)
    };
    ui.painter().rect_filled(rect, radius::SM, fill);
    ui.painter().rect_stroke(
        rect,
        radius::SM,
        Stroke::new(stroke::HAIRLINE, border),
        StrokeKind::Inside,
    );

    let marker = egui::Rect::from_min_size(
        Pos2::new(rect.left() + pad, rect.top() + pad + space::S1),
        Vec2::splat(extent::ENGINE_MARKER),
    );
    if row.chosen {
        ui.painter()
            .rect_filled(marker, radius::XS, tokens.palette.text);
    } else {
        ui.painter().rect_stroke(
            marker,
            radius::XS,
            Stroke::new(stroke::HAIRLINE, tokens.palette.border_strong),
            StrokeKind::Inside,
        );
    }

    let left = rect.left() + pad + extent::ENGINE_MARKER + gap;
    let mut top = rect.top() + pad;
    paint(ui, Pos2::new(left, top), &name, tokens.palette.text);
    top += name.size().y + space::S1;
    paint(ui, Pos2::new(left, top), &body, tokens.palette.text_muted);
    if let Some(restart) = &restart {
        top += body.size().y + space::S1;
        paint(
            ui,
            Pos2::new(left, top),
            restart,
            tokens.palette.text_subtle,
        );
    }
    ui.painter().galley(
        Pos2::new(rect.right() - pad - status.size().x, rect.top() + pad),
        status,
        status_color,
    );

    let slot = controls::slot(
        ui,
        row.control,
        Role::RadioButton,
        rect,
        row.name,
        tokens,
        true,
    );
    let _ = ui
        .ctx()
        .accesskit_node_builder(slot.response.id, |node| node.set_selected(row.chosen));
    if slot.response.clicked() {
        presses.engine = Some(row.choice);
    }
}
