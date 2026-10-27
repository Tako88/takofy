//! One saved thing, drawn as the library's tile or as one of its rows.
//!
//! The two densities are the same item: a cover, a name and a line saying what
//! the thing is. The grid puts the cover above the words, the list puts it beside
//! them and adds the type and size columns the reference only draws in that
//! density.

use egui::{Pos2, Rect, Sense, Ui, Vec2};

use crate::availability::Control;
use crate::cover::Cover;
use crate::icon::Icon;
use crate::menu::{Anchor, Device, Door};
use crate::sidebar_kind::SidebarKind;
use crate::theme::icon::Size;
use crate::theme::{Tokens, extent, radius, space, stroke, text, tracking};

use super::art;
use super::carry;
use super::context_menu;
use super::controls;
use super::hero;
use super::text as recipe;

/// One item, as either density draws it.
pub struct Media<'a> {
    /// The item's uri: it keys the row and the control inside it.
    pub uri: &'a str,
    pub name: &'a str,
    /// The Type cell: the one word the reference puts there.
    pub kind: &'a str,
    /// The line under the name: what it is and who made it.
    pub sub: &'a str,
    /// The Size cell: empty when the wire named no count.
    pub size: &'a str,
    /// An artist's cover is a circle; everything else is a rounded square.
    pub round: bool,
    /// The sidebar list this item belongs to, so a menu on it is built without
    /// re-reading its uri.
    pub shelf: SidebarKind,
    /// Whether pressing play on this item starts it, or it has no play.
    pub playable: bool,
    /// The item's one picture, which a row always draws.
    pub cover: Cover,
    /// What a tile draws in its art box: the mosaic once every quadrant is held.
    pub picture: hero::Picture,
}

/// The columns a list row solves, left to right, inside its padding.
///
/// The head row solves the same ones, so a column cannot drift from its heading.
#[derive(Debug, Clone, Copy)]
pub struct Columns {
    pub art: Rect,
    pub body: Rect,
    pub kind: Rect,
    pub size: Rect,
    pub more: Rect,
}

/// The rects of one row's five columns.
pub fn columns(rect: Rect) -> Columns {
    let inner = Rect::from_min_max(
        Pos2::new(rect.left() + space::S3, rect.top()),
        Pos2::new(rect.right() - space::S3, rect.bottom()),
    );
    let art = Rect::from_center_size(
        Pos2::new(inner.left() + extent::LIBRARY_ART / 2.0, rect.center().y),
        Vec2::splat(extent::LIBRARY_ART),
    );
    let more = Rect::from_center_size(
        Pos2::new(inner.right() - extent::BUTTON / 2.0, rect.center().y),
        Vec2::splat(extent::BUTTON),
    );
    let size = Rect::from_min_size(
        Pos2::new(more.left() - space::S4 - extent::SIZE_W, rect.top()),
        Vec2::new(extent::SIZE_W, rect.height()),
    );
    let kind = Rect::from_min_size(
        Pos2::new(size.left() - space::S4 - extent::TYPE_W, rect.top()),
        Vec2::new(extent::TYPE_W, rect.height()),
    );
    let body_left = art.right() + space::S4;
    let body = Rect::from_min_max(
        Pos2::new(body_left, rect.top()),
        Pos2::new((kind.left() - space::S4).max(body_left), rect.bottom()),
    );
    Columns {
        art,
        body,
        kind,
        size,
        more,
    }
}

/// The height a tile takes beyond its width.
///
/// The art is square and takes the tile's width less its padding, so the height
/// beyond the width is one gap, the name and the subline. The grid is told this
/// rather than working it out: the grid lays out boxes, and the tile decides
/// what they hold.
pub fn extra_height(ui: &Ui) -> f32 {
    space::S2 + row_height(ui, &recipe::card_name()) + row_height(ui, &recipe::card_kind())
}

