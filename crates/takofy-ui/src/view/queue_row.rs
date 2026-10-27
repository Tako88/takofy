//! One row of the Now Playing queue.
//!
//! The row is handed its data and its rect; it fetches nothing and decides no
//! width. What it decides is what a row looks like: the cover, the name over the
//! artist, the length, and a wash that follows the pointer.
//!
//! A row's identity is its absolute index in the queue, never its uri. Duplicate
//! adjacent entries are legal, so keying on the uri would give both rows one id,
//! one hover and one click, and the wrong occurrence would play. The row does not
//! carry a uri at all, which is what keeps that mistake out of reach.
//!
//! Its body is a click target and a drag source both. Enter and a double click
//! play the entry; a single click picks the row, and a drag lifts it, so scanning
//! the list with the pointer never starts playback.
//!
//! The name and the artist are links: a click on the words opens the page they
//! name and never plays, while a click beside them is the row's own. A carried
//! row and a ghost keep the words as plain text, because neither is a place to
//! navigate from.

use egui::{FontId, Id, Pos2, Rect, Ui, WidgetInfo};

use crate::cover::Cover;
use crate::layout::queue::row_parts;
use crate::route::Page;
use crate::theme::{Tokens, radius, space, text, tracking};

use super::link::{self, Face, Target};
use super::rows::{self, Ghosting, Look};
use super::text as recipe;
use super::track_row::Click;
use super::{art, controls};

/// What one row draws.
pub struct Row<'a> {
    /// The entry's position in the queue, which is its identity.
    pub index: usize,
    pub title: &'a str,
    pub artist: &'a str,
    pub duration: Option<&'a str>,
    pub cover: Cover,
    /// The album URI the name links to, when one is known.
    pub title_target: Option<&'a str>,
    /// The first artist's URI the artist links to, when one is known.
    pub artist_target: Option<&'a str>,
    /// Where the row stands in the queue's selection, and in a carry.
    pub look: Look,
    /// Whether this row is the one a just-finished edit landed on.
    pub refocus: bool,
}

/// What a row's two links reported.
#[derive(Debug, Default)]
struct Links {
    /// Whether the pointer is on either line's words.
    hovered: bool,
    /// The page a click on either line opened.
    page: Option<Page>,
}

/// The id a row's body is interacted with, so a caller can ask whether it holds
/// the keyboard.
pub fn id(ui: &Ui, index: usize) -> Id {
    ui.scope_id().with(("queue-row", index))
}

/// Draws one row into `rect` and reports what the pointer or the keyboard asked
/// for.
pub fn show(ui: &mut Ui, rect: Rect, row: &Row<'_>, tokens: &Tokens) -> Option<Click> {
    let body = rows::body(ui, rect, id(ui, row.index), row.look);
    let response = &body.response;
    response.widget_info(|| {
        WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!("{} by {}", row.title, row.artist),
        )
    });
    if row.refocus {
        // A row an edit landed on takes the keyboard with it, so Enter plays what
        // was just placed.
        response.request_focus();
    }

    let hover = rows::fade(
        ui,
        ui.scope_id().with(("queue-row-hover", row.index)),
        response.hovered() || response.has_focus(),
        row.look,
    );
    let before = rows::dim(ui, row.look);
    rows::wash(ui, rect, hover, row.look, tokens);
    // A ring on a carried row is noise: the pointer already holds it.
    if !row.look.carrying {
        controls::focus_ring(ui, response, rect, radius::SM, tokens);
    }
    let links = paint(ui, rect, row, None, tokens);
    ui.set_opacity(before);

    // The row's own keyboard activation is answered before the pointer's, so a
    // click landing in the same frame can never swallow it.
    let keyboard = response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    if keyboard {
        return Some(Click::Play);
    }
    if let Some(page) = links.page {
        return Some(Click::Open(page));
    }
    // When a link wins the hit test the row's `double_clicked()` is already
    // false, because egui tells only the hit widget; the guard keeps a second
    // click on the words a navigation rather than a play whatever the hit test
    // does.
    if response.double_clicked() && !links.hovered {
        return Some(Click::Play);
    }
    if body.lift {
        return Some(Click::Lift);
    }
    if links.hovered {
        return None;
    }
    body.press.map(Click::Select)
}

/// One row's own paint, kept so a carried row can be drawn away from its list.
#[derive(Clone, Debug, PartialEq)]
pub struct Lifted {
    title: String,
    artist: String,
    duration: Option<String>,
    cover: Cover,
}

impl Row<'_> {
    /// The row as it is drawn in flight: owned, and free of interaction.
    pub fn lifted(&self) -> Lifted {
        Lifted {
            title: self.title.to_owned(),
            artist: self.artist.to_owned(),
            duration: self.duration.map(str::to_owned),
            cover: self.cover,
        }
    }
}

impl Lifted {
    /// A ghost's own text, for a row whose list draws no queue row of its own.
    pub fn new(title: String, artist: String, duration: Option<String>, cover: Cover) -> Self {
        Self {
            title,
            artist,
            duration,
            cover,
        }
    }

