//! The band above a playlist's tracks.
//!
//! The band says what you are looking at: which kind of context it is, its name in
//! the largest type in the window, and what it holds. Under it is the row of
//! actions that act on the whole context rather than on one track — of which only
//! play can act today.
//!
//! The playlist's description sits under its title in the reference's own byline
//! style — mono, muted, tracked apart — and its owner rides the meta line as
//! `Made by <owner>` rather than the reference's inline link, because there is no
//! user page to open. A description longer than two rows elides, where the
//! reference lets it wrap.
//!
//! What the reference puts here and this does not: the four-cover mosaic, and the
//! artist form of the band with a photograph behind it.

use egui::{FontId, Pos2, Rect, Sense, Ui, Vec2, vec2};

use crate::availability::Control;
use crate::cover::Cover;
use crate::icon::Icon;
use crate::layout::hero::title_size;
use crate::theme::icon::Size;
use crate::theme::{
    MonoWeight, Tokens, Weight, extent, fonts, hero, radius, space, stroke, text, tracking,
};

use super::{art, controls, text as recipe};

/// What a band says about the context it heads.
pub struct Band<'a> {
    /// The kind of context: "Playlist" everywhere it is drawn today.
    pub eyebrow: &'a str,
    pub title: &'a str,
    /// The counts and the lengths under the title, already worded.
    pub meta: Vec<String>,
    /// The playlist's description, already collapsed to a line; empty is absent.
    pub description: &'a str,
    /// Whether there is anything to play.
    pub playable: bool,
    /// Whether the reference gives this context a shuffle control.
    ///
    /// A playlist has one; an album's action bar has like and more only, so the
    /// flag drops the control rather than duplicating the band. The tools are
    /// laid out right-to-left, so dropping the leftmost keeps the others still.
    pub shuffle: bool,
    /// The context's cover, as the art pipeline resolves it.
    pub cover: Cover,
}

/// Draws the band and its action row, and reports a request to play the context.
///
/// The band takes the width it is given and starts at the cursor, so the caller
/// decides where it sits; the painter decides only how it looks. The column of
/// words is stacked up from the band's floor, so a title that changes size with
/// the window does not move where the band ends.
pub fn show(ui: &mut Ui, band: &Band<'_>, tokens: &Tokens) -> bool {
    let width = ui.available_width();
    // The art is taller than the band's own minimum, and the padding sits inside
    // the band, so the art plus that padding is what the band comes to.
    let height = (hero::ART + 2.0 * space::S5).max(hero::MIN_H);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    wash(ui, rect, tokens);
    inner(ui, rect, band, tokens);

    actions(ui, band, tokens)
}

/// The tint and the hairline that close the band off from the tracks.
fn wash(ui: &Ui, rect: Rect, tokens: &Tokens) {
    ui.painter()
        .rect_filled(rect, 0.0, tokens.palette.hero_wash);
    ui.painter().rect_filled(
        Rect::from_min_size(
            Pos2::new(rect.left(), rect.bottom() - stroke::HAIRLINE),
            vec2(rect.width(), stroke::HAIRLINE),
        ),
        0.0,
        tokens.palette.border,
    );
}

