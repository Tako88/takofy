//! The Now Playing panel: what the panel shows before it has a queue.
//!
//! The tabs and the queue arrive with the data that fills them. What is here is
//! real: the caption, an art slot, and the track the transport names.

use egui::{Align2, FontId, Pos2, Rect, Ui, Vec2};

use crate::availability::Control;
use crate::cover::Cover;
use crate::icon::Icon;
use crate::player::Transport;
use crate::theme::icon::Size;
use crate::theme::{MonoWeight, Tokens, Weight, extent, fonts, radius, space, text, tracking};

use super::art;
use super::controls;
use super::text as recipe;

/// Draws the panel's contents inside its own rect.
pub fn show(ui: &mut Ui, transport: &Transport, cover: Cover, tokens: &Tokens) {
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
        cover,
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

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use takofy_model::{CoverImage, CoverUrl};

    use super::*;
    use crate::cover::{Art, Cover};
    use crate::message::Update;
    use crate::theme::{fonts, style};

    fn context() -> egui::Context {
        let tokens = Tokens::DARK;
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        ctx
    }

    fn input() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 400.0))),
            ..Default::default()
        }
    }

    #[test]
    fn a_cover_arriving_after_play_is_ready_on_the_next_frame() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url");
        let mut transport = Transport::default();
        transport.play("spotify:track:x", "A Title", Some(url.clone()));
        let before = transport.now_playing.clone();

        let mut art = Art::default();
        let now = Instant::now();
        art.begin_frame();
        assert_eq!(art.look(Some(&url), now), Cover::Loading);
        let _ = art.requests(now);
        art.apply(
            &Update::Cover {
                url: url.clone(),
                image: CoverImage::new(1, 1, vec![0; 4]).expect("a 1x1 image"),
            },
            now,
        );
        art.upload(&ctx);
        art.begin_frame();
        let cover = art.look(Some(&url), now);
        assert!(matches!(cover, Cover::Ready(_)));

        // The panel reads the cover each frame and stores no pixels, so the
        // transport is untouched by the arrival.
        assert_eq!(transport.now_playing, before);

        let mut output = ctx.run_ui(input(), |ui| {
            show(ui, &transport, cover, &tokens);
        });
        output.textures_delta.clear();
        let texts: Vec<&str> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect();
        assert!(
            texts.contains(&"A Title"),
            "the panel must name the track: {texts:?}"
        );
    }
}
