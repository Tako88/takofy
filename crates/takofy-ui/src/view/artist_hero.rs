//! The artist page's hero: a photograph with the name standing on it.
//!
//! The reference's artist hero is not a playlist band with another word in it.
//! The picture is behind the words rather than beside them, a gradient scrim
//! darkens it so the name reads, there is no byline, and the action bar has no
//! like: play, `Follow`, shuffle, more. That is why this is its own view and the
//! playlist's band is untouched.
//!
//! The hero draws what it is handed. Following is the view's one write, and it
//! reports the press rather than performing it: the page owns the state and the
//! command.

use egui::{Pos2, Rect, Sense, Shape, Ui, Vec2, vec2};

use crate::availability::Control;
use crate::cover::Cover;
use crate::icon::Icon;
use crate::layout::hero::artist_title_size;
use crate::theme::icon::Size;
use crate::theme::{
    MonoWeight, Tokens, Weight, extent, fonts, hero, space, text, tracking,
};

use super::{art, controls, text as recipe};

/// What the artist hero says.
pub struct ArtistHero<'a> {
    pub name: &'a str,
    /// The portrait as the art pipeline resolves it.
    pub portrait: Cover,
    /// Whether there is anything to play.
    pub playable: bool,
    /// `Follow` or `Following`, as the followed state reads it.
    pub follow: &'a str,
    /// Whether the Follow control can act right now.
    pub follow_live: bool,
}

/// What a press on the hero asked for.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Pressed {
    pub played: bool,
    pub followed: bool,
}

/// Draws the hero and its action row.
///
/// The hero takes the width it is given and starts at the cursor, so the page
/// decides where it sits; the painter decides only how it looks.
pub fn show(ui: &mut Ui, hero: &ArtistHero<'_>, tokens: &Tokens) -> Pressed {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, hero::ARTIST_MIN_H), Sense::hover());
    art::fill(ui.painter(), rect, hero.portrait, tokens);
    scrim(ui, rect, tokens);
    words(ui, rect, hero.name, tokens);

    actions(ui, hero, tokens)
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

/// The row under the hero: play, Follow, and the tools that cannot act yet.
///
/// A press on Follow is reported rather than applied: the label only moves when
/// the server answers, and the page owns that state.
fn actions(ui: &mut Ui, hero: &ArtistHero<'_>, tokens: &Tokens) -> Pressed {
    let width = ui.available_width();
    let height = hero::PLAY + 2.0 * space::S4;
    let (row, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let centre = row.center().y;

    let play = Rect::from_min_size(
        Pos2::new(row.left() + space::GUTTER, centre - hero::PLAY / 2.0),
        Vec2::splat(hero::PLAY),
    );
    let played = controls::play_tile(
        ui,
        tokens,
        Control::HeroPlay,
        play,
        &format!("Play {}", hero.name),
        hero.playable,
    )
    .response
    .clicked();

    let follow_size = controls::text_button_size(ui, tokens, hero.follow);
    let follow = Rect::from_min_size(
        Pos2::new(play.right() + space::S4, centre - follow_size.y / 2.0),
        follow_size,
    );
    let followed = controls::text_button(
        ui,
        tokens,
        Control::Follow,
        follow,
        hero.follow,
        hero.follow_live,
    )
    .response
    .clicked();

    // The tools hug the right edge, in the reference's order.
    let mut right = row.right() - space::GUTTER;
    for (control, icon, label) in [
        (Control::HeroMore, Icon::More, "More options"),
        (Control::HeroShuffle, Icon::Shuffle, "Shuffle"),
    ] {
        let rect = Rect::from_min_size(
            Pos2::new(right - extent::BUTTON_LG, centre - extent::BUTTON_LG / 2.0),
            Vec2::splat(extent::BUTTON_LG),
        );
        controls::icon_button(ui, tokens, control, rect, icon, Size::MD, label, false);
        right = rect.left() - space::S2;
    }

    Pressed { played, followed }
}
