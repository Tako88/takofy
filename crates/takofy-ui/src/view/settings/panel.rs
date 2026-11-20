//! The Soloist setup: three steps, the ready frame and the expiry a build owes.
//!
//! The block is a panel inside the page, never a dialog: the reference has no
//! modal and this one asks for files and a key, so covering the window would hide
//! the window the listener has to drag the file from.

use egui::{Margin, Pos2, Rect, Sense, Stroke, Ui, Vec2, vec2};

use crate::availability::Control;
use crate::format::{
    expiry_line, expiry_title, not_found_line, pair_failure, program_line, setup_failure,
};
use crate::message::{KeyState, SetupFault, SoloistPage};
use crate::settings::{Pairing, Settings, Step};
use crate::theme::{Tokens, extent, opacity, radius, space, stroke};

use super::{
    Field, KEY_MAX, PATH_MAX, Presses, Reading, body, caps, done_label, done_row, dropped, field,
    found, ghost, ghost_size, hint, paint, spoken_line, text_button, title,
};

/// The keychain sentence the plan adds under the key step.
const KEYCHAIN: &str = "The key is stored in your system keychain. While Takofy runs Soloist it is visible to other programs on this computer.";

/// What a listener sees when the daemon has not said whether it is paired.
const ALREADY_PAIRED: &str =
    "Already paired? Switch to Soloist and restart — Takofy will say if it still needs pairing.";

const MARK: &str = "Downloaded but not extracted? Right-click the file → Extract, then look again. Or drag the file onto this window.";

const PHONE_HINT: &str = "On a computer instead? The same list is in the Spotify app's bottom bar.";

/// Draws the block, and takes a dropped file as a proposal for the program.
pub(super) fn show(
    ui: &mut Ui,
    reading: &Reading,
    settings: &mut Settings,
    tokens: &Tokens,
    presses: &mut Presses,
) {
    if let Some(path) = dropped(ui) {
        presses.dropped = Some(path);
    }
    ui.add_space(space::S3);
    egui::Frame::new()
        .fill(tokens.palette.panel)
        .stroke(Stroke::new(stroke::HAIRLINE, tokens.palette.border))
        .corner_radius(radius::MD)
        .inner_margin(Margin::same(space::S4 as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            if reading.all_done() {
                ready(ui, reading, tokens, presses);
                return;
            }
            step_program(ui, reading, settings, tokens, presses, reading.steps[0]);
            for (index, kind) in reading.steps.iter().enumerate().skip(1) {
                rule(ui, tokens);
                match index {
                    1 => step_key(ui, reading, settings, tokens, presses, *kind),
                    _ => step_pair(ui, reading, tokens, presses, *kind),
                }
            }
        });
}

/// The rule and the air between two steps.
fn rule(ui: &mut Ui, tokens: &Tokens) {
    ui.add_space(space::S3);
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), stroke::HAIRLINE), Sense::hover());
    ui.painter()
        .rect_filled(rect, radius::NONE, tokens.palette.border);
    ui.add_space(space::S3);
}

/// A step's number column and its content, faded while it is still ahead.
fn step<R>(
    ui: &mut Ui,
    tokens: &Tokens,
    number: &str,
    kind: Step,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let before = ui.opacity();
    if kind == Step::Ahead {
        ui.multiply_opacity(opacity::AHEAD);
    }
    let out = ui
        .horizontal_top(|ui| {
            let label = caps(ui, number, tokens.palette.text_subtle);
            let (rect, _) = ui.allocate_exact_size(vec2(space::S6, label.size().y), Sense::hover());
            paint(ui, rect.min, &label, tokens.palette.text_subtle);
            ui.add_space(space::S3);
            ui.vertical(|ui| add(ui)).inner
        })
        .inner;
    ui.set_opacity(before);
    out
}

