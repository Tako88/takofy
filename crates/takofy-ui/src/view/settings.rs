//! The settings page: the sound engine, the Soloist setup and autoplay.
//!
//! The page draws what the settings state already holds and leaves every press
//! on that state's outbox, which the shell drains once a frame. It returns no
//! command of its own, because one frame may carry several setup presses and the
//! shell takes at most one.

use std::sync::Arc;

use egui::{
    Align2, Color32, FontId, Galley, Margin, Pos2, Rect, Role, Sense, Stroke, StrokeKind, Ui, Vec2,
    vec2,
};

use crate::availability::Control;
use crate::library::Library;
use crate::message::{Command, Facts, Freshness, KeyState, SetupFault, SoloistPage};
use crate::prefs::SoundChoice;
use crate::queue::Queue;
use crate::settings::{Pairing, RowStatus, Settings, Step};
use crate::theme::{
    MonoWeight, Tokens, Weight, extent, fonts, radius, space, stroke, text, tracking,
};
use crate::view::{controls, name_field, page_head, section, text as recipe};

mod engine;
mod panel;
mod playback;

#[cfg(test)]
mod tests;

/// Draws the page. Nothing here returns a command: the presses land on the state
/// and the shell drains its outbox.
pub(crate) fn settings_view(
    ui: &mut Ui,
    library: &mut Library,
    queue: &mut Queue,
    tokens: &Tokens,
) -> Option<Command> {
    let reading = Reading::of(&library.settings);
    let mut presses = Presses::default();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            crate::view::inner(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                page_head::show(ui, "Settings", "Sound · playback", tokens);
                engine::sound(ui, &reading, tokens, &mut presses);
                if reading.panel {
                    panel::show(ui, &reading, &mut library.settings, tokens, &mut presses);
                }
                section::divide(ui, tokens);
                playback::show(ui, queue.autoplay(), tokens, &mut presses);
            });
        });

    let pressed = presses.any();
    presses.apply(library, queue);
    if pressed {
        // The state now holds something the frame after this one draws.
        ui.ctx().request_repaint();
    }
    None
}

/// What the page reads from the settings state, copied so the fields it draws
/// can borrow that state mutably while it is drawn.
struct Reading {
    chosen: SoundChoice,
    running: SoundChoice,
    name: String,
    status: RowStatus,
    panel: bool,
    steps: [Step; 3],
    pairing: Pairing,
    pair_ready: bool,
    paired: Option<bool>,
    fault: Option<SetupFault>,
    expiry: bool,
    facts: Option<Facts>,
    path: Option<String>,
    submit_count: u32,
}

impl Reading {
    fn of(settings: &Settings) -> Self {
        Self {
            chosen: settings.chosen(),
            running: settings.running(),
            name: settings.name().to_owned(),
            status: settings.status(),
            panel: settings.panel(),
            steps: settings.steps(),
            pairing: settings.pairing(),
            pair_ready: settings.pair_ready(),
            paired: settings.paired(),
            fault: settings.fault(),
            expiry: settings.expiry_block(),
            facts: settings.facts().cloned(),
            path: settings.path().map(str::to_owned),
            submit_count: settings.submit_count(),
        }
    }

    /// The program the setup block names: what the scan found, or the pin it
    /// could not confirm.
    fn program(&self) -> Option<&str> {
        self.facts
            .as_ref()
            .and_then(|facts| facts.program.as_deref())
            .or(self.path.as_deref())
    }

    fn build(&self) -> Freshness {
        self.facts
            .as_ref()
            .map_or(Freshness::Unknown, |facts| facts.build)
    }

    fn built(&self) -> Option<takofy_model::home::Day> {
        self.facts.as_ref().and_then(|facts| facts.built)
    }

    fn expires(&self) -> Option<takofy_model::home::Day> {
        self.facts.as_ref().and_then(|facts| facts.expires)
    }

    fn key(&self) -> KeyState {
        self.facts
            .as_ref()
            .map_or(KeyState::Absent, |facts| facts.key)
    }

