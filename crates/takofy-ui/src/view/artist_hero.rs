//! The artist page's hero: a photograph with the name standing on it.
//!
//! The reference's artist hero is not a playlist band with another word in it.
//! The picture is behind the words rather than beside them, and a gradient scrim
//! darkens it so the name reads; there is no byline, no verified badge and no
//! monthly listeners, because the wire carries none. That is why this is its own
//! view and the playlist's band is untouched.
//!
//! The band is all this view draws. Its action row is the shared one, which is
//! the only way the row can pin while the discography scrolls under it; the page
//! owns the presses that row reports.

use egui::{Pos2, Rect, Sense, Shape, Ui, vec2};

use crate::cover::Cover;
use crate::layout::hero::artist_title_size;
use crate::theme::{MonoWeight, Tokens, Weight, fonts, hero, space, text, tracking};

use super::{art, text as recipe};

/// What the artist hero says.
pub struct ArtistHero<'a> {
    pub name: &'a str,
    /// The portrait as the art pipeline resolves it.
    pub portrait: Cover,
}

/// Draws the hero.
///
/// The hero takes the width it is given and starts at the cursor, so the page
/// decides where it sits; the painter decides only how it looks.
pub fn show(ui: &mut Ui, hero: &ArtistHero<'_>, tokens: &Tokens) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, hero::ARTIST_MIN_H), Sense::hover());
    art::fill(ui.painter(), rect, hero.portrait, tokens);
    scrim(ui, rect, tokens);
    words(ui, rect, hero.name, tokens);
}

/// The gradient over the photograph: the reference's three stops.
///
/// Two palette fields carry black at 32% and 58%, and the last stop is the
/// surface the page underneath is drawn on, so the hero fades into its own page
/// rather than ending on a hard edge.
fn scrim(ui: &Ui, rect: Rect, tokens: &Tokens) {
    let mut mesh = egui::epaint::Mesh::default();
    for (fraction, color) in [
        (0.0, tokens.palette.hero_scrim_top),
        (hero::SCRIM_MID_STOP, tokens.palette.hero_scrim_mid),
        (1.0, tokens.palette.surface),
    ] {
        let y = rect.top() + rect.height() * fraction;
        mesh.colored_vertex(Pos2::new(rect.left(), y), color);
        mesh.colored_vertex(Pos2::new(rect.right(), y), color);
    }
    for row in 0..2u32 {
        let base = row * 2;
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base + 1, base + 2, base + 3);
    }
    ui.painter().add(Shape::mesh(mesh));
}

/// The eyebrow and the name, stacked up from the hero's floor.
///
/// The column is built upward so a title that grows with the window never moves
/// where the hero ends. There is no verified badge and no monthly listeners: the
/// wire carries neither.
fn words(ui: &Ui, rect: Rect, name: &str, tokens: &Tokens) {
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
        .galley(Pos2::new(left, bottom), title, tokens.palette.text);
    bottom -= space::S2;

    let eyebrow = recipe::spaced(
        ui,
        &"Artist".to_uppercase(),
        egui::FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
        tokens.palette.text_muted,
        recipe::tracking_pts(text::XXS, tracking::CAPS),
    );
    bottom -= eyebrow.size().y;
    ui.painter()
        .galley(Pos2::new(left, bottom), eyebrow, tokens.palette.text_muted);
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

    #[test]
    fn the_hero_is_only_the_band() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut top = 0.0;
        let mut settled = 0.0;
        let mut spacing = 0.0;
        let mut output = ctx.run_ui(input(Vec2::new(900.0, 800.0)), |ui| {
            let hero = ArtistHero {
                name: "Vela Moreno",
                portrait: Cover::Missing,
            };
            top = ui.cursor().min.y;
            spacing = ui.spacing().item_spacing.y;
            show(ui, &hero, &tokens);
            settled = ui.cursor().min.y;
        });
        output.textures_delta.clear();

        assert_eq!(
            settled - top,
            hero::ARTIST_MIN_H + spacing,
            "the hero is the photograph and its words, and nothing under them"
        );

        // The action row was the band's own: its play tile was the page's one
        // accent fill and its word the band's own text control.
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
}
