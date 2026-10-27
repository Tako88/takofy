//! The band above a playlist's tracks.
//!
//! The band says what you are looking at: which kind of context it is, its name in
//! the largest type in the window, and what it holds. It is the words and the
//! picture only: the row of actions that act on the whole context is
//! [`super::actionbar`], drawn under the band by the page and pinned to the top of
//! the page's scroll viewport.
//!
//! The byline under the title is a credit and its facts. The credit is the one
//! name the band can open — an album's artist — and it is a link when the source
//! knows that page; the facts follow it on the same line and are dropped when the
//! name has taken the column. A credit with no page, or a band with no credit at
//! all, paints the whole byline as plain words.
//!
//! The playlist's description sits under its title in the reference's own byline
//! style — mono, muted, tracked apart — and its owner rides the meta line as
//! `Made by <owner>` rather than the reference's inline link, because there is no
//! user page to open. A description longer than two rows elides, where the
//! reference lets it wrap.
//!
//! A playlist's band draws the four-cover mosaic when the list has four distinct
//! album covers and every one of them is held; until then it draws the playlist's
//! own picture.

use egui::{Color32, FontId, Id, Margin, Pos2, Rect, Sense, Ui, Vec2, WidgetInfo, vec2};
use takofy_model::PLAYLIST_NAME_MAX;

use crate::cover::Cover;
use crate::layout::hero::title_size;
use crate::mosaic::QUADRANTS;
use crate::naming::Renaming;
use crate::route::Page;
use crate::theme::{
    MonoWeight, Tokens, Weight, fonts, hero, radius, space, stroke, text, tracking,
};

use super::link::{self, Target};
use super::{art, context_menu, name_field, text as recipe};

/// The byline's credit: the one name that may open a page.
///
/// A blank name is no credit at all, whatever page it would have opened.
pub struct Credit<'a> {
    name: &'a str,
    target: Option<Target<'a>>,
}

impl<'a> Credit<'a> {
    /// The credit named `name`, or `None` when that name is blank.
    pub fn new(name: &'a str, target: Option<Target<'a>>) -> Option<Self> {
        let name = name.trim();
        (!name.is_empty()).then_some(Self { name, target })
    }
}

/// What a band says about the context it heads.
pub struct Band<'a> {
    /// The kind of context: "Playlist" everywhere it is drawn today.
    pub eyebrow: &'a str,
    pub title: &'a str,
    /// The counts and the lengths under the title, already worded.
    pub meta: Vec<String>,
    /// The credit the facts follow, drawn first on the byline.
    pub credit: Option<Credit<'a>>,
    /// The playlist's description, already collapsed to a line; empty is absent.
    pub description: &'a str,
    /// The context's cover, as the art pipeline resolves it.
    pub cover: Cover,
    /// The colour derived from the same art, when its pixels are held.
    pub tint: Option<Color32>,
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
    /// The page the byline's credit asked for.
    pub open: Option<Page>,
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
    wash(ui, rect, band.tint, tokens);
    let title = inner(ui, rect, band, picture, tokens, title);
    Reply {
        rename_clicked: title.activated,
        title_id: title.id,
        open: title.open,
    }
}

/// The tint and the hairline that close the band off from the tracks.
pub(super) fn wash(ui: &Ui, rect: Rect, tint: Option<Color32>, tokens: &Tokens) {
    ui.painter()
        .rect_filled(rect, 0.0, tint.unwrap_or(tokens.palette.hero_wash));
    ui.painter().rect_filled(
        Rect::from_min_size(
            Pos2::new(rect.left(), rect.bottom() - stroke::HAIRLINE),
            vec2(rect.width(), stroke::HAIRLINE),
        ),
        0.0,
        tokens.palette.border,
    );
}

/// The byline's plain line: one muted, tracked mono row at `width`.
pub(super) fn byline_line(
    ui: &Ui,
    words: &str,
    width: f32,
    tokens: &Tokens,
) -> std::sync::Arc<egui::Galley> {
    recipe::line(
        ui,
        words,
        recipe::hero_description(),
        tokens.palette.text_muted,
        recipe::tracking_pts(text::XS, tracking::SLIGHT),
        width,
    )
}

struct TitleDrawn {
    id: Option<Id>,
    activated: bool,
    open: Option<Page>,
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
            open: None,
        };
    }

    let mut bottom = floor;
    let mut open = None;
    if band.credit.is_some() || !band.meta.is_empty() {
        let (next, asked) = byline(ui, band, tokens, left, width, bottom);
        bottom = next;
        open = asked;
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
        open,
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
            // The field has the pointer; the byline under it is not clickable.
            drawn.open = None;
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

/// The byline under the title: the credit, then the plain facts after it.
///
/// With a page to open, the name is a link and the facts take what it left on
/// the same line, as long as that is room for one whole fact; a name that
/// leaves less drops them. Without a page the two are one joined line, so an
/// unknown URI reads exactly as a plain name.
/// Returns the byline's top edge and the page the credit asked for.
fn byline(
    ui: &Ui,
    band: &Band<'_>,
    tokens: &Tokens,
    left: f32,
    width: f32,
    bottom: f32,
) -> (f32, Option<Page>) {
    let ink = tokens.palette.text_muted;
    let font = recipe::hero_description();
    let tracking = recipe::tracking_pts(text::XS, tracking::SLIGHT);
    let facts = band.meta.join(" · ");
    let name = band.credit.as_ref().map(|credit| credit.name);
    let target = band.credit.as_ref().and_then(|credit| credit.target);

    let Some((name, target)) = name.zip(target) else {
        let words = match name {
            Some(name) if facts.is_empty() => name.to_owned(),
            Some(name) => format!("{name} · {facts}"),
            None => facts,
        };
        let line = byline_line(ui, &words, width, tokens);
        let top = bottom - line.size().y;
        ui.painter().galley(Pos2::new(left, top), line, ink);
        return (top, None);
    };

    let galley = link::lay(ui, name, &link::Face { font, tracking }, width);
    // A name that elides has taken the column; the facts would be a dot and an
    // ellipsis after it.
    let elided = galley.elided;
    let top = bottom - galley.size().y;
    let shown = link::Placed {
        pos: Pos2::new(left, top),
        galley,
    }
    .link(
        ui,
        ui.scope_id().with("band-credit"),
        ui.clip_rect(),
        Some(target),
        ink,
        tokens,
    );
    // The facts take what the name left, and a name that filled the column
    // leaves them out rather than eliding them down to a dot.
    let room = if elided {
        0.0
    } else {
        (left + width - shown.rect.right()).max(0.0)
    };
    // The facts need room for the separator and one whole fact. Less than that
    // and the line reads as a stray dot or a lone ellipsis after the name.
    let least = band.meta.first().map_or(0.0, |first| {
        recipe::line(
            ui,
            &format!(" · {first}"),
            recipe::hero_description(),
            ink,
            tracking,
            f32::INFINITY,
        )
        .size()
        .x
    });
    if !facts.is_empty() && room >= least {
        let line = byline_line(ui, &format!(" · {facts}"), room, tokens);
        ui.painter()
            .galley(Pos2::new(shown.rect.right(), top), line, ink);
    }
    (top, shown.page)
}

#[cfg(test)]
mod tests;
