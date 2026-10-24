//! The band above a playlist's tracks.
//!
//! The band says what you are looking at: which kind of context it is, its name in
//! the largest type in the window, and what it holds. It is the words and the
//! picture only: the row of actions that act on the whole context is
//! [`super::actionbar`], drawn under the band by the page and pinned to the top of
//! the page's scroll viewport.
//!
//! The playlist's description sits under its title in the reference's own byline
//! style — mono, muted, tracked apart — and its owner rides the meta line as
//! `Made by <owner>` rather than the reference's inline link, because there is no
//! user page to open. A description longer than two rows elides, where the
//! reference lets it wrap.
//!
//! A playlist's band draws the four-cover mosaic when the list has four distinct
//! album covers and every one of them is held; until then it draws the playlist's
//! own picture. The artist form of the band, with a photograph behind it, is
//! still missing.

use egui::{FontId, Id, Margin, Pos2, Rect, Sense, Ui, Vec2, WidgetInfo, vec2};
use takofy_model::PLAYLIST_NAME_MAX;

use crate::cover::Cover;
use crate::layout::hero::title_size;
use crate::mosaic::QUADRANTS;
use crate::naming::Renaming;
use crate::theme::{
    MonoWeight, Tokens, Weight, fonts, hero, radius, space, stroke, text, tracking,
};

use super::{art, context_menu, name_field, text as recipe};

/// What a band says about the context it heads.
pub struct Band<'a> {
    /// The kind of context: "Playlist" everywhere it is drawn today.
    pub eyebrow: &'a str,
    pub title: &'a str,
    /// The counts and the lengths under the title, already worded.
    pub meta: Vec<String>,
    /// The playlist's description, already collapsed to a line; empty is absent.
    pub description: &'a str,
    /// The context's cover, as the art pipeline resolves it.
    pub cover: Cover,
}

/// What the band draws in its art box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Picture {
    /// The context's single cover, as the art pipeline resolves it.
    Cover,
    /// Four covers tiling the box, in reading order.
    Mosaic([Cover; QUADRANTS]),
}

/// What the title can do this frame.
pub enum Title<'a> {
    Plain,
    Clickable,
    Editing(&'a mut Renaming),
}

/// What drawing the band reported.
pub struct Reply {
    pub rename_clicked: bool,
    /// The title's id, so a finished rename hands the keyboard back to it.
    pub title_id: Option<Id>,
}

/// Draws the band and how its title may be used this frame.
pub fn show_with(ui: &mut Ui, band: &Band<'_>, tokens: &Tokens, title: Title<'_>) -> Reply {
    show_picture(ui, band, Picture::Cover, tokens, title)
}

