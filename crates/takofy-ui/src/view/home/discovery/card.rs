//! One discovery card: a face, its name, and the three things it can do.
//!
//! The card is a slot like any other, and its Follow, `⋯` and play are registered
//! after it so a press on a corner never falls through to the card. The corners
//! are revealed on hover or while one of them holds the keyboard, so they cover
//! no word a reader came for.

use egui::{Pos2, Rect, Ui};

use crate::availability::Control;
use crate::cover::{Cover, want_px};
use crate::icon::Icon;
use crate::layout::discovery;
use crate::menu::Door;
use crate::theme::{Tokens, radius, space, stroke, text, tracking};
use crate::view::playlist::root::MediaDraw;
use takofy_model::home::DiscoveryCard;

use super::super::super::{art, carry, context_menu, controls, media, text as recipe};
use super::super::Pressed;
use crate::verbs;

/// How a card's words sit about its picture.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Words {
    /// Under the picture, centred: the shared shelf shape.
    Below,
    /// Beside the picture: a pair's turned-on-its-side card.
    Beside,
}

/// One card to draw.
pub(super) struct Card<'a> {
    pub rect: Rect,
    pub art: Rect,
    pub data: &'a DiscoveryCard,
    /// The seed whose row this card belongs to, so two rows never share ids.
    pub seed: &'a str,
    pub words: Words,
}

/// Draws one card and records the presses it took.
pub(super) fn show(
    ui: &mut Ui,
    card: Card<'_>,
    tokens: &Tokens,
    draw: &mut MediaDraw<'_, '_>,
    out: &mut Pressed,
) {
    let key = (card.seed, card.data.artist.uri.as_str());
    let slot = controls::slot_keyed(
        ui,
        Control::DiscoveryCard,
        key,
        egui::Role::Button,
        card.rect,
        &format!("Open artist {}", card.data.artist.name),
        tokens,
        !carry::carrying(ui.ctx()) && !context_menu::is_open(ui.ctx()),
    );
    let over = slot.response.hovered() || ui.rect_contains_pointer(card.rect);
    if over || slot.response.has_focus() {
        ui.painter()
            .rect_filled(card.rect, radius::MD, tokens.palette.surface_raised);
        ui.painter().rect_stroke(
            card.rect,
            radius::MD,
            egui::Stroke::new(stroke::HAIRLINE, tokens.palette.border),
            egui::StrokeKind::Inside,
        );
    }

    let want = want_px(card.art.width(), ui.pixels_per_point());
    let cover: Cover = draw.art.look(card.data.portrait.as_ref(), want, draw.now);
    art::tile(
        ui.painter(),
        card.art,
        radius::PILL,
        tokens.palette.border,
        cover,
        tokens,
    );

    let follow = controls::slot_id(ui, Control::DiscoveryFollow, key);
    let more_id = controls::slot_id(ui, Control::DiscoveryMore, key);
    let play_id = controls::slot_id(ui, Control::DiscoveryPlay, key);
    let focused = ui.memory(|memory| memory.focused());
    let play_focused = focused == Some(play_id);
    let menu_open = context_menu::open_opener(ui.ctx()).is_some_and(|opener| opener == more_id);
    let revealed = over
        || slot.response.has_focus()
        || focused == Some(follow)
        || focused == Some(more_id)
        || play_focused
        || menu_open;

    paint_words(ui, &card, tokens);

    // The corner controls belong to the picture's box, as they do in the
    // reference, so a narrow card cannot put the play on its own title.
    let art = card.art;
    let tile = discovery::tile(art);
    let glyph = crate::theme::icon::Size::of_tile(tile);

    let live = !carry::carrying(ui.ctx());
    let mut pressed_play = false;

    if revealed {
        let follow_rect = discovery::follow(art);
        let follow = controls::plate_tile_keyed(
            ui,
            tokens,
            Control::DiscoveryFollow,
            key,
            follow_rect,
            Icon::Plus,
            glyph,
            &format!("Follow {}", card.data.artist.name),
            live && !draw.library.follow_pending(&card.data.artist.uri),
        );
        if follow.response.clicked() {
            out.follow = Some(card.data.artist.uri.clone());
        }

        let more = controls::plate_tile_open(
            ui,
            tokens,
            Control::DiscoveryMore,
            key,
            discovery::more(art),
            Icon::More,
            glyph,
            &format!("More options for {}", card.data.artist.name),
            live,
            menu_open,
        );
        let _ = ui.ctx().accesskit_node_builder(more.response.id, |node| {
            node.set_expanded(menu_open);
            node.set_has_popup(egui::accesskit::HasPopup::Menu);
        });

        let play = controls::play_tile_keyed(
            ui,
            tokens,
            Control::DiscoveryPlay,
            key,
            discovery::play(art),
            Icon::Play,
            glyph,
            &format!("Play {}", card.data.artist.name),
            live,
        );
        pressed_play = play.response.clicked();

        let id = more.response.id;
        if live
            && let Some(door) = context_menu::button_door(ui, &more, discovery::more(art), id)
                .or_else(|| media::key_door(ui, &slot.response, &more, discovery::more(art)))
                .or_else(|| {
                    context_menu::pointer_door(
                        ui,
                        card.rect,
                        slot.response.id,
                        discovery::more(art),
                    )
                })
        {
            post_menu(ui, &card, door);
        }
    }

    // A corner keeps its click even while inert: a press on Follow or the
    // menu must do nothing, not fall through and open the artist instead.
    let on_tile = slot.response.interact_pointer_pos().is_some_and(|at| {
        [
            discovery::follow(art),
            discovery::more(art),
            discovery::play(art),
        ]
        .iter()
        .any(|tile| tile.contains(at))
    });

    if pressed_play || (slot.response.clicked() && !on_tile && live) {
        if pressed_play {
            if let Some(track) = card.data.tracks.first() {
                out.discovery = Some(vec![track.clone()]);
            }
        } else {
            *draw.open = Some(crate::route::Page::Artist(card.data.artist.uri.clone()));
        }
    }
}

