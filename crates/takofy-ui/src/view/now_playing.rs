//! The Now Playing panel: what the panel shows before it has a queue.
//!
//! The tabs and the queue arrive with the data that fills them. What is here is
//! real: the caption, an art slot, and the track the transport names.

use egui::{Align2, FontId, Pos2, Rect, Ui, Vec2};

use crate::availability::Control;
use crate::icon::Icon;
use crate::player::Transport;
use crate::theme::icon::Size;
use crate::theme::{MonoWeight, Tokens, Weight, extent, fonts, radius, space, text, tracking};

use super::art;
use super::controls;
use super::text as recipe;

/// Draws the panel's contents inside its own rect.
pub fn show(ui: &mut Ui, transport: &Transport, tokens: &Tokens) {
    let band = ui.available_rect_before_wrap();

    let head = Rect::from_min_size(
        Pos2::new(band.left() + space::S3, band.top() + space::S3),
        Vec2::new(band.width() - 2.0 * space::S3, extent::BUTTON),
    );
    let caption = recipe::spaced(
        ui,
        "NOW PLAYING",
        FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XXS, tracking::CAPS),
    );
    ui.painter().galley(
        Pos2::new(head.left(), head.center().y - caption.size().y / 2.0),
        caption,
        tokens.palette.text_subtle,
    );
    let more = Rect::from_min_size(
        Pos2::new(
            head.right() - extent::BUTTON,
            head.center().y - extent::BUTTON / 2.0,
        ),
        Vec2::splat(extent::BUTTON),
    );
    controls::icon_button(
        ui,
        tokens,
        Control::NowPlayingMore,
        more,
        Icon::More,
        Size::MD,
        "More",
        true,
    );

    // The art slot: the largest square the panel can hold, up to the reference's
    // cap. It is the same placeholder a row and a card draw, so the art milestone
    // replaces one painter's body rather than hunting for every empty box.
    let side = (band.width() - 2.0 * space::S3).min(extent::NOWPLAYING_ART_MAX);
    let art = Rect::from_min_size(
        Pos2::new(band.center().x - side / 2.0, head.bottom() + space::S3),
        Vec2::splat(side),
    );
    art::tile(
        ui.painter(),
        art,
        radius::SM,
        tokens.palette.border_strong,
        tokens,
    );

    let meta = Rect::from_min_size(
        Pos2::new(band.left() + space::S3, art.bottom() + space::S4),
        Vec2::new(band.width() - 2.0 * space::S3, text::MD + space::S3),
    );
    match &transport.now_playing {
        None => {
            let line = recipe::line(
                ui,
                "Nothing playing",
                FontId::new(text::SM, fonts::sans(Weight::Regular)),
                tokens.palette.text_muted,
                recipe::tracking_pts(text::SM, tracking::SLIGHT),
                meta.width(),
            );
            ui.painter()
                .galley(meta.min, line, tokens.palette.text_muted);
        }
        Some(now) => {
            let title = recipe::line(
                ui,
                &now.title,
                FontId::new(text::MD, fonts::sans(Weight::Bold)),
                tokens.palette.text,
                recipe::tracking_pts(text::MD, tracking::TIGHT),
                meta.width(),
            );
            ui.painter().galley(meta.min, title, tokens.palette.text);
            let state = transport.state.label();
            if !state.is_empty() {
                ui.painter().text(
                    Pos2::new(meta.left(), meta.top() + text::MD + space::S1),
                    Align2::LEFT_TOP,
                    state,
                    FontId::new(text::XXS, fonts::mono(MonoWeight::Regular)),
                    tokens.palette.text_subtle,
                );
            }
        }
    }
}
