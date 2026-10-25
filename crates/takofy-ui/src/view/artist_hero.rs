//! The artist page's hero: the name on the colour its portrait gave.
//!
//! The hero is the band's words on the band's wash. The square plate and the
//! shelf of release covers are still to come, so the portrait's own mean is what
//! colours it. There is no byline, no verified badge and no monthly listeners,
//! because the wire carries none of them.
//!
//! The band is all this view draws. Its action row is the shared one, which is
//! the only way the row can pin while the discography scrolls under it; the page
//! owns the presses that row reports.

use egui::{Color32, Sense, Ui, vec2};

use crate::layout::hero::artist_title_size;
use crate::theme::{MonoWeight, Tokens, Weight, fonts, hero, space, text, tracking};

use super::{hero as band, text as recipe};

/// What the artist hero says.
pub struct ArtistHero<'a> {
    pub name: &'a str,
    /// The colour derived from the portrait, when its pixels are held.
    pub tint: Option<Color32>,
}

/// Draws the hero.
///
/// The hero takes the width it is given and starts at the cursor, so the page
/// decides where it sits; the painter decides only how it looks.
pub fn show(ui: &mut Ui, artist: &ArtistHero<'_>, tokens: &Tokens) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, hero::ARTIST_MIN_H), Sense::hover());
    band::wash(ui, rect, artist.tint, tokens);
    words(ui, rect, artist.name, tokens);
}

/// The eyebrow and the name, stacked up from the hero's floor.
///
/// The column is built upward so a title that grows with the window never moves
/// where the hero ends.
fn words(ui: &Ui, rect: egui::Rect, name: &str, tokens: &Tokens) {
    let left = rect.left() + space::GUTTER;
    let width = (rect.width() - 2.0 * space::GUTTER).max(0.0);
    if width <= 0.0 {
        return;
    }
    let mut bottom = rect.bottom() - space::S5;

    let size = artist_title_size(ui.ctx().content_rect().width());
    let title = recipe::line(
        ui,
        name,
        egui::FontId::new(size, fonts::sans(Weight::Black)),
        tokens.palette.text,
        recipe::tracking_pts(size, tracking::TIGHT),
        width,
    );
    bottom -= title.size().y;
    ui.painter()
        .galley(egui::Pos2::new(left, bottom), title, tokens.palette.text);
    bottom -= space::S2;

    let eyebrow = recipe::spaced(
        ui,
        &"Artist".to_uppercase(),
        egui::FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
        tokens.palette.text_muted,
        recipe::tracking_pts(text::XXS, tracking::CAPS),
    );
    bottom -= eyebrow.size().y;
    ui.painter().galley(
        egui::Pos2::new(left, bottom),
        eyebrow,
        tokens.palette.text_muted,
    );
}

#[cfg(test)]
mod tests {
    use egui::{Context, Pos2, RawInput, Rect, Vec2};

    use super::*;
    use crate::theme::{Tokens, fonts, style};

    fn context() -> Context {
        let tokens = Tokens::DARK;
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        ctx
    }

    fn input(size: Vec2) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            ..Default::default()
        }
    }

    fn hero<'a>(name: &'a str, tint: Option<Color32>) -> ArtistHero<'a> {
        ArtistHero { name, tint }
    }

    /// The rects the frame filled with `color`.
    fn fills(output: &egui::FullOutput, color: Color32) -> Vec<Rect> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape) if shape.fill == color => Some(shape.rect),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_hero_is_only_the_band() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut top = 0.0;
        let mut settled = 0.0;
        let mut spacing = 0.0;
        let mut output = ctx.run_ui(input(Vec2::new(900.0, 800.0)), |ui| {
            top = ui.cursor().min.y;
            spacing = ui.spacing().item_spacing.y;
            show(ui, &hero("Vela Moreno", None), &tokens);
            settled = ui.cursor().min.y;
        });
        output.textures_delta.clear();

        assert_eq!(
            settled - top,
            hero::ARTIST_MIN_H + spacing,
            "the hero is the band and its words, and nothing under them"
        );

        let accent = output
            .shapes
            .iter()
            .filter(|clipped| {
                matches!(&clipped.shape, egui::epaint::Shape::Rect(shape) if shape.fill == tokens.palette.accent)
            })
            .count();
        assert_eq!(accent, 0, "the hero draws no play tile of its own");
        let words: Vec<&str> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect();
        assert!(
            !words.contains(&"Follow"),
            "the hero draws no control of its own: {words:?}"
        );
        assert!(
            words.contains(&"Vela Moreno"),
            "the band still carries the name: {words:?}"
        );
    }

    #[test]
    fn a_tinted_hero_fills_the_band() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let tint = crate::theme::tint_color([116, 27, 27]);
        let mut output = ctx.run_ui(input(Vec2::new(900.0, 800.0)), |ui| {
            show(ui, &hero("Vela Moreno", Some(tint)), &tokens);
        });
        output.textures_delta.clear();

        assert_eq!(
            fills(&output, tint),
            vec![Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(900.0, hero::ARTIST_MIN_H)
            )],
            "the tint covers the band and nothing else"
        );
        assert!(
            fills(&output, tokens.palette.hero_wash).is_empty(),
            "a tinted hero does not also paint the fallback"
        );
    }

    #[test]
    fn an_untinted_hero_fills_the_wash() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut output = ctx.run_ui(input(Vec2::new(900.0, 800.0)), |ui| {
            show(ui, &hero("Vela Moreno", None), &tokens);
        });
        output.textures_delta.clear();

        let [_, _, _, alpha] = tokens.palette.hero_wash.to_srgba_unmultiplied();
        assert_eq!(alpha, 255, "the fallback is opaque");
        assert_eq!(
            fills(&output, tokens.palette.hero_wash),
            vec![Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(900.0, hero::ARTIST_MIN_H)
            )]
        );
    }

    #[test]
    fn a_tint_does_not_change_the_band_height() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let tint = crate::theme::tint_color([60, 60, 14]);

        let height = |tint: Option<Color32>| {
            let mut top = 0.0;
            let mut settled = 0.0;
            let mut spacing = 0.0;
            let mut output = ctx.run_ui(input(Vec2::new(900.0, 800.0)), |ui| {
                top = ui.cursor().min.y;
                spacing = ui.spacing().item_spacing.y;
                show(ui, &hero("Vela Moreno", tint), &tokens);
                settled = ui.cursor().min.y;
            });
            output.textures_delta.clear();
            settled - top - spacing
        };

        assert_eq!(height(None), hero::ARTIST_MIN_H);
        assert_eq!(height(Some(tint)), hero::ARTIST_MIN_H);
    }
}
