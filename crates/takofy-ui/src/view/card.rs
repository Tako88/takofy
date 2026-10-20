//! One card in the library grid: a cover, a name and what the thing is.
//!
//! The card is one click target, the way the reference's stretched link is. It
//! raises itself off the panel and reveals an accent play tile in its corner when
//! the pointer is on it or a keyboard reaches it. The tile cannot act yet — playing
//! from a card needs that playlist's tracks, and nothing loads them from here — so
//! it is drawn waiting, and a click on it does not fall through to the card either.

use egui::{FontId, Pos2, Rect, Sense, Ui, Vec2};

use crate::availability::Control;
use crate::cover::Cover;
use crate::theme::{Tokens, ease, extent, motion, radius, space, stroke, text, tracking};

use super::{art, controls, text as recipe};

/// What one card shows.
pub struct Card<'a> {
    /// The card's uri. It keys the card and the control inside it.
    pub uri: &'a str,
    pub name: &'a str,
    /// The kind of thing this is, in the reference's one-word form.
    pub kind: &'a str,
    /// The card's cover, as the art pipeline resolves it.
    pub cover: Cover,
}

/// The height a card takes beyond its square art: its two gaps and its two lines.
///
/// The art is square and takes the card's width less its padding, so a card's
/// height is its width plus those gaps and lines — the padding cancels against the
/// width the art gives up.
///
/// The grid is told this rather than working it out: the grid lays out boxes, and
/// the card decides what they hold.
pub fn extra_height(ui: &Ui) -> f32 {
    2.0 * space::S2 + line_height(ui, &recipe::card_name()) + line_height(ui, &recipe::card_kind())
}

/// Draws one card into `rect`, and reports a click on it.
pub fn show(ui: &mut Ui, rect: Rect, card: &Card<'_>, tokens: &Tokens) -> bool {
    let key = ("library-card", card.uri);
    let response = ui.interact(rect, ui.scope_id().with(key), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, spoken(card)));
    let revealed = response.hovered() || response.has_focus();
    let reveal = ui.ctx().animate_bool_with_time_and_easing(
        ui.scope_id().with(key),
        revealed,
        motion::BASE,
        ease::out,
    );

    // The card may have no fill of its own — it is the panel until the pointer
    // raises it, which is what makes a grid of them read as one surface.
    if reveal > 0.0 {
        ui.painter().rect_filled(
            rect,
            radius::MD,
            tokens.palette.surface_raised.gamma_multiply(reveal),
        );
        ui.painter().rect_stroke(
            rect,
            radius::MD,
            egui::Stroke::new(
                stroke::HAIRLINE,
                tokens.palette.border.gamma_multiply(reveal),
            ),
            egui::StrokeKind::Inside,
        );
    }
    if response.has_focus() {
        controls::focus_ring(ui, rect, tokens);
    }

    let inner = rect.shrink(space::S2);
    let art = Rect::from_min_size(inner.min, Vec2::splat(inner.width()));
    art::tile(
        ui.painter(),
        art,
        radius::SM,
        tokens.palette.border,
        card.cover,
        tokens,
    );

    let mut top = art.bottom() + space::S2;
    let name = recipe::line(
        ui,
        card.name,
        recipe::card_name(),
        tokens.palette.text,
        0.0,
        inner.width(),
    );
    ui.painter()
        .galley(Pos2::new(inner.left(), top), name, tokens.palette.text);
    top += line_height(ui, &recipe::card_name()) + space::S2;
    let kind = recipe::line(
        ui,
        card.kind,
        recipe::card_kind(),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XS, tracking::SLIGHT),
        inner.width(),
    );
    ui.painter().galley(
        Pos2::new(inner.left(), top),
        kind,
        tokens.palette.text_subtle,
    );

    let tile = tile_rect(rect, reveal);
    if reveal > 0.0 {
        controls::play_tile(
            ui,
            tokens,
            Control::CardPlay,
            tile,
            &format!("Play {}", card.name),
            false,
        );
    }

    // The tile is a waiting control, so on the frame a click is released egui
    // reports the card as the hovered widget and the tile as not hovered at all.
    // Where the click landed is what decides, and only a pointer click has one —
    // a keyboard activation is never mistaken for a click on the tile.
    let on_tile = response
        .interact_pointer_pos()
        .is_some_and(|at| tile.contains(at));
    response.clicked() && !on_tile
}

/// What a screen reader says for the card: its name and what it is, so a grid of
/// playlists and podcasts is not announced as one kind.
fn spoken(card: &Card<'_>) -> String {
    format!("{} {}", card.name, card.kind.to_lowercase())
}

/// Where the tile rests once the card is fully revealed: `space::S2` in from
/// the card's bottom-right corner, an `extent::CARD_PLAY` square. The click test
/// solves the same rect, so a click and the tile it aims at cannot disagree.
pub(super) fn play_rect(rect: Rect) -> Rect {
    Rect::from_min_size(
        Pos2::new(
            rect.right() - space::S2 - extent::CARD_PLAY,
            rect.bottom() - space::S2 - extent::CARD_PLAY,
        ),
        Vec2::splat(extent::CARD_PLAY),
    )
}

/// Where the tile sits in the card's corner, rising as the card is revealed.
fn tile_rect(rect: Rect, reveal: f32) -> Rect {
    play_rect(rect).translate(Vec2::new(0.0, space::S1 * (1.0 - reveal)))
}

/// The height of one line of `font`.
fn line_height(ui: &Ui, font: &FontId) -> f32 {
    ui.ctx().fonts_mut(|fonts| fonts.row_height(font))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_is_announced_with_its_own_kind() {
        let tokens = Tokens::DARK;
        let card = |kind| Card {
            uri: "spotify:show:a",
            name: "Field Notes",
            kind,
            cover: Cover::Missing,
        };
        assert_eq!(spoken(&card("Podcast")), "Field Notes podcast");
        assert_eq!(spoken(&card("Playlist")), "Field Notes playlist");
        let _ = tokens;
    }
}
