//! Text helpers shared by the views.
//!
//! A recipe names a role in the type system — a wordmark, a library name, a
//! readout — so a view asks for the role and never assembles a size, a weight
//! and a tracking value by hand.

use egui::{Color32, FontId, RichText};

use crate::theme::{MonoWeight, Tokens, Weight, fonts, text, tracking};

/// Lays out one line with the reference's letter spacing, ready to paint.
///
/// A caps label is the reason this exists: `Painter::text` cannot carry spacing,
/// and the reference tracks its small mono text apart.
pub fn spaced(
    ui: &egui::Ui,
    label: &str,
    font: FontId,
    color: Color32,
    spacing: f32,
) -> std::sync::Arc<egui::Galley> {
    let format = egui::TextFormat {
        font_id: font,
        color,
        extra_letter_spacing: spacing,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::single_section(label.to_owned(), format);
    job.wrap.max_rows = 1;
    ui.painter().layout_job(job)
}

/// Text wrapped at `width`, no taller than `max_rows`, and elided with an
/// ellipsis when it does not fit.
///
/// `ui.put` centres the widget it is given, which is wrong for anything that
/// starts at a known left edge, so a view paints its own galleys.
pub fn clamped(
    ui: &egui::Ui,
    label: &str,
    font: FontId,
    color: Color32,
    spacing: f32,
    width: f32,
    max_rows: usize,
) -> std::sync::Arc<egui::Galley> {
    let format = egui::TextFormat {
        font_id: font,
        color,
        extra_letter_spacing: spacing,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::single_section(label.to_owned(), format);
    job.wrap.max_width = width;
    job.wrap.max_rows = max_rows;
    job.wrap.overflow_character = Some('…');
    ui.painter().layout_job(job)
}

/// One line, wrapped at `width` and elided with an ellipsis.
pub fn line(
    ui: &egui::Ui,
    label: &str,
    font: FontId,
    color: Color32,
    spacing: f32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    clamped(ui, label, font, color, spacing, width, 1)
}

/// The band's small mono line: the description and the meta byline.
///
/// The reference draws both at its `xs` size in mono, tracked apart by a
/// fractional em, so they read as annotation under the title.
pub fn hero_description() -> FontId {
    FontId::new(text::XS, fonts::mono(MonoWeight::Regular))
}

/// Secondary text, so chrome does not read as content.
pub fn muted(text: &str, tokens: &Tokens) -> RichText {
    RichText::new(text).color(tokens.palette.text_muted)
}

/// Letter spacing in points for a size and an em value.
///
/// egui wants spacing in points, the reference writes it in em; this is the one
/// conversion between them.
pub fn tracking_pts(size: f32, em: f32) -> f32 {
    size * em
}

/// A monospace, all-caps label: the reference's caps tier.
pub fn caps(label: &str, tracking_em: f32, color: Color32) -> RichText {
    RichText::new(label.to_uppercase())
        .font(FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)))
        .extra_letter_spacing(tracking_pts(text::XXS, tracking_em))
        .color(color)
}

/// The wordmark beside the logo.
pub fn wordmark(label: &str, tokens: &Tokens) -> RichText {
    RichText::new(label)
        .font(FontId::new(text::MD, fonts::sans(Weight::Bold)))
        .extra_letter_spacing(tracking_pts(text::MD, tracking::TIGHT))
        .color(tokens.palette.text)
}

/// A library row's name.
pub fn library_name(label: &str, tokens: &Tokens) -> RichText {
    RichText::new(label)
        .font(FontId::new(text::SM, fonts::sans(Weight::Medium)))
        .color(tokens.palette.text)
}

/// A library row's kind line, below the name.
pub fn library_kind(label: &str, tokens: &Tokens) -> RichText {
    RichText::new(label)
        .font(FontId::new(text::XXS, fonts::mono(MonoWeight::Regular)))
        .extra_letter_spacing(tracking_pts(text::XXS, tracking::SLIGHT))
        .color(tokens.palette.text_subtle)
}