    fn looked_in_downloads(&self) -> bool {
        self.facts
            .as_ref()
            .is_some_and(|facts| facts.looked_in_downloads)
    }

    fn all_done(&self) -> bool {
        self.steps == [Step::Done; 3]
    }
}

/// What the page's controls asked for this frame, collected until it ends.
///
/// The presses are held rather than acted on where they are drawn, because
/// drawing borrows the settings state the presses change.
#[derive(Debug, Default)]
struct Presses {
    /// The engine a row was pressed for.
    engine: Option<SoundChoice>,
    /// The program field was submitted, or Look again was pressed.
    path: bool,
    /// The key field was submitted.
    key: bool,
    /// The pairing step was started.
    pair: bool,
    /// The pairing step was stopped.
    stop: bool,
    /// A setup page was asked for in the browser.
    open: Option<SoloistPage>,
    /// What a press asked the autoplay switch to be.
    autoplay: Option<bool>,
    /// The file a drag-and-drop handed the window.
    dropped: Option<String>,
}

impl Presses {
    fn any(&self) -> bool {
        self.engine.is_some()
            || self.path
            || self.key
            || self.pair
            || self.stop
            || self.open.is_some()
            || self.autoplay.is_some()
            || self.dropped.is_some()
    }

    fn apply(self, library: &mut Library, queue: &mut Queue) {
        if let Some(choice) = self.engine {
            library.settings.choose(choice);
        }
        if self.path {
            library.settings.submit_path();
        }
        if self.key {
            library.settings.submit_key();
        }
        if self.pair {
            library.settings.start_pairing();
        }
        if self.stop {
            library.settings.stop_pairing();
        }
        if let Some(page) = self.open {
            library.settings.open(page);
        }
        if let Some(path) = self.dropped {
            library.settings.drop_file(path);
        }
        if let Some(on) = self.autoplay {
            queue.set_autoplay(on);
        }
    }
}

/// The file a drag-and-drop brought this frame.
///
/// Nothing else in the window takes a dropped file, so the page keeps the first
/// one it is handed rather than letting it fall through.
fn dropped(ui: &Ui) -> Option<String> {
    ui.input(|input| {
        input
            .raw
            .dropped_files
            .first()
            .map(|file| file.path().to_string_lossy().into_owned())
    })
}

/// The measure some characters of a face take, which is how the reference writes
/// the length of a line.
fn measure(ui: &Ui, font: &FontId, chars: f32) -> f32 {
    let width = ui.ctx().fonts_mut(|fonts| fonts.glyph_width(font, '0'));
    width * chars
}

/// The type the engine rows and step titles are set in.
fn name_face() -> FontId {
    FontId::new(text::MD, fonts::sans(Weight::Semibold))
}

fn title_face() -> FontId {
    FontId::new(text::BASE, fonts::sans(Weight::Semibold))
}

fn body_face() -> FontId {
    FontId::new(text::SM, fonts::sans(Weight::Regular))
}

fn mono_face() -> FontId {
    FontId::new(text::SM, fonts::mono(MonoWeight::Regular))
}

fn note_face() -> FontId {
    FontId::new(text::XXS, fonts::mono(MonoWeight::Regular))
}

fn ghost_face() -> FontId {
    FontId::new(text::XXS, fonts::mono(MonoWeight::Medium))
}

/// A caps line: a status, a step number, the waiting word.
fn caps(ui: &Ui, label: &str, color: Color32) -> Arc<Galley> {
    recipe::spaced(
        ui,
        &label.to_uppercase(),
        ghost_face(),
        color,
        recipe::tracking_pts(text::XXS, tracking::CAPS),
    )
}

/// One line of a face, elided rather than wrapped.
fn line(
    ui: &Ui,
    label: &str,
    font: FontId,
    color: Color32,
    tracking_em: f32,
    width: f32,
) -> Arc<Galley> {
    let spacing = recipe::tracking_pts(font.size, tracking_em);
    recipe::line(ui, label, font, color, spacing, width)
}