/// Draws the band with `picture` in its art box.
pub fn show_picture(
    ui: &mut Ui,
    band: &Band<'_>,
    picture: Picture,
    tokens: &Tokens,
    title: Title<'_>,
) -> Reply {
    let width = ui.available_width();
    // The art is taller than the band's own minimum, and the padding sits inside
    // the band, so the art plus that padding is what the band comes to.
    let height = (hero::ART + 2.0 * space::S5).max(hero::MIN_H);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    wash(ui, rect, tokens);
    let title = inner(ui, rect, band, picture, tokens, title);
    Reply {
        rename_clicked: title.activated,
        title_id: title.id,
    }
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

struct TitleDrawn {
    id: Option<Id>,
    activated: bool,
}

/// The art and the words, both standing on the band's floor.
fn inner(
    ui: &mut Ui,
    rect: Rect,
    band: &Band<'_>,
    picture: Picture,
    tokens: &Tokens,
    title_tool: Title<'_>,
) -> TitleDrawn {
    let floor = rect.bottom() - space::S5;
    let art = Rect::from_min_size(
        Pos2::new(rect.left() + space::GUTTER, floor - hero::ART),
        Vec2::splat(hero::ART),
    );
    match picture {
        Picture::Cover => art::tile(
            ui.painter(),
            art,
            radius::SM,
            tokens.palette.border_strong,
            band.cover,
            tokens,
        ),
        Picture::Mosaic(covers) => art::mosaic(
            ui.painter(),
            art,
            radius::SM,
            tokens.palette.border_strong,
            covers,
            tokens,
        ),
    }

    let left = art.right() + space::S5;
    let width = (rect.right() - space::GUTTER - left).max(0.0);
    if width <= 0.0 {
        return TitleDrawn {
            id: None,
            activated: false,
        };
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
    let font = FontId::new(size, fonts::sans(Weight::Black));
    let title = recipe::line(
        ui,
        band.title,
        font.clone(),
        tokens.palette.text,
        recipe::tracking_pts(size, tracking::TIGHT),
        width,
    );
    bottom -= title.size().y;
    // The edit and click target is the column, never the galley's text width: a
    // short name must still offer a field wide enough to type in.
    let title_rect = Rect::from_min_size(Pos2::new(left, bottom), vec2(width, title.size().y));
    let mut drawn = TitleDrawn {
        id: None,
        activated: false,
    };
    match title_tool {
        Title::Plain => {
            ui.painter()
                .galley(Pos2::new(left, bottom), title, tokens.palette.text);
        }
        Title::Clickable => {
            ui.painter()
                .galley(Pos2::new(left, bottom), title, tokens.palette.text);
            let id = ui.scope_id().with("band-title");
            let response = ui.interact(title_rect, id, Sense::click());
            response
                .widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, "Rename playlist"));
            if response.clicked() {
                response.request_focus();
            }
            drawn.id = Some(id);
            drawn.activated = response.clicked() && !context_menu::held_repeat(ui);
        }
        Title::Editing(renaming) => {
            let id = ui.scope_id().with("band-title");
            drawn.id = Some(id);
            let writing = renaming.writing();
            let wants_focus = renaming.wants_focus();
            let out = {
                let text = renaming.edit();
                name_field::edit(
                    ui,
                    name_field::Spec {
                        id,
                        rect: title_rect,
                        text,
                        writing,
                        wants_focus,
                        font,
                        hint: "",
                        inset: Margin::same(0),
                        text_color: tokens.palette.text,
                        weak_color: tokens.palette.text_muted,
                        cursor: egui::Stroke::new(stroke::HAIRLINE, tokens.palette.text),
                        align: egui::Align2::LEFT_CENTER,
                        limit: PLAYLIST_NAME_MAX,
                    },
                )
            };
            if out.focused {
                renaming.focus_taken();
            }
            if out.lost_focus && renaming.text().trim().is_empty() {
                renaming.dismiss();
            }
            if out.enter {
                name_field::post_submit(ui.ctx());
            }
            if out.escape {
                renaming.cancel();
            }
        }
    }
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
    drawn
}

#[cfg(test)]
mod tests {
    use egui::{Rect, Vec2};

    use super::*;

    /// A drawn band, so the frame's own answers are read rather than the helper
    /// that decides them.
    fn band_output() -> (Reply, egui::FullOutput) {
        let tokens = Tokens::DARK;
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::theme::fonts::definitions());
        ctx.set_global_style(crate::theme::style(&tokens));
        let band = Band {
            eyebrow: "Album",
            title: "Signal Decay",
            meta: Vec::new(),
            description: "",
            cover: crate::cover::Cover::Missing,
        };
        let mut reply = Reply {
            rename_clicked: false,
            title_id: None,
        };
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 400.0))),
                ..Default::default()
            },
            |ui| {
                reply = show_with(ui, &band, &tokens, Title::Plain);
            },
        );
        output.textures_delta.clear();
        (reply, output)
    }

    #[test]
    fn a_band_with_a_plain_title_reports_nothing() {
        let (reply, _) = band_output();
        assert!(
            !reply.rename_clicked,
            "an untouched frame asked for nothing"
        );
        assert_eq!(reply.title_id, None, "a painted title carries no id");
    }
}