/// One tile: the cover over the name and the subline, raised when reached.
///
/// The play sits in the tile's corner and is drawn only while the pointer is on
/// the tile or the keyboard is on its play, so it covers no word the user came to
/// read.
pub fn tile(ui: &mut Ui, rect: Rect, media: &Media<'_>, tokens: &Tokens) -> Option<Press> {
    let response = ui.interact(
        rect,
        ui.scope_id().with(("library-media", media.uri)),
        Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, spoken(media)));
    if response.clicked() {
        // The keyboard follows the pointer: Tab continues from the tile that was
        // picked.
        response.request_focus();
    }
    // Read before the play draws: a play that takes the keyboard would otherwise
    // be revealed on the frame it is pressed and hidden on the next.
    let play_focused = ui.memory(|memory| memory.focused())
        == Some(controls::slot_id(ui, Control::CardPlay, media.uri));
    // One predicate for the wash and the reveal: a live child under the pointer
    // makes `hovered` false, and two predicates would flicker apart.
    let over = response.hovered() || ui.rect_contains_pointer(rect);
    if over || response.has_focus() {
        ui.painter()
            .rect_filled(rect, radius::MD, tokens.palette.surface_raised);
        ui.painter().rect_stroke(
            rect,
            radius::MD,
            egui::Stroke::new(stroke::HAIRLINE, tokens.palette.border),
            egui::StrokeKind::Inside,
        );
    }
    controls::focus_ring(ui, &response, rect, radius::MD, tokens);

    let inner = rect.shrink(space::S2);
    let art = Rect::from_min_size(inner.min, Vec2::splat(inner.width()));
    let radius = if media.round {
        radius::PILL
    } else {
        radius::SM
    };
    match media.picture {
        hero::Picture::Mosaic(covers) => art::mosaic(
            ui.painter(),
            art,
            radius,
            tokens.palette.border,
            covers,
            tokens,
        ),
        hero::Picture::Cover => art::tile(
            ui.painter(),
            art,
            radius,
            tokens.palette.border,
            media.cover,
            tokens,
        ),
    }

    let mut top = art.bottom() + space::S2;
    let name = recipe::line(
        ui,
        media.name,
        recipe::card_name(),
        tokens.palette.text,
        0.0,
        inner.width(),
    );
    ui.painter()
        .galley(Pos2::new(inner.left(), top), name, tokens.palette.text);
    top += row_height(ui, &recipe::card_name());
    let sub = recipe::line(
        ui,
        media.sub,
        recipe::card_kind(),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XXS, tracking::SLIGHT),
        inner.width(),
    );
    ui.painter().galley(
        Pos2::new(inner.left(), top),
        sub,
        tokens.palette.text_subtle,
    );

    let mut pressed = None;
    if media.playable && (over || response.has_focus() || play_focused) {
        let slot = controls::play_tile_keyed(
            ui,
            tokens,
            Control::CardPlay,
            media.uri,
            tile_play_rect(rect),
            Icon::Play,
            &format!("Play {}", media.name),
            !carry::carrying(ui.ctx()),
        );
        if slot.response.clicked() {
            pressed = Some(Press::Play);
        }
    }
    // A carried block makes the play inert, so egui reports the tile as the
    // hovered widget on the release frame and the play as not hovered at all.
    // Where the press landed is what tells them apart.
    let on_play = media.playable
        && response
            .interact_pointer_pos()
            .is_some_and(|at| tile_play_rect(rect).contains(at));

    pressed.or((response.clicked() && !on_play).then_some(Press::Open))
}

/// The height of the head's one line of text, before its padding.
pub fn head_height(ui: &Ui) -> f32 {
    row_height(ui, &head_font())
}

/// The head's font: the reference's tracked mono capitals.
fn head_font() -> egui::FontId {
    egui::FontId::new(
        text::XXS,
        crate::theme::fonts::mono(crate::theme::MonoWeight::Medium),
    )
}

