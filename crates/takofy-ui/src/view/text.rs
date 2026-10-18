//! Text helpers shared by the views.
//!
//! A recipe names a role in the type system — a wordmark, a library name, a
//! readout — so a view asks for the role and never assembles a size, a weight
//! and a tracking value by hand.

use egui::{Color32, FontId, RichText};

use crate::theme::{MonoWeight, Tokens, Weight, fonts, text, tracking};

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