/// A card's name, and the heading of anything that names a thing in a grid.
pub fn card_name() -> FontId {
    FontId::new(text::SM, fonts::sans(Weight::Semibold))
}

/// What a card is: a playlist, an album. Monospace, so a column of them reads as a
/// list of labels rather than as a sentence.
pub fn card_kind() -> FontId {
    FontId::new(text::XS, fonts::mono(MonoWeight::Regular))
}

/// A lyric line: the interface face at the size the pane reads at.
pub fn lyric_line() -> FontId {
    FontId::new(text::BASE, fonts::sans(Weight::Medium))
}

/// A track row's title, in the interface face at 500.
pub fn track_name() -> FontId {
    FontId::new(text::BASE, fonts::sans(Weight::Medium))
}

/// A queue row's name: a step below a track row's, because the panel's rows are
/// denser and the reference sets them at `--text-sm`.
pub fn queue_name() -> FontId {
    FontId::new(text::SM, fonts::sans(Weight::Medium))
}

/// The Now Playing panel's title, the largest interface type the panel carries.
pub fn panel_title() -> FontId {
    FontId::new(text::MD, fonts::sans(Weight::Bold))
}

/// A queue row's artist line, at the reference's `--text-2xs`.
pub fn queue_artist() -> FontId {
    FontId::new(text::XXS, fonts::sans(Weight::Regular))
}

/// An episode row's title, at the reference's `--text-md` in 600.
pub fn episode_title() -> FontId {
    FontId::new(text::MD, fonts::sans(Weight::Semibold))
}

/// The description under an episode's title: body text, one step under the name.
pub fn episode_text() -> FontId {
    FontId::new(text::SM, fonts::sans(Weight::Regular))
}

/// A caps label laid out for the painter: the mono tier the reference tracks
/// apart. The panel's head and the queue's caption are the same recipe.
pub fn caption(ui: &egui::Ui, label: &str, tokens: &Tokens) -> std::sync::Arc<egui::Galley> {
    spaced(
        ui,
        &label.to_uppercase(),
        FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
        tokens.palette.text_subtle,
        tracking_pts(text::XXS, tracking::CAPS),
    )
}

/// A track row's artist and album, which sit under the title and beside it.
pub fn track_secondary() -> FontId {
    FontId::new(text::SM, fonts::sans(Weight::Regular))
}

/// A track row's numbers: the index, the date added, the duration.
///
/// Monospace, so the digits line up down the column and a duration reads as a
/// quantity rather than as prose.
pub fn track_mono(size: f32) -> FontId {
    FontId::new(size, fonts::mono(MonoWeight::Regular))
}

/// A page head's title: the reference's `--text-2xl` at 800, tracked tight.
///
/// The hero's title is its own recipe because its size follows the window.
pub fn page_title() -> FontId {
    FontId::new(text::XXL, fonts::sans(Weight::Black))
}

/// The page head's note, laid out for the painter: a small mono line tracked
/// wide and upper-cased, the reference's `.page-head__note`.
pub fn page_note_galley(
    ui: &egui::Ui,
    label: &str,
    tokens: &Tokens,
) -> std::sync::Arc<egui::Galley> {
    spaced(
        ui,
        &label.to_uppercase(),
        FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
        tokens.palette.text_subtle,
        tracking_pts(text::XXS, tracking::WIDE),
    )
}

/// The sign-in card's heading: the largest type in the window after the hero's,
/// and the only place the interface face is set this heavy at this size.
pub fn card_heading() -> FontId {
    FontId::new(text::XL, fonts::sans(Weight::Bold))
}

/// A card's explanatory line.
pub fn card_body() -> FontId {
    FontId::new(text::SM, fonts::sans(Weight::Regular))
}

/// A playbar readout: the position, its end, the state, the artist line.
pub fn readout(label: &str, tokens: &Tokens) -> RichText {
    RichText::new(label)
        .font(FontId::new(text::XXS, fonts::mono(MonoWeight::Regular)))
        .extra_letter_spacing(tracking_pts(text::XXS, tracking::SLIGHT))
        .color(tokens.palette.text_muted)
}

#[cfg(test)]
mod tests;
