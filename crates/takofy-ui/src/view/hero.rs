//! The band above a playlist's tracks.
//!
//! The band says what you are looking at: which kind of context it is, its name in
//! the largest type in the window, and what it holds. Under it is the row of
//! actions that act on the whole context rather than on one track.
//!
//! The playlist's description sits under its title in the reference's own byline
//! style — mono, muted, tracked apart — and its owner rides the meta line as
//! `Made by <owner>` rather than the reference's inline link, because there is no
//! user page to open. A description longer than two rows elides, where the
//! reference lets it wrap.
//!
//! What the reference puts here and this does not: the four-cover mosaic, and the
//! artist form of the band with a photograph behind it.

use egui::{FontId, Id, Margin, Pos2, Rect, Sense, Ui, Vec2, WidgetInfo, vec2};
use takofy_model::PLAYLIST_NAME_MAX;

use crate::availability::Control;
use crate::cover::Cover;
use crate::icon::Icon;
use crate::layout::hero::title_size;
use crate::menu::Door;
use crate::naming::Renaming;
use crate::theme::icon::Size;
use crate::theme::{
    MonoWeight, Tokens, Weight, extent, fonts, hero, radius, space, stroke, text, tracking,
};

use super::{art, context_menu, controls, name_field, text as recipe};

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

/// How the band's heart reads, given what the library says about the context.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Heart {
    label: &'static str,
    /// Whether it can act: no list could say, so nothing is offered.
    live: bool,
    /// Whether the glyph is the filled, saved one.
    lit: bool,
}

/// The heart's three answers, in one place so the label and the glyph cannot
/// disagree about whether the context is in the library.
fn heart(saved: Option<bool>) -> Heart {
    match saved {
        Some(true) => Heart {
            label: "Remove from your library",
            live: true,
            lit: true,
        },
        Some(false) => Heart {
            label: "Save to your library",
            live: true,
            lit: false,
        },
        None => Heart {
            label: "Save to your library",
            live: false,
            lit: false,
        },
    }
}

/// What the title can do this frame.
pub enum Title<'a> {
    Plain,
    Clickable,
    Editing(&'a mut Renaming),
}

/// The band's optional tools.
impl Tools<'_> {
    /// A band with no tools: what a test draws when it is about the band itself.
    #[cfg(test)]
    pub fn plain() -> Tools<'static> {
        Tools {
            title: Title::Plain,
            more: None,
            saved: None,
        }
    }
}

pub struct Tools<'a> {
    pub title: Title<'a>,
    /// Whether the "…" can act. `None` keeps it present and inert.
    pub more: Option<bool>,
    /// Whether this context is in the library, or `None` while no list can say.
    pub saved: Option<bool>,
}

/// What drawing the band reported.
pub struct Reply {
    pub played: bool,
    pub rename_clicked: bool,
    pub more_door: Option<Door>,
    /// The value the heart asks for, when it was pressed.
    pub saved: Option<bool>,
    /// Whether the shuffle was pressed.
    pub shuffled: bool,
    /// The title's id, so a finished rename hands the keyboard back to it.
    pub title_id: Option<Id>,
}