/// A wrapped block of a face.
fn block(
    ui: &Ui,
    label: &str,
    font: FontId,
    color: Color32,
    width: f32,
    rows: usize,
) -> Arc<Galley> {
    recipe::clamped(ui, label, font, color, 0.0, width, rows)
}

/// A wrapped block of several faces, for the one line that sets the device name
/// apart from the sentence about it.
fn held(ui: &Ui, parts: &[(&str, FontId)], color: Color32, width: f32, rows: usize) -> Arc<Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = width;
    job.wrap.max_rows = rows;
    job.wrap.overflow_character = Some('…');
    for (text, font) in parts {
        job.append(
            text,
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color,
                ..Default::default()
            },
        );
    }
    ui.painter().layout_job(job)
}

fn paint(ui: &Ui, pos: Pos2, galley: &Arc<Galley>, color: Color32) {
    ui.painter().galley(pos, galley.clone(), color);
}

/// Reserves the room a laid-out line needs and paints it there.
fn word(ui: &mut Ui, galley: Arc<Galley>, color: Color32) -> Rect {
    let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
    paint(ui, rect.min, &galley, color);
    rect
}

/// The room a ghost button's word takes.
fn ghost_size(ui: &Ui, label: &str) -> Vec2 {
    let galley = recipe::spaced(
        ui,
        &label.to_uppercase(),
        ghost_face(),
        Color32::PLACEHOLDER,
        recipe::tracking_pts(text::XXS, tracking::WIDE),
    );
    vec2(galley.size().x + 2.0 * space::S2, extent::BUTTON)
}

/// A word with no frame: the reference's ghost button, in the caps the page sets
/// its small labels in.
fn ghost(ui: &mut Ui, tokens: &Tokens, control: Control, label: &str, live: bool) -> bool {
    let (rect, _) = ui.allocate_exact_size(ghost_size(ui, label), Sense::hover());
    let slot = controls::slot(ui, control, Role::Button, rect, label, tokens, live);
    let color = if slot.live {
        if slot.hover > 0.0 {
            tokens.palette.text
        } else {
            tokens.palette.text_muted
        }
    } else {
        tokens.palette.text_disabled
    };
    let galley = recipe::spaced(
        ui,
        &label.to_uppercase(),
        ghost_face(),
        color,
        recipe::tracking_pts(text::XXS, tracking::WIDE),
    );
    paint(
        ui,
        Pos2::new(
            rect.left() + space::S2,
            rect.center().y - galley.size().y / 2.0,
        ),
        &galley,
        color,
    );
    slot.response.clicked()
}

/// A bordered button carrying a word, sized and placed by the caller's layout.
fn text_button(ui: &mut Ui, tokens: &Tokens, control: Control, label: &str, live: bool) -> bool {
    let (rect, _) = ui.allocate_exact_size(
        controls::text_button_size(ui, tokens, label),
        Sense::hover(),
    );
    controls::text_button(ui, tokens, control, rect, label, live)
        .response
        .clicked()
}

/// The longest path the field takes. A longer one is not a path a filesystem
/// names, and its line would read as a wall.
const PATH_MAX: usize = 1024;

/// The longest key the field takes. It is generous on purpose: a truncated key
/// would be stored as a broken one.
const KEY_MAX: usize = 4096;

/// One of the page's text fields.
struct Field<'a> {
    control: Control,
    hint: &'a str,
    limit: usize,
    password: bool,
    /// Salted into the widget's id, so a submit discards the state egui kept.
    salt: u32,
}