/// Posts the card's menu request for `door`.
fn post_menu(ui: &Ui, card: &Card<'_>, door: Door) {
    let request = verbs::discovery_request(
        card.data,
        door,
        Some(crate::route::Page::Artist(card.data.artist.uri.clone())),
    );
    context_menu::post(ui.ctx(), request);
}

/// The name and the sub line, placed by the card's shape.
fn paint_words(ui: &Ui, card: &Card<'_>, tokens: &Tokens) {
    let width = match card.words {
        Words::Below => card.art.width(),
        Words::Beside => (card.rect.right() - space::S2 - (card.art.right() + space::S4)).max(0.0),
    };
    let name = recipe::line(
        ui,
        &card.data.artist.name,
        recipe::card_name(),
        tokens.palette.text,
        0.0,
        width,
    );
    let sub = recipe::line(
        ui,
        &crate::format::discovery_sub(card.data.latest_year),
        recipe::card_kind(),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XXS, tracking::SLIGHT),
        width,
    );
    let (left, top) = match card.words {
        Words::Below => (
            card.art.center().x - width / 2.0,
            card.art.bottom() + space::S2,
        ),
        Words::Beside => {
            let left = card.art.right() + space::S4;
            let block = name.size().y + sub.size().y;
            (left, card.rect.center().y - block / 2.0)
        }
    };
    if card.words == Words::Below {
        let centred = card.art.center().x - name.size().x / 2.0;
        ui.painter()
            .galley(Pos2::new(centred, top), name.clone(), tokens.palette.text);
        let sub_x = card.art.center().x - sub.size().x / 2.0;
        ui.painter().galley(
            Pos2::new(sub_x, top + name.size().y),
            sub,
            tokens.palette.text_subtle,
        );
        return;
    }
    ui.painter()
        .galley(Pos2::new(left, top), name.clone(), tokens.palette.text);
    if !sub.text().is_empty() {
        ui.painter().galley(
            Pos2::new(left, top + name.size().y),
            sub,
            tokens.palette.text_subtle,
        );
    }
}