/// The head over the list density: the columns' names, and a rule under them.
///
/// The first and last cells carry no heading, as the reference leaves them.
pub fn head(ui: &mut Ui, rect: Rect, tokens: &Tokens) {
    let columns = columns(rect);
    for (rect, label) in [
        (columns.body, "Title"),
        (columns.kind, "Type"),
        (columns.size, "Size"),
    ] {
        let line = recipe::spaced(
            ui,
            &label.to_uppercase(),
            head_font(),
            tokens.palette.text_subtle,
            recipe::tracking_pts(text::XXS, tracking::CAPS),
        );
        ui.painter().galley(
            Pos2::new(rect.left(), rect.center().y - line.size().y / 2.0),
            line,
            tokens.palette.text_subtle,
        );
    }

    ui.painter().rect_filled(
        Rect::from_min_size(
            Pos2::new(columns.art.left(), rect.bottom() - stroke::HAIRLINE),
            Vec2::new(columns.more.right() - columns.art.left(), stroke::HAIRLINE),
        ),
        0.0,
        tokens.palette.border,
    );
}

/// What one list row reports: the row itself was chosen, its play was pressed,
/// or a menu was asked for.
///
/// A row has three things to say, so it reports one of them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Press {
    /// The row's own body was chosen.
    Open,
    /// The play over the cover was pressed.
    Play,
    /// One of the row's doors asked for its menu.
    Menu(Door),
}

/// One list row: the cover, the words, the type, the size and the more button.
///
/// The `⋯` and the play are their own controls, so a press on either never falls
/// through to the row.
pub fn row(ui: &mut Ui, rect: Rect, media: &Media<'_>, tokens: &Tokens) -> Option<Press> {
    let columns = columns(rect);
    let response = ui.interact(
        rect,
        ui.scope_id().with(("library-media-row", media.uri)),
        Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, spoken(media)));
    if response.clicked() {
        // The keyboard follows the pointer: Tab continues from the row that was
        // picked.
        response.request_focus();
    }
    let live = !carry::carrying(ui.ctx());
    // Read before the play draws, as the tile's is.
    let play_focused = ui.memory(|memory| memory.focused())
        == Some(controls::slot_id(ui, Control::CardPlay, media.uri));
    let over = response.hovered() || ui.rect_contains_pointer(rect);
    if over {
        ui.painter()
            .rect_filled(rect, radius::SM, tokens.palette.row_hover);
    }
    controls::focus_ring(ui, &response, rect, radius::SM, tokens);

    let radius = if media.round {
        radius::PILL
    } else {
        radius::XS
    };
    // A row keeps the single cover: the four-up is unreadable at this size.
    art::tile(
        ui.painter(),
        columns.art,
        radius,
        tokens.palette.border,
        media.cover,
        tokens,
    );

    let name = recipe::line(
        ui,
        media.name,
        recipe::card_name(),
        tokens.palette.text,
        0.0,
        columns.body.width(),
    );
    let sub = recipe::line(
        ui,
        media.sub,
        recipe::card_kind(),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XXS, tracking::SLIGHT),
        columns.body.width(),
    );
    let top = rect.center().y - (name.size().y + sub.size().y) / 2.0;
    ui.painter().galley(
        Pos2::new(columns.body.left(), top),
        name.clone(),
        tokens.palette.text,
    );
    ui.painter().galley(
        Pos2::new(columns.body.left(), top + name.size().y),
        sub,
        tokens.palette.text_subtle,
    );

    for (rect, label) in [(columns.kind, media.kind), (columns.size, media.size)] {
        if label.is_empty() {
            continue;
        }
        let line = recipe::spaced(
            ui,
            &label.to_uppercase(),
            egui::FontId::new(
                text::XXS,
                crate::theme::fonts::mono(crate::theme::MonoWeight::Regular),
            ),
            tokens.palette.text_subtle,
            recipe::tracking_pts(text::XXS, tracking::WIDE),
        );
        ui.painter().galley(
            Pos2::new(rect.left(), rect.center().y - line.size().y / 2.0),
            line,
            tokens.palette.text_subtle,
        );
    }

    // The menu's door, drawn where the `⋯` sits. It says its menu is up for
    // either opener: its own press, or a right-click and the keyboard from the
    // row.
    let id = controls::slot_id(ui, Control::LibraryMore, media.uri);
    let opener = context_menu::open_opener(ui.ctx());
    let open = opener.is_some_and(|opener| opener == id || opener == response.id);
    let more = controls::icon_button_open(
        ui,
        tokens,
        Control::LibraryMore,
        media.uri,
        columns.more,
        Icon::More,
        Size::MD,
        &format!("More options for {}", media.name),
        live,
        open,
    );
    let _ = ui.ctx().accesskit_node_builder(more.response.id, |node| {
        node.set_expanded(open);
        node.set_has_popup(egui::accesskit::HasPopup::Menu);
    });

    let on_more = response
        .interact_pointer_pos()
        .is_some_and(|at| columns.more.contains(at));

    // The play over the cover: the grid's own control, keyed by the same uri.
    let mut pressed = None;
    if media.playable && (over || response.has_focus() || play_focused) {
        let slot = controls::play_tile_keyed(
            ui,
            tokens,
            Control::CardPlay,
            media.uri,
            row_play_rect(rect),
            Icon::Play,
            &format!("Play {}", media.name),
            live,
        );
        if slot.response.clicked() {
            pressed = Some(Press::Play);
        }
    }

    // The three doors report a menu before the row reports itself: a menu opens
    // on the row the user pointed at, and never also selects or opens it.
    if live
        && let Some(door) = context_menu::button_door(ui, &more, columns.more, id)
            .or_else(|| key_door(ui, &response, &more, columns.more))
            .or_else(|| context_menu::pointer_door(ui, rect, response.id, columns.more))
    {
        return Some(Press::Menu(door));
    }

    if pressed.is_some() {
        return pressed;
    }
    let on_play = media.playable
        && response
            .interact_pointer_pos()
            .is_some_and(|at| row_play_rect(rect).contains(at));

    (response.clicked() && !on_more && !on_play).then_some(Press::Open)
}

