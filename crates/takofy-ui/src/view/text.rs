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

/// One line, wrapped at `width` and elided with an ellipsis.
///
/// `ui.put` centres the widget it is given, which is wrong for anything that
/// starts at a known left edge, so a row paints its own galleys.
pub fn line(
    ui: &egui::Ui,
    label: &str,
    font: FontId,
    color: Color32,
    spacing: f32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let format = egui::TextFormat {
        font_id: font,
        color,
        extra_letter_spacing: spacing,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::single_section(label.to_owned(), format);
    job.wrap.max_width = width;
    job.wrap.max_rows = 1;
    job.wrap.overflow_character = Some('…');
    ui.painter().layout_job(job)
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

/// A track row's title, in the interface face at 500.
pub fn track_name() -> FontId {
    FontId::new(text::BASE, fonts::sans(Weight::Medium))
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
mod tests {
    use super::*;

    #[test]
    fn tracking_scales_with_the_size() {
        assert!((tracking_pts(text::XXS, tracking::CAPS) - 1.1).abs() < 1e-6);
        assert_eq!(
            tracking_pts(20.0, tracking::WIDE),
            2.0 * tracking_pts(10.0, tracking::WIDE)
        );
    }

    #[test]
    fn caps_upper_cases_including_non_ascii() {
        let color = Tokens::DARK.palette.text;
        assert_eq!(caps("straße", tracking::WIDE, color).text(), "STRASSE");
        assert_eq!(caps("sō", tracking::WIDE, color).text(), "SŌ");
    }
}