    /// Draws the row into `rect` with no interaction of its own.
    pub fn paint(&self, ui: &Ui, rect: Rect, ghosting: Ghosting<'_>, tokens: &Tokens) {
        let row = Row {
            index: 0,
            title: &self.title,
            artist: &self.artist,
            duration: self.duration.as_deref(),
            cover: self.cover,
            title_target: None,
            artist_target: None,
            look: Look::default(),
            refocus: false,
        };
        paint(ui, rect, &row, Some(ghosting), tokens);
    }
}

/// Draws the cover, the name over the artist, and the length.
///
/// `ghosting` is set when this is a carried block rather than a row in its list:
/// it brings the block's own count and running time, and the flat ink a refused
/// target asks for.
fn paint(
    ui: &Ui,
    rect: Rect,
    row: &Row<'_>,
    ghosting: Option<Ghosting<'_>>,
    tokens: &Tokens,
) -> Links {
    let flat = ghosting.is_some_and(|ghost| ghost.flat);
    // A ghost and a row that a carry is holding are drawn, not navigated from.
    let live = ghosting.is_none() && !row.look.carrying;
    let count = ghosting.and_then(|ghost| (ghost.count > 1).then_some(ghost.count));
    // A block's total stands in for its rows' lengths, and an unknown one stands
    // in for nothing rather than for the row under the pointer.
    let length = match ghosting {
        Some(ghost) if ghost.count > 1 => ghost.total,
        Some(ghost) => ghost.total.or(row.duration),
        None => row.duration,
    };
    let duration = length.map(|label| {
        recipe::line(
            ui,
            label,
            recipe::track_mono(text::XXS),
            tokens.palette.text_subtle,
            0.0,
            rect.width(),
        )
    });
    let duration_w = duration.as_ref().map_or(0.0, |line| line.size().x);
    let parts = row_parts(rect, duration_w);

    art::tile(
        ui.painter(),
        parts.art,
        radius::XS,
        tokens.palette.border,
        row.cover,
        tokens,
    );

    let links = if parts.text.width() > 0.0 {
        stacked(ui, parts.text, row, count, flat, live, tokens)
    } else {
        Links::default()
    };
    if let Some(line) = duration {
        ui.painter().galley(
            Pos2::new(
                parts.duration.right() - line.size().x,
                parts.duration.center().y - line.size().y / 2.0,
            ),
            line,
            tokens.palette.text_subtle,
        );
    }
    links
}

/// The name over the artist, centred together as one block, with the count a
/// carried block stands for beside the name.
///
/// `live` is whether the two lines are links; a ghost and a carried row paint
/// them as plain text.
fn stacked(
    ui: &Ui,
    rect: Rect,
    row: &Row<'_>,
    count: Option<usize>,
    flat: bool,
    live: bool,
    tokens: &Tokens,
) -> Links {
    let name = recipe::queue_name();
    let name_h = line_height(ui, &name);
    let artist = recipe::queue_artist();
    let artist_h = if row.artist.is_empty() {
        0.0
    } else {
        line_height(ui, &artist)
    };
    let top = rect.center().y - (name_h + artist_h) / 2.0;
    // The flat ink a refused target gets, so the ghost reads as set back from
    // what it is over.
    let title_ink = if flat {
        tokens.palette.text_subtle
    } else {
        tokens.palette.text
    };
    let secondary_ink = tokens.palette.text_subtle;

    let mut width = rect.width();
    let suffix = count.map(|count| {
        let galley = recipe::spaced(
            ui,
            &format!("×{count}"),
            recipe::track_mono(text::XXS),
            secondary_ink,
            recipe::tracking_pts(text::XXS, tracking::WIDE),
        );
        // The count takes its room from the name rather than from the column, so
        // the two never overlap.
        width = (rect.width() - galley.size().x - space::S2).max(0.0);
        galley
    });

    let placed = link::Placed {
        pos: Pos2::new(rect.left(), top),
        galley: link::lay(
            ui,
            row.title,
            &Face {
                font: name,
                tracking: 0.0,
            },
            width,
        ),
    };
    let title_right = placed.rect().right();
    let shown = placed.link(
        ui,
        ui.scope_id().with(("queue-title", row.index)),
        rect,
        row.title_target.filter(|_| live).map(Target::Album),
        title_ink,
        tokens,
    );
    let mut links = Links {
        hovered: shown.hovered,
        page: shown.page,
    };
    if let Some(suffix) = suffix {
        ui.painter().galley(
            Pos2::new(title_right + space::S2, top),
            suffix,
            secondary_ink,
        );
    }
    if artist_h > 0.0 {
        let shown = link::Placed {
            pos: Pos2::new(rect.left(), top + name_h),
            galley: link::lay(
                ui,
                row.artist,
                &Face {
                    font: artist,
                    tracking: 0.0,
                },
                rect.width(),
            ),
        }
        .link(
            ui,
            ui.scope_id().with(("queue-artist", row.index)),
            rect,
            row.artist_target.filter(|_| live).map(Target::Credit),
            secondary_ink,
            tokens,
        );
        links.hovered |= shown.hovered;
        links.page = links.page.or(shown.page);
    }
    links
}

/// The height of one line of `font`.
fn line_height(ui: &Ui, font: &FontId) -> f32 {
    ui.ctx().fonts_mut(|fonts| fonts.row_height(font))
}

#[cfg(test)]
mod tests;