/// The keyboard's door: the context-menu key or Shift+F10, on the focused row or
/// its focused `⋯`.
///
/// A repeated press while the menu is already open posts the same door again,
/// re-anchoring the menu rather than closing it. The track row's door does the
/// same, so the two lists answer the key alike.
fn key_door(ui: &Ui, row: &egui::Response, more: &controls::Slot, anchor: Rect) -> Option<Door> {
    let (row_focused, more_focused) = (row.has_focus(), more.response.has_focus());
    if !row_focused && !more_focused {
        return None;
    }
    context_menu::menu_key_pressed(ui).then_some(Door {
        anchor: Anchor::Button(anchor),
        opener: if more_focused {
            more.response.id
        } else {
            row.id
        },
        device: Device::Keyboard,
    })
}

/// What a screen reader says for one item: its name and what it is.
fn spoken(media: &Media<'_>) -> String {
    match media.size.is_empty() {
        true => format!("{} {}", media.name, media.kind.to_lowercase()),
        false => format!(
            "{} {}, {}",
            media.name,
            media.kind.to_lowercase(),
            media.size
        ),
    }
}

/// The height of one line of `font`, measured as the painter lays it out.
///
/// `row_height` is the font's leading, which is a little shorter than a mono
/// galley's own height; the tile reserves what is actually painted.
fn row_height(ui: &Ui, font: &egui::FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(" ".to_owned(), font.clone(), crate::theme::art::TINT)
        .size()
        .y
}

/// Where a tile's cover sits, so a test and the painter cannot disagree.
pub fn art_rect(rect: Rect) -> Rect {
    let inner = rect.shrink(space::S2);
    Rect::from_min_size(inner.min, Vec2::splat(inner.width()))
}

/// Where a tile's play sits: `space::S2` in from the tile's bottom-right
/// corner.
pub fn tile_play_rect(rect: Rect) -> Rect {
    Rect::from_min_size(
        Pos2::new(
            rect.right() - space::S2 - extent::CARD_PLAY,
            rect.bottom() - space::S2 - extent::CARD_PLAY,
        ),
        Vec2::splat(extent::CARD_PLAY),
    )
}

/// Where a row's play sits: centred on the cover, so no column moves for it.
pub fn row_play_rect(rect: Rect) -> Rect {
    Rect::from_center_size(columns(rect).art.center(), Vec2::splat(extent::CARD_PLAY))
}

#[cfg(test)]
mod tests;