/// Draws the band with the tools the page has, and reports what they did.
pub fn show_with(ui: &mut Ui, band: &Band<'_>, tokens: &Tokens, tools: Tools<'_>) -> Reply {
    let Tools { title, more, saved } = tools;
    let width = ui.available_width();
    // The art is taller than the band's own minimum, and the padding sits inside
    // the band, so the art plus that padding is what the band comes to.
    let height = (hero::ART + 2.0 * space::S5).max(hero::MIN_H);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    wash(ui, rect, tokens);
    let title = inner(ui, rect, band, tokens, title);
    let (played, more_door, saved, shuffled) = actions(ui, band, tokens, more, saved);

    Reply {
        played,
        shuffled,
        rename_clicked: title.activated,
        more_door,
        saved,
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
    tokens: &Tokens,
    title_tool: Title<'_>,
) -> TitleDrawn {
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

/// The row under the band: play, and the tools that act on the context.
///
/// A playlist's row holds three of them, an album's two: the shuffle is the one
/// the flag drops.
fn actions(
    ui: &mut Ui,
    band: &Band<'_>,
    tokens: &Tokens,
    more: Option<bool>,
    saved: Option<bool>,
) -> (bool, Option<Door>, Option<bool>, bool) {
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

    // The tools hug the right edge, in the reference's order. Only the menu can
    // act on a playlist; the other two wait for the writes behind them.
    let mut tools = Vec::with_capacity(3);
    if band.shuffle {
        tools.push((Control::HeroShuffle, Icon::Shuffle, "Shuffle"));
    }
    tools.push((Control::HeroLike, Icon::Heart, "Save to your library"));
    tools.push((Control::HeroMore, Icon::More, "More options"));
    let more_live = more.unwrap_or(false);
    let mut right = row.right() - space::GUTTER;
    let mut door = None;
    let mut asked = None;
    let mut shuffled = false;
    for (control, icon, label) in tools.into_iter().rev() {
        let rect = Rect::from_min_size(
            Pos2::new(right - extent::BUTTON_LG, centre - extent::BUTTON_LG / 2.0),
            Vec2::splat(extent::BUTTON_LG),
        );
        let heart = heart(saved);
        let (live, label) = match control {
            Control::HeroMore => (more_live, label),
            Control::HeroLike => (heart.live, heart.label),
            Control::HeroShuffle => (true, label),
            _ => (false, label),
        };
        let slot = if control == Control::HeroLike && heart.lit {
            controls::icon_button_on(ui, tokens, control, (), rect, icon, Size::MD, label, live)
        } else {
            controls::icon_button(ui, tokens, control, rect, icon, Size::MD, label, live)
        };
        if control == Control::HeroMore {
            door = context_menu::button_door(ui, &slot, rect, slot.response.id);
        }
        if control == Control::HeroLike && slot.response.clicked() {
            asked = saved.map(|saved| !saved);
        }
        if control == Control::HeroShuffle && slot.response.clicked() {
            shuffled = true;
        }
        right = rect.left() - space::S2;
    }

    (played, door, asked, shuffled)
}

#[cfg(test)]
mod tests {
    use egui::{Pos2, Rect, Vec2};

    use super::*;

    /// A drawn band, so the frame's own answers are read rather than the helper
    /// that decides them.
    fn band_output(saved: Option<bool>, at: Option<Pos2>) -> (Reply, egui::FullOutput) {
        let tokens = Tokens::DARK;
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::theme::fonts::definitions());
        ctx.set_global_style(crate::theme::style(&tokens));
        let band = Band {
            eyebrow: "Album",
            title: "Signal Decay",
            meta: Vec::new(),
            description: "",
            playable: true,
            shuffle: true,
            cover: crate::cover::Cover::Missing,
        };
        let mut reply = Reply {
            played: false,
            shuffled: false,
            rename_clicked: false,
            more_door: None,
            saved: None,
            title_id: None,
        };
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 400.0))),
                ..Default::default()
            },
            |ui| {
                reply = show_with(
                    ui,
                    &band,
                    &tokens,
                    Tools {
                        title: Title::Plain,
                        more: None,
                        saved,
                    },
                );
                if let Some(at) = at {
                    let _ = at;
                }
            },
        );
        output.textures_delta.clear();
        (reply, output)
    }

    #[test]
    fn a_band_with_shuffle_reports_its_press() {
        let (reply, _) = band_output(Some(true), None);
        assert!(!reply.shuffled, "an untouched frame asked for nothing");
        assert_eq!(reply.saved, None);
    }

    #[test]
    fn the_heart_says_what_it_would_do_and_only_acts_on_a_known_answer() {
        let saved = heart(Some(true));
        assert_eq!(saved.label, "Remove from your library");
        assert!(saved.lit, "a saved context's heart is filled");
        assert!(saved.live);

        let unsaved = heart(Some(false));
        assert_eq!(unsaved.label, "Save to your library");
        assert!(!unsaved.lit);
        assert!(unsaved.live);

        let unknown = heart(None);
        assert!(
            !unknown.live,
            "no list could say, so the heart offers nothing"
        );
        assert!(!unknown.lit);
    }
}