/// A sunken field the page takes a line of text in.
fn field(
    ui: &mut Ui,
    tokens: &Tokens,
    spec: Field<'_>,
    width: f32,
    text: &mut String,
) -> name_field::Out {
    let (rect, _) = ui.allocate_exact_size(vec2(width, extent::BUTTON), Sense::hover());
    ui.painter()
        .rect_filled(rect, radius::SM, tokens.palette.surface_sunken);
    let id = ui.scope_id().with((spec.control, spec.salt));
    let out = name_field::edit(
        ui,
        name_field::Spec {
            id,
            rect,
            text,
            writing: false,
            wants_focus: false,
            font: mono_face(),
            hint: spec.hint,
            inset: Margin {
                left: space::S3 as i8,
                right: space::S3 as i8,
                top: 0,
                bottom: 0,
            },
            text_color: tokens.palette.text,
            weak_color: tokens.palette.text_subtle,
            cursor: Stroke::new(stroke::HAIRLINE, tokens.palette.text),
            align: Align2::LEFT_CENTER,
            limit: spec.limit,
            password: spec.password,
        },
    );
    let border = if out.focused {
        tokens.palette.text
    } else {
        tokens.palette.border_strong
    };
    ui.painter().rect_stroke(
        rect,
        radius::SM,
        Stroke::new(stroke::HAIRLINE, border),
        StrokeKind::Inside,
    );
    out
}

/// The reference's found line: a fact the app established, drawn as a status
/// rather than as something to fill in.
fn found(ui: &mut Ui, tokens: &Tokens, text: &str, width: f32) {
    let galley = line(
        ui,
        text,
        mono_face(),
        tokens.palette.text,
        0.0,
        (width - 2.0 * space::S3).max(0.0),
    );
    let height = galley.size().y + 2.0 * space::S2;
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    ui.painter()
        .rect_filled(rect, radius::SM, tokens.palette.surface_sunken);
    ui.painter().rect_stroke(
        rect,
        radius::SM,
        Stroke::new(stroke::HAIRLINE, tokens.palette.border),
        StrokeKind::Inside,
    );
    paint(
        ui,
        Pos2::new(
            rect.left() + space::S3,
            rect.center().y - galley.size().y / 2.0,
        ),
        &galley,
        tokens.palette.text,
    );
}

/// A finished step: its fact on the left, and what can still be done about it on
/// the right.
fn done_row(
    ui: &mut Ui,
    tokens: &Tokens,
    text: &str,
    trailing_w: f32,
    trailing: impl FnOnce(&mut Ui),
) {
    let width = ui.available_width();
    ui.horizontal(|ui| {
        let found_w = (width - trailing_w - space::S3).max(0.0);
        found(ui, tokens, text, found_w);
        ui.add_space(space::S3);
        ui.vertical(|ui| trailing(ui));
    });
}

/// A step title.
fn title(ui: &mut Ui, tokens: &Tokens, text: &str) {
    let galley = line(
        ui,
        text,
        title_face(),
        tokens.palette.text,
        0.0,
        ui.available_width(),
    );
    word(ui, galley, tokens.palette.text);
}

/// A step's sentence, clamped to the settings measure.
fn body(ui: &mut Ui, tokens: &Tokens, text: &str) {
    let font = body_face();
    let width = ui
        .available_width()
        .min(measure(ui, &font, extent::SETTINGS_TEXT_CH));
    let galley = block(ui, text, font, tokens.palette.text_muted, width, 4);
    word(ui, galley, tokens.palette.text_muted);
}

/// A line that says what happened, in the ink of something that must be read.
fn spoken_line(ui: &mut Ui, text: &str, color: Color32) {
    let font = body_face();
    let width = ui
        .available_width()
        .min(measure(ui, &font, extent::SETTINGS_TEXT_CH));
    let galley = block(ui, text, font, color, width, 3);
    word(ui, galley, color);
}

/// The small mono line under a step, wrapped at a wider measure because it is
/// set smaller.
fn hint(ui: &mut Ui, tokens: &Tokens, text: &str) {
    let font = note_face();
    let width = ui
        .available_width()
        .min(measure(ui, &font, extent::SETTINGS_TEXT_CH * 1.4));
    let galley = block(ui, text, font, tokens.palette.text_subtle, width, 4);
    word(ui, galley, tokens.palette.text_subtle);
}

/// The caps label a finished step ends with.
fn done_label(ui: &mut Ui, tokens: &Tokens, text: &str) {
    let galley = caps(ui, text, tokens.palette.text_subtle);
    word(ui, galley, tokens.palette.text_subtle);
}