/// Step one: find the program, or take the one the listener points at.
fn step_program(
    ui: &mut Ui,
    reading: &Reading,
    settings: &mut Settings,
    tokens: &Tokens,
    presses: &mut Presses,
    kind: Step,
) {
    if kind == Step::Done {
        let text = program_text(reading);
        if reading.all_done() {
            let recheck = ghost_size(ui, "Recheck");
            let change = ghost_size(ui, "Change the program");
            done_row(ui, tokens, &text, recheck.x + space::S3 + change.x, |ui| {
                ui.horizontal(|ui| {
                    if ghost(ui, tokens, Control::RescanProgram, "Recheck", true) {
                        presses.path = true;
                    }
                    ui.add_space(space::S3);
                    // The ready frame has one exit, and the intent behind it
                    // is not on the settings state yet.
                    let _ = ghost(
                        ui,
                        tokens,
                        Control::ChangeProgram,
                        "Change the program",
                        false,
                    );
                });
            });
            return;
        }
        let label = "Program · done";
        let width = caps(ui, label, tokens.palette.text_subtle).size().x;
        done_row(ui, tokens, &text, width, |ui| {
            done_label(ui, tokens, label);
        });
        return;
    }

    step(ui, tokens, "01", kind, |ui| {
        title(ui, tokens, "Get Spotify Soloist");
        body(
            ui,
            tokens,
            "Spotify does not let Takofy ship it, so download it once and unpack it. Takofy finds it by itself — it is looking in your Downloads folder.",
        );
        ui.add_space(space::S3);
        ui.horizontal(|ui| {
            if text_button(
                ui,
                tokens,
                Control::OpenDownload,
                "Open the download page",
                true,
            ) {
                presses.open = Some(SoloistPage::Download);
            }
            ui.add_space(space::S3);
            let text = program_text(reading);
            found(ui, tokens, &text, ui.available_width());
        });
        ui.add_space(space::S3);
        let mut submitted = false;
        let look = ghost_size(ui, "Look again");
        // A field narrower than its floor reads as no field at all, so the button
        // takes its own line instead.
        let stacked = ui.available_width() - look.x - space::S3 < extent::PATH_FIELD_MIN_W;
        if stacked {
            let width = ui.available_width();
            let out = field(ui, tokens, path_field(), width, settings.path_field_mut());
            submitted = out.enter;
            if ghost(ui, tokens, Control::RescanProgram, "Look again", true) {
                submitted = true;
            }
        } else {
            ui.horizontal(|ui| {
                let width = (ui.available_width() - look.x - space::S3).max(0.0);
                let out = field(ui, tokens, path_field(), width, settings.path_field_mut());
                submitted = out.enter;
                ui.add_space(space::S3);
                if ghost(ui, tokens, Control::RescanProgram, "Look again", true) {
                    submitted = true;
                }
            });
        }
        if submitted {
            presses.path = true;
        }
        hint(ui, tokens, MARK);
        if reading.fault == Some(SetupFault::NotSoloist) {
            ui.add_space(space::S2);
            spoken_line(
                ui,
                setup_failure(SetupFault::NotSoloist),
                tokens.palette.text,
            );
        }
    });
}

/// The program field, which takes a folder or a program the listener points at.
fn path_field<'a>() -> Field<'a> {
    Field {
        control: Control::PathField,
        hint: "Moved it? Paste the folder or the program here",
        limit: PATH_MAX,
        password: false,
        salt: 0,
    }
}

/// Step two: the key, pasted once.
fn step_key(
    ui: &mut Ui,
    reading: &Reading,
    settings: &mut Settings,
    tokens: &Tokens,
    presses: &mut Presses,
    kind: Step,
) {
    let stored = reading.key() == KeyState::Stored;
    if kind == Step::Done || stored {
        let label = "Key · done";
        let width = caps(ui, label, tokens.palette.text_subtle).size().x;
        done_row(
            ui,
            tokens,
            "key stored in your system keychain",
            width,
            |ui| done_label(ui, tokens, label),
        );
        ui.add_space(space::S2);
        hint(ui, tokens, KEYCHAIN);
        return;
    }

    step(ui, tokens, "02", kind, |ui| {
        title(ui, tokens, "Get your key");
        body(
            ui,
            tokens,
            "One key, from your own Spotify developer page. It is yours alone — it cannot be shared, and Takofy cannot make one for you.",
        );
        ui.add_space(space::S3);
        let mut submitted = false;
        // Enter sends the key, but a paste is not a keystroke ending in Enter:
        // without a button a pasted key sits in the field and the pairing step
        // stays out of reach.
        let save = ghost_size(ui, "Save the key");
        let stacked = ui.available_width() < extent::PATH_FIELD_MIN_W + save.x + space::S3;
        ui.horizontal(|ui| {
            if text_button(ui, tokens, Control::OpenKeyPage, "Open the key page", true) {
                presses.open = Some(SoloistPage::Key);
            }
            ui.add_space(space::S3);
            let out = field(
                ui,
                tokens,
                Field {
                    control: Control::KeyField,
                    hint: "Paste the key",
                    limit: KEY_MAX,
                    password: true,
                    salt: reading.submit_count,
                },
                ui.available_width(),
                settings.key_field_mut(),
            );
            submitted = out.enter;
        });
        if stacked {
            if ghost(
                ui,
                tokens,
                Control::SaveKey,
                "Save the key",
                !settings.key_field_empty(),
            ) {
                submitted = true;
            }
        } else {
            ui.horizontal(|ui| {
                if ghost(
                    ui,
                    tokens,
                    Control::SaveKey,
                    "Save the key",
                    !settings.key_field_empty(),
                ) {
                    submitted = true;
                }
            });
        }
        if submitted {
            presses.key = true;
        }
        ui.add_space(space::S2);
        hint(ui, tokens, KEYCHAIN);
        if let Some(fault) = reading.fault.filter(is_key_fault) {
            ui.add_space(space::S2);
            spoken_line(ui, setup_failure(fault), tokens.palette.text);
        }
    });
}