/// The art and the words, both standing on the band's floor.
fn inner(ui: &Ui, rect: Rect, band: &Band<'_>, tokens: &Tokens) {
    let floor = rect.bottom() - space::S5;
    let art = Rect::from_min_size(
        Pos2::new(rect.left() + space::GUTTER, floor - hero::ART),
        Vec2::splat(hero::ART),
    );
    art::tile(
        ui.painter(),
        art,
        radius::SM,
        tokens.palette.border_strong,
        band.cover,
        tokens,
    );

    let left = art.right() + space::S5;
    let width = (rect.right() - space::GUTTER - left).max(0.0);
    if width <= 0.0 {
        return;
    }

    let mut bottom = floor;
    if !band.meta.is_empty() {
        let byline = recipe::line(
            ui,
            &band.meta.join(" · "),
            recipe::hero_description(),
            tokens.palette.text_muted,
            recipe::tracking_pts(text::XS, tracking::SLIGHT),
            width,
        );
        bottom -= byline.size().y;
        ui.painter()
            .galley(Pos2::new(left, bottom), byline, tokens.palette.text_muted);
        bottom -= space::S2;
    }

    // The description is the reference's second byline: mono, muted and tracked
    // apart like the meta line, but wrapped to two rows at most and elided past
    // them. An empty one takes neither rows nor gap, so the band closes up.
    if !band.description.is_empty() {
        let description = recipe::clamped(
            ui,
            band.description,
            recipe::hero_description(),
            tokens.palette.text_muted,
            recipe::tracking_pts(text::XS, tracking::SLIGHT),
            width,
            hero::DESCRIPTION_ROWS,
        );
        bottom -= description.size().y;
        ui.painter().galley(
            Pos2::new(left, bottom),
            description,
            tokens.palette.text_muted,
        );
        bottom -= space::S2;
    }

    // The reference's 4.5 vw is a share of the *window*, not of the column this
    // band happens to be drawn in, so the panel's own width is the wrong input.
    let size = title_size(ui.ctx().content_rect().width());
    let title = recipe::line(
        ui,
        band.title,
        FontId::new(size, fonts::sans(Weight::Black)),
        tokens.palette.text,
        recipe::tracking_pts(size, tracking::TIGHT),
        width,
    );
    bottom -= title.size().y;
    ui.painter()
        .galley(Pos2::new(left, bottom), title, tokens.palette.text);
    bottom -= space::S2;

    // The eyebrow is the reference's small caps tier: monospace, tracked apart,
    // upper-cased, and muted rather than subtle.
    let eyebrow = recipe::spaced(
        ui,
        &band.eyebrow.to_uppercase(),
        FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
        tokens.palette.text_muted,
        recipe::tracking_pts(text::XXS, tracking::CAPS),
    );
    bottom -= eyebrow.size().y;
    ui.painter()
        .galley(Pos2::new(left, bottom), eyebrow, tokens.palette.text_muted);
}

/// The row under the band: play, and the tools that cannot act yet.
///
/// A playlist's row holds three of them, an album's two: the shuffle is the one
/// the flag drops.
fn actions(ui: &mut Ui, band: &Band<'_>, tokens: &Tokens) -> bool {
    let width = ui.available_width();
    let height = hero::PLAY + 2.0 * space::S4;
    let (row, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let centre = row.center().y;

    let play = Rect::from_min_size(
        Pos2::new(row.left() + space::GUTTER, centre - hero::PLAY / 2.0),
        Vec2::splat(hero::PLAY),
    );
    let slot = controls::play_tile(
        ui,
        tokens,
        Control::HeroPlay,
        play,
        &format!("Play {}", band.title),
        band.playable,
    );
    let played = slot.response.clicked();

    // The tools hug the right edge, in the reference's order. None of them can act
    // yet: shuffle and a save need the Web API, and the menu is a later phase.
    let mut tools = Vec::with_capacity(3);
    if band.shuffle {
        tools.push((Control::HeroShuffle, Icon::Shuffle, "Shuffle"));
    }
    tools.push((Control::HeroLike, Icon::Heart, "Save to your library"));
    tools.push((Control::HeroMore, Icon::More, "More options"));
    let mut right = row.right() - space::GUTTER;
    for (control, icon, label) in tools.into_iter().rev() {
        let rect = Rect::from_min_size(
            Pos2::new(right - extent::BUTTON_LG, centre - extent::BUTTON_LG / 2.0),
            Vec2::splat(extent::BUTTON_LG),
        );
        controls::icon_button(ui, tokens, control, rect, icon, Size::MD, label, false);
        right = rect.left() - space::S2;
    }

    played
}