/// Whether a setup fault is about the key rather than about the program.
fn is_key_fault(fault: &SetupFault) -> bool {
    matches!(
        fault,
        SetupFault::KeyEmpty | SetupFault::KeychainUnavailable | SetupFault::KeychainFailed
    )
}

/// Step three: the one step that waits on something outside the app.
fn step_pair(ui: &mut Ui, reading: &Reading, tokens: &Tokens, presses: &mut Presses, kind: Step) {
    if kind == Step::Done {
        let label = "Paired · done";
        let width = caps(ui, label, tokens.palette.text_subtle).size().x;
        done_row(ui, tokens, "paired with your phone", width, |ui| {
            done_label(ui, tokens, label)
        });
        return;
    }

    step(ui, tokens, "03", kind, |ui| {
        title(ui, tokens, "Pair it with your phone");
        body(
            ui,
            tokens,
            "Open Spotify on your phone, tap the device list, and pick Takofy. Once is enough — it will remember.",
        );
        ui.add_space(space::S3);
        match reading.pairing {
            Pairing::Waiting => {
                ui.horizontal(|ui| {
                    waiting(ui, tokens);
                    ui.add_space(space::S5);
                    if ghost(ui, tokens, Control::StopPairing, "Stop", true) {
                        presses.stop = true;
                    }
                });
                ui.add_space(space::S3);
                hint(ui, tokens, PHONE_HINT);
            }
            Pairing::Failed(fault) => {
                spoken_line(ui, pair_failure(fault), tokens.palette.text);
                ui.add_space(space::S3);
                if text_button(
                    ui,
                    tokens,
                    Control::StartPairing,
                    "Start pairing",
                    reading.pair_ready,
                ) {
                    presses.pair = true;
                }
            }
            Pairing::Idle => {
                if text_button(
                    ui,
                    tokens,
                    Control::StartPairing,
                    "Start pairing",
                    reading.pair_ready,
                ) {
                    presses.pair = true;
                }
            }
        }
        if reading.paired.is_none() {
            ui.add_space(space::S3);
            hint(ui, tokens, ALREADY_PAIRED);
        }
    });
}

/// The dot and the word that say the step is waiting on the phone.
fn waiting(ui: &mut Ui, tokens: &Tokens) {
    let label = caps(ui, "Waiting for your phone", tokens.palette.text);
    let height = label.size().y.max(extent::WAIT_DOT);
    let width = extent::WAIT_DOT + space::S2 + label.size().x;
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let dot = Rect::from_center_size(
        Pos2::new(rect.left() + extent::WAIT_DOT / 2.0, rect.center().y),
        Vec2::splat(extent::WAIT_DOT),
    );
    ui.painter()
        .rect_filled(dot, radius::PILL, tokens.palette.text);
    paint(
        ui,
        Pos2::new(
            rect.left() + extent::WAIT_DOT + space::S2,
            rect.center().y - label.size().y / 2.0,
        ),
        &label,
        tokens.palette.text,
    );
}

/// The frame every step is done in: the three facts and the one warning.
fn ready(ui: &mut Ui, reading: &Reading, tokens: &Tokens, presses: &mut Presses) {
    let text = program_text(reading);
    let recheck = ghost_size(ui, "Recheck");
    let change = ghost_size(ui, "Change the program");
    done_row(ui, tokens, &text, recheck.x + space::S3 + change.x, |ui| {
        ui.horizontal(|ui| {
            if ghost(ui, tokens, Control::RescanProgram, "Recheck", true) {
                presses.path = true;
            }
            ui.add_space(space::S3);
            // The ready frame has one exit, and the intent behind it is not on
            // the settings state yet.
            let _ = ghost(
                ui,
                tokens,
                Control::ChangeProgram,
                "Change the program",
                false,
            );
        });
    });
    ui.add_space(space::S2);
    hint(ui, tokens, KEYCHAIN);
    if reading.expiry {
        expiry(ui, reading, tokens, presses);
    }
}

/// The expiry block: the day the build stops working, and the way to a newer one.
fn expiry(ui: &mut Ui, reading: &Reading, tokens: &Tokens, presses: &mut Presses) {
    ui.add_space(space::S3);
    if let Some(day) = reading.expires() {
        title(ui, tokens, &expiry_title(day));
    }
    body(ui, tokens, expiry_line());
    ui.add_space(space::S3);
    if text_button(
        ui,
        tokens,
        Control::OpenDownload,
        "Open the download page",
        true,
    ) {
        presses.open = Some(SoloistPage::Download);
    }
}

/// What the block says about the program, whether it was found or not.
fn program_text(reading: &Reading) -> String {
    match reading.program() {
        Some(program) => program_line(program, reading.built(), reading.build()),
        None => not_found_line(reading.looked_in_downloads()).to_owned(),
    }
}
