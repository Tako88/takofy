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

/// The column widths the reference gives the list density.
pub const TYPE_W: f32 = 144.0;
pub const SIZE_W: f32 = 80.0;

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
        Pos2::new(more.left() - space::S4 - SIZE_W, rect.top()),
        Vec2::new(SIZE_W, rect.height()),
    );
    let kind = Rect::from_min_size(
        Pos2::new(size.left() - space::S4 - TYPE_W, rect.top()),
        Vec2::new(TYPE_W, rect.height()),
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
        .layout_no_wrap(" ".to_owned(), font.clone(), egui::Color32::WHITE)
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
mod tests {
    use super::*;
    use crate::theme::{fonts, style};
    use crate::view::test_support::{click_events, rings_in, secondary_click_events};
    use egui::{Color32, Event, Key, Modifiers};

    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&Tokens::DARK));
        ctx
    }

    fn media<'a>(uri: &'a str, round: bool, size: &'a str) -> Media<'a> {
        Media {
            uri,
            name: "Slow Tide",
            kind: "Album",
            sub: "Album · Nils Aker",
            size,
            round,
            shelf: SidebarKind::Albums,
            playable: true,
            cover: Cover::Missing,
            picture: hero::Picture::Cover,
        }
    }

    /// A card holding `cover` as the single picture and `picture` as what it tiles.
    fn pictured<'a>(uri: &'a str, cover: Cover, picture: hero::Picture) -> Media<'a> {
        Media {
            cover,
            picture,
            ..media(uri, false, "")
        }
    }

    /// A cover the store has uploaded, so the painter can size it.
    ///
    /// The handle is handed back because dropping it frees the texture, and the
    /// box would then draw a block rather than a picture.
    fn uploaded(ctx: &egui::Context, name: &str) -> (egui::TextureHandle, Cover) {
        let texture = ctx.load_texture(
            name,
            egui::ColorImage::filled([4, 4], Color32::WHITE),
            crate::theme::art::TEXTURE,
        );
        // A load is queued and only reaches the texture manager in a pass.
        let mut output = ctx.run_ui(Default::default(), |_| {});
        output.textures_delta.clear();
        let cover = Cover::Ready(texture.id());
        (texture, cover)
    }

    /// The textured rects a pass painted inside `area`, in paint order.
    fn textured(output: &egui::FullOutput, area: Rect) -> Vec<Rect> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if shape.brush.is_some() && area.contains(shape.rect.center()) =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .collect()
    }

    /// One pass over the library's own card size, reporting the tile's rect.
    fn card_pass(ctx: &egui::Context, media: &Media<'_>) -> (Rect, egui::FullOutput) {
        let tokens = Tokens::DARK;
        let mut card = Rect::ZERO;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
                ..Default::default()
            },
            |ui| {
                card = Rect::from_min_size(Pos2::ZERO, Vec2::new(176.0, 176.0 + extra_height(ui)));
                let _ = tile(ui, card, media, &tokens);
            },
        );
        output.textures_delta.clear();
        (card, output)
    }

    fn painted_text(output: &egui::FullOutput) -> Vec<String> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_tile_is_its_square_cover_plus_the_two_lines() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let mut measured = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 400.0))),
                ..Default::default()
            },
            |ui| {
                let width = 176.0;
                let rect =
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(width, width + extra_height(ui)));
                measured = Some(rect);
                let _ = tile(
                    ui,
                    rect,
                    &media("spotify:album:a", false, "11 tracks"),
                    &tokens,
                );
            },
        );
        output.textures_delta.clear();

        let rect = measured.expect("the tile is laid out");
        let art_rect = art_rect(rect);
        assert!(
            (art_rect.width() - art_rect.height()).abs() < 0.01,
            "the cover is square"
        );
        assert_eq!(art_rect.width(), 176.0 - 2.0 * space::S2);

        // The subline is the last thing painted, and it stays inside the tile.
        let sub = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == "Album · Nils Aker" => {
                    Some(Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
            .expect("the tile paints its subline");
        assert!(
            sub.bottom() <= rect.bottom() - space::S2 + 0.01,
            "the subline ends above the tile's padding: {sub:?} in {rect:?}"
        );
        assert!(painted_text(&output).contains(&"Slow Tide".to_owned()));
    }

    #[test]
    fn the_list_head_names_the_three_columns_that_carry_one() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 200.0))),
                ..Default::default()
            },
            |ui| {
                let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 32.0));
                head(ui, rect, &tokens);
            },
        );
        output.textures_delta.clear();

        let texts = painted_text(&output);
        assert_eq!(texts, vec!["TITLE", "TYPE", "SIZE"]);
    }

    #[test]
    fn a_row_draws_its_five_columns_and_a_round_cover_for_an_artist() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 200.0))),
                ..Default::default()
            },
            |ui| {
                let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, extent::ROW_H));
                let mut artist = media("spotify:artist:a", true, "");
                artist.kind = "Artist";
                artist.sub = "Artist";
                let _ = row(ui, rect, &artist, &tokens);
            },
        );
        output.textures_delta.clear();

        let texts = painted_text(&output);
        assert!(texts.contains(&"Slow Tide".to_owned()));
        assert!(
            texts.contains(&"ARTIST".to_owned()),
            "the type cell is a capitalised column: {texts:?}"
        );
        assert!(
            texts.contains(&"Artist".to_owned()),
            "the subline keeps the reference's lower case: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text.contains("0 ")),
            "an artist's size cell draws nothing: {texts:?}"
        );
    }

    /// One tile pass, returning the tile's rect and what it painted.
    fn tile_frame(ctx: &egui::Context, events: Vec<Event>) -> (Rect, egui::FullOutput) {
        let tokens = Tokens::DARK;
        let mut rect = Rect::ZERO;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 400.0))),
                events,
                ..Default::default()
            },
            |ui| {
                crate::view::focus::begin_frame(ui.ctx());
                let width = 176.0;
                rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, width + extra_height(ui)));
                let _ = tile(
                    ui,
                    rect,
                    &media("spotify:album:a", false, "11 tracks"),
                    &tokens,
                );
            },
        );
        output.textures_delta.clear();
        (rect, output)
    }

    /// One list-row pass, returning the row's rect and what it painted.
    fn row_frame(ctx: &egui::Context, events: Vec<Event>) -> (Rect, egui::FullOutput) {
        let tokens = Tokens::DARK;
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, extent::ROW_H));
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 200.0))),
                events,
                ..Default::default()
            },
            |ui| {
                crate::view::focus::begin_frame(ui.ctx());
                let _ = row(
                    ui,
                    rect,
                    &media("spotify:album:a", false, "11 tracks"),
                    &tokens,
                );
            },
        );
        output.textures_delta.clear();
        (rect, output)
    }

    #[test]
    fn a_clicked_tile_wears_no_ring() {
        let ctx = context();
        let (rect, _) = tile_frame(&ctx, Vec::new());
        for event in click_events(rect.center()) {
            let _ = tile_frame(&ctx, vec![event]);
        }
        let (_, output) = tile_frame(&ctx, Vec::new());
        assert!(
            ctx.memory(|memory| memory.focused()).is_some(),
            "the click took the keyboard"
        );
        assert!(
            rings_in(&output).is_empty(),
            "a clicked tile wears no ring: {:?}",
            rings_in(&output)
        );
    }

    #[test]
    fn a_clicked_list_row_wears_no_ring() {
        let ctx = context();
        let (rect, _) = row_frame(&ctx, Vec::new());
        let at = Pos2::new(rect.width() / 2.0, rect.height() / 2.0);
        for event in click_events(at) {
            let _ = row_frame(&ctx, vec![event]);
        }
        let (_, output) = row_frame(&ctx, Vec::new());
        assert!(
            ctx.memory(|memory| memory.focused()).is_some(),
            "the click took the keyboard"
        );
        assert!(
            rings_in(&output).is_empty(),
            "a clicked list row wears no ring: {:?}",
            rings_in(&output)
        );
    }

    /// The fills one list row paints inside its own rect, for a pointer at `at`.
    fn row_fills(at: Option<Pos2>) -> Vec<Color32> {
        let ctx = context();
        let tokens = Tokens::DARK;
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, extent::ROW_H));
        let mut fills = Vec::new();
        for _ in 0..2 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 200.0))),
                    events: at.map(Event::PointerMoved).into_iter().collect(),
                    ..Default::default()
                },
                |ui| {
                    let _ = row(
                        ui,
                        rect,
                        &media("spotify:album:a", false, "11 tracks"),
                        &tokens,
                    );
                },
            );
            output.textures_delta.clear();
            fills = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::epaint::Shape::Rect(shape) if shape.rect == rect => Some(shape.fill),
                    _ => None,
                })
                .collect();
        }
        fills
    }

    #[test]
    fn a_hovered_media_row_washes_with_row_hover() {
        let tokens = Tokens::DARK;
        assert_eq!(
            row_fills(Some(Pos2::new(400.0, extent::ROW_H / 2.0))),
            vec![tokens.palette.row_hover],
            "a hovered list row washed with something other than the row hover"
        );
        assert!(
            row_fills(None).is_empty(),
            "a list row with the pointer away washed"
        );
    }

    /// The rect the pointer tests draw their row into.
    fn row_rect() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, extent::ROW_H))
    }

    fn input_with(events: Vec<Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 200.0))),
            events,
            ..Default::default()
        }
    }

    /// One pass per event, collecting what the row reported each time.
    fn run(ctx: &egui::Context, item: &Media<'_>, events: &[Event]) -> Vec<Option<Press>> {
        let tokens = Tokens::DARK;
        let rect = row_rect();
        let mut output = ctx.run_ui(input_with(Vec::new()), |ui| {
            let _ = row(ui, rect, item, &tokens);
        });
        output.textures_delta.clear();

        let mut answers = Vec::new();
        for event in events {
            let mut answer = None;
            let mut output = ctx.run_ui(input_with(vec![event.clone()]), |ui| {
                answer = row(ui, rect, item, &tokens);
            });
            output.textures_delta.clear();
            answers.push(answer);
        }
        answers
    }

    /// The one menu door a run reported.
    fn door(answers: &[Option<Press>]) -> Option<Door> {
        answers.iter().flatten().find_map(|press| match press {
            Press::Menu(door) => Some(*door),
            Press::Open | Press::Play => None,
        })
    }

    /// Whether the run reported a play.
    fn played(answers: &[Option<Press>]) -> bool {
        answers.iter().flatten().any(|press| *press == Press::Play)
    }

    /// Whether the run reported the row itself.
    fn opened(answers: &[Option<Press>]) -> bool {
        answers.iter().flatten().any(|press| *press == Press::Open)
    }

    /// The row's own widget id, which the pointer and key doors open from.
    fn row_id(ctx: &egui::Context, item: &Media<'_>) -> egui::Id {
        let mut id = None;
        let mut output = ctx.run_ui(input_with(Vec::new()), |ui| {
            id = Some(ui.scope_id().with(("library-media-row", item.uri)));
            let _ = row(ui, row_rect(), item, &Tokens::DARK);
        });
        output.textures_delta.clear();
        id.expect("a frame ran")
    }

    /// The `⋯`'s own slot id, which its own press opens from.
    fn more_id(ctx: &egui::Context, item: &Media<'_>) -> egui::Id {
        let mut id = None;
        let mut output = ctx.run_ui(input_with(Vec::new()), |ui| {
            id = Some(controls::slot_id(ui, Control::LibraryMore, item.uri));
            let _ = row(ui, row_rect(), item, &Tokens::DARK);
        });
        output.textures_delta.clear();
        id.expect("a frame ran")
    }

    #[test]
    fn a_press_on_the_more_opens_a_button_door_and_not_the_row() {
        let ctx = context();
        let item = media("spotify:album:a", false, "11 tracks");
        let more = columns(row_rect()).more;
        let answers = run(&ctx, &item, &click_events(more.center()));

        let door = door(&answers).expect("the `⋯` opened a menu");
        assert_eq!(
            door.anchor,
            Anchor::Button(more),
            "the menu hangs off the `⋯`"
        );
        assert_eq!(door.device, Device::Pointer);
        assert_eq!(door.opener, more_id(&ctx, &item));
        assert!(
            !opened(&answers),
            "the press on the `⋯` also opened the row: {answers:?}"
        );
    }

    #[test]
    fn a_press_on_the_body_opens_the_row_and_reports_no_door() {
        let ctx = context();
        let item = media("spotify:album:a", false, "11 tracks");
        let body = columns(row_rect()).body;
        let answers = run(&ctx, &item, &click_events(body.center()));

        assert!(opened(&answers), "the body opened nothing: {answers:?}");
        assert_eq!(door(&answers), None, "the body opened a menu too");
    }

    #[test]
    fn a_right_click_on_the_body_opens_at_the_pointer_from_the_row() {
        let ctx = context();
        let item = media("spotify:album:a", false, "11 tracks");
        let at = columns(row_rect()).body.center();
        let answers = run(&ctx, &item, &secondary_click_events(at));

        let menu = door(&answers).expect("a right-click opened no menu");
        let Anchor::Pointer(where_) = menu.anchor else {
            panic!("a right-click anchored to the `⋯`: {:?}", menu.anchor);
        };
        assert!(
            (where_ - at).length() < 0.01,
            "a right-click at {at:?} opened at {where_:?}"
        );
        assert_eq!(menu.device, Device::Pointer);
        assert_eq!(menu.opener, row_id(&ctx, &item));
        assert!(!opened(&answers), "a right-click also opened the row");
    }

    #[test]
    fn a_right_click_on_the_more_opens_at_the_pointer_from_the_row() {
        let ctx = context();
        let item = media("spotify:album:a", false, "11 tracks");
        let at = columns(row_rect()).more.center();
        let answers = run(&ctx, &item, &secondary_click_events(at));

        let menu = door(&answers).expect("a right-click on the `⋯` opened no menu");
        let Anchor::Pointer(where_) = menu.anchor else {
            panic!("a right-click on the `⋯` anchored to it: {:?}", menu.anchor);
        };
        assert!((where_ - at).length() < 0.01);
        assert_eq!(
            menu.opener,
            row_id(&ctx, &item),
            "the pointer door on the `⋯` names the row"
        );
        assert_ne!(menu.opener, more_id(&ctx, &item));
    }

    #[test]
    fn enter_on_the_focused_more_opens_with_the_keyboard_and_a_held_enter_does_not() {
        let ctx = context();
        let item = media("spotify:album:a", false, "11 tracks");
        let more = columns(row_rect()).more;
        // The click gives the `⋯` the keyboard, as a pointer press does.
        let first = run(&ctx, &item, &click_events(more.center()));
        assert_eq!(
            door(&first).map(|door| door.device),
            Some(Device::Pointer),
            "the click did not open the menu"
        );

        let enter = |pressed: bool| Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let answers = run(&ctx, &item, &[enter(true)]);
        let menu = door(&answers).expect("Enter on the focused `⋯` opened no menu");
        assert_eq!(menu.device, Device::Keyboard);
        assert_eq!(menu.opener, more_id(&ctx, &item));
        assert_eq!(menu.anchor, Anchor::Button(more));

        // Still held: egui marks a press with no release before it a repeat.
        let held = run(&ctx, &item, &[enter(true)]);
        assert_eq!(door(&held), None, "a held Enter reopened the menu");
        assert!(
            held.iter().flatten().next().is_none(),
            "a held Enter reported {held:?}"
        );
    }

    #[test]
    fn the_menu_key_opens_from_either_focus_and_nothing_when_none_holds_it() {
        let press = |key: Key, modifiers: Modifiers| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };

        // The body click is what gives the row the keyboard.
        let ctx = context();
        let item = media("spotify:album:a", false, "11 tracks");
        let first = run(
            &ctx,
            &item,
            &click_events(columns(row_rect()).body.center()),
        );
        assert!(opened(&first), "the row never took the keyboard: {first:?}");
        let from_row = run(&ctx, &item, &[press(Key::Menu, Modifiers::NONE)]);
        let menu = door(&from_row).expect("the menu key on the focused row opened nothing");
        assert_eq!(menu.device, Device::Keyboard);
        assert_eq!(menu.opener, row_id(&ctx, &item));
        let shift_f10 = run(&ctx, &item, &[press(Key::F10, Modifiers::SHIFT)]);
        assert_eq!(
            door(&shift_f10).map(|door| door.device),
            Some(Device::Keyboard),
            "Shift+F10 on the focused row opened nothing"
        );

        // The `⋯` takes the keyboard from its own press.
        let more = context();
        let _ = run(
            &more,
            &item,
            &click_events(columns(row_rect()).more.center()),
        );
        let from_more = run(&more, &item, &[press(Key::Menu, Modifiers::NONE)]);
        let menu = door(&from_more).expect("the menu key on the focused `⋯` opened nothing");
        assert_eq!(menu.opener, more_id(&more, &item));

        // Nothing focused: the key is not the row's to take.
        let bare = context();
        let answers = run(&bare, &item, &[press(Key::Menu, Modifiers::NONE)]);
        assert_eq!(door(&answers), None, "an unfocused row took the menu key");
    }

    #[test]
    fn a_press_on_the_more_whose_menu_is_open_posts_no_door_and_opens_nothing() {
        let ctx = context();
        let item = media("spotify:album:a", false, "11 tracks");
        context_menu::note_open(&ctx, Some(more_id(&ctx, &item)));
        let answers = run(
            &ctx,
            &item,
            &click_events(columns(row_rect()).more.center()),
        );

        assert_eq!(
            door(&answers),
            None,
            "the open `⋯` posted a request instead of closing"
        );
        assert!(!opened(&answers), "closing the menu also opened the row");
    }

    #[test]
    fn a_carried_row_has_a_waiting_more_and_opens_nothing() {
        let ctx = context();
        let item = media("spotify:album:a", false, "11 tracks");
        egui::DragAndDrop::set_payload(&ctx, ());
        let answers = run(
            &ctx,
            &item,
            &click_events(columns(row_rect()).more.center()),
        );
        egui::DragAndDrop::clear_payload(&ctx);

        assert_eq!(door(&answers), None, "a carried row opened a menu");
        assert!(!opened(&answers), "a carried row's `⋯` opened the row");
    }

    #[test]
    fn the_columns_do_not_overlap_and_the_more_button_is_last() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, extent::ROW_H));
        let columns = columns(rect);
        assert!(columns.art.right() <= columns.body.left());
        assert!(columns.body.right() <= columns.kind.left());
        assert!(columns.kind.right() <= columns.size.left());
        assert!(columns.size.right() <= columns.more.left());
        assert!(
            (columns.kind.width() - TYPE_W).abs() < 0.01
                && (columns.size.width() - SIZE_W).abs() < 0.01
        );
        assert!(rect.contains_rect(columns.more));
    }

    /// The tile's rect at the standard corner, and what its pass reported.
    fn tile_pass(
        ctx: &egui::Context,
        item: &Media<'_>,
        events: Vec<Event>,
    ) -> (Rect, Option<Press>, egui::FullOutput) {
        let tokens = Tokens::DARK;
        let mut rect = Rect::ZERO;
        let mut pressed = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 400.0))),
                events,
                ..Default::default()
            },
            |ui| {
                crate::view::focus::begin_frame(ui.ctx());
                let width = 176.0;
                rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, width + extra_height(ui)));
                pressed = tile(ui, rect, item, &tokens);
            },
        );
        output.textures_delta.clear();
        (rect, pressed, output)
    }

    /// One pass per event, collecting what the tile reported each time.
    fn tile_run(ctx: &egui::Context, item: &Media<'_>, events: &[Event]) -> Vec<Option<Press>> {
        let _ = tile_pass(ctx, item, Vec::new());
        events
            .iter()
            .map(|event| tile_pass(ctx, item, vec![event.clone()]).1)
            .collect()
    }

    /// The tile's last pass with the pointer resting at `at`.
    fn tile_rest(ctx: &egui::Context, item: &Media<'_>, at: Pos2) -> (Rect, egui::FullOutput) {
        let mut last = tile_pass(ctx, item, Vec::new());
        for _ in 0..2 {
            last = tile_pass(ctx, item, vec![Event::PointerMoved(at)]);
        }
        (last.0, last.2)
    }

    /// Whether a frame painted the play tile in `rect`.
    ///
    /// The tile is the only thing that paints a whole rect there: its hover eases
    /// the fill between two tones, so the fill itself is not the marker.
    fn play_painted(output: &egui::FullOutput, rect: Rect) -> bool {
        output.shapes.iter().any(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape) => shape.rect == rect,
            _ => false,
        })
    }

    /// Whether a frame painted the tile's raised wash.
    fn washed(output: &egui::FullOutput, rect: Rect) -> bool {
        output.shapes.iter().any(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape) => {
                shape.rect == rect && shape.fill == Tokens::DARK.palette.surface_raised
            }
            _ => false,
        })
    }

    #[test]
    fn the_tile_play_rect_is_the_cards_corner() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(176.0, 240.0));
        let play = tile_play_rect(rect);

        assert_eq!(play.size(), Vec2::splat(extent::CARD_PLAY));
        assert_eq!(play.right(), rect.right() - space::S2);
        assert_eq!(play.bottom(), rect.bottom() - space::S2);
        assert!(rect.contains_rect(play), "the play hangs off the card");
    }

    #[test]
    fn a_hovered_playable_tile_draws_its_play_and_an_unplayable_one_does_not() {
        let ctx = context();
        let item = media("spotify:album:a", false, "11 tracks");
        let at = Pos2::new(88.0, 88.0);
        let (rect, output) = tile_rest(&ctx, &item, at);
        assert!(
            play_painted(&output, tile_play_rect(rect)),
            "a hovered playable tile drew no play"
        );

        let bare = Media {
            playable: false,
            ..media("spotify:artist:a", true, "")
        };
        let (rect, output) = tile_rest(&ctx, &bare, at);
        assert!(
            !play_painted(&output, tile_play_rect(rect)),
            "an item with no play drew one"
        );
    }

    #[test]
    fn a_press_on_the_tiles_play_reports_play_and_not_open() {
        let ctx = context();
        let item = media("spotify:album:a", false, "11 tracks");
        let (rect, _, _) = tile_pass(&ctx, &item, Vec::new());
        let play = tile_play_rect(rect);
        let answers = tile_run(&ctx, &item, &click_events(play.center()));

        assert_eq!(
            answers,
            vec![None, None, Some(Press::Play)],
            "a press on the play reported something else: {answers:?}"
        );
    }

    #[test]
    fn a_press_on_the_tile_body_still_reports_open() {
        let ctx = context();
        let item = media("spotify:album:a", false, "11 tracks");
        let (rect, _, _) = tile_pass(&ctx, &item, Vec::new());
        let body = art_rect(rect).center();
        let answers = tile_run(&ctx, &item, &click_events(body));

        assert_eq!(
            answers,
            vec![None, None, Some(Press::Open)],
            "the tile body stopped opening the item: {answers:?}"
        );
    }

    #[test]
    fn the_play_tile_stays_drawn_while_the_pointer_is_on_it() {
        let ctx = context();
        let item = media("spotify:album:a", false, "11 tracks");
        let (rect, _) = tile_rest(&ctx, &item, Pos2::new(88.0, 88.0));
        let (rect, output) = tile_rest(&ctx, &item, tile_play_rect(rect).center());

        assert!(
            play_painted(&output, tile_play_rect(rect)),
            "the pointer on the play took the play away"
        );
    }

    #[test]
    fn the_wash_stays_drawn_while_the_pointer_is_on_the_play() {
        let ctx = context();
        let item = media("spotify:album:a", false, "11 tracks");
        let (rect, _) = tile_rest(&ctx, &item, Pos2::new(88.0, 88.0));
        assert!(
            washed(&tile_rest(&ctx, &item, Pos2::new(88.0, 88.0)).1, rect),
            "a hovered tile away from its play washed with something else"
        );

        let (rect, output) = tile_rest(&ctx, &item, tile_play_rect(rect).center());
        assert!(
            washed(&output, rect),
            "the live play child took the tile's wash away"
        );
        assert_eq!(
            row_fills(Some(row_play_rect(row_rect()).center())),
            vec![Tokens::DARK.palette.row_hover],
            "the live play child took the row's wash away"
        );
    }

    #[test]
    fn tab_from_a_tile_reaches_its_play_and_keeps_it_drawn() {
        let ctx = context();
        let item = media("spotify:album:a", false, "11 tracks");
        let mut ids = None;
        let mut output = ctx.run_ui(input_with(Vec::new()), |ui| {
            crate::view::focus::begin_frame(ui.ctx());
            let width = 176.0;
            let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, width + extra_height(ui)));
            let _ = tile(ui, rect, &item, &Tokens::DARK);
            ids = Some((
                ui.scope_id().with(("library-media", item.uri)),
                controls::slot_id(ui, Control::CardPlay, item.uri),
            ));
        });
        output.textures_delta.clear();
        let (tile_id, play_id) = ids.expect("a pass ran");

        // The tile takes the keyboard, as its own click does.
        let (rect, _, _) = tile_pass(&ctx, &item, Vec::new());
        for event in click_events(art_rect(rect).center()) {
            let _ = tile_pass(&ctx, &item, vec![event]);
        }
        assert_eq!(
            ctx.memory(|memory| memory.focused()),
            Some(tile_id),
            "the click never took the keyboard"
        );

        let [press, release] = crate::view::test_support::tab();
        let _ = tile_pass(&ctx, &item, vec![press]);
        let (rect, _, output) = tile_pass(&ctx, &item, vec![release]);

        assert_eq!(
            ctx.memory(|memory| memory.focused()),
            Some(play_id),
            "Tab never reached the tile's play"
        );
        assert!(
            play_painted(&output, tile_play_rect(rect)),
            "a focused play was hidden again"
        );
    }

    #[test]
    fn a_press_on_the_rows_play_reports_play_and_not_open_or_a_door() {
        let ctx = context();
        let item = media("spotify:album:a", false, "11 tracks");
        let play = row_play_rect(row_rect());
        let answers = run(&ctx, &item, &click_events(play.center()));

        assert!(
            played(&answers),
            "a press on the row's play reported nothing: {answers:?}"
        );
        assert!(!opened(&answers), "the play also opened the row");
        assert_eq!(door(&answers), None, "the play also opened a menu");
    }

    #[test]
    fn the_row_play_sits_inside_the_cover_and_clear_of_the_more() {
        let columns = columns(row_rect());
        let play = row_play_rect(row_rect());

        assert_eq!(play.size(), Vec2::splat(extent::CARD_PLAY));
        assert!(columns.art.contains_rect(play), "the play left the cover");
        assert!(!play.intersects(columns.more), "the play reached the `⋯`");
    }

    #[test]
    fn a_carried_tile_or_row_has_a_gated_play_that_does_not_open_it() {
        let ctx = context();
        let item = media("spotify:album:a", false, "11 tracks");
        let (rect, _, _) = tile_pass(&ctx, &item, Vec::new());

        // A release ends the carry, so each run is carried on its own.
        egui::DragAndDrop::set_payload(&ctx, ());
        let tile_answers = tile_run(&ctx, &item, &click_events(tile_play_rect(rect).center()));
        egui::DragAndDrop::clear_payload(&ctx);

        egui::DragAndDrop::set_payload(&ctx, ());
        let row_answers = run(
            &ctx,
            &item,
            &click_events(row_play_rect(row_rect()).center()),
        );
        egui::DragAndDrop::clear_payload(&ctx);

        assert!(
            !played(&tile_answers),
            "a carried tile's play started something: {tile_answers:?}"
        );
        assert!(
            tile_answers.iter().flatten().next().is_none(),
            "a carried tile's play opened the item: {tile_answers:?}"
        );
        assert!(
            !played(&row_answers),
            "a carried row's play started something: {row_answers:?}"
        );
        assert!(!opened(&row_answers), "a carried row's play opened the row");
        assert_eq!(door(&row_answers), None);
    }

    #[test]
    fn a_card_with_four_covers_draws_four_quadrants() {
        let ctx = context();
        let (_a, a) = uploaded(&ctx, "a");
        let (_b, b) = uploaded(&ctx, "b");
        let (_c, c) = uploaded(&ctx, "c");
        let (_d, d) = uploaded(&ctx, "d");
        let covers = [a, b, c, d];
        let card = pictured(
            "spotify:playlist:a",
            covers[0],
            hero::Picture::Mosaic(covers),
        );

        let (rect, output) = card_pass(&ctx, &card);
        let art = art_rect(rect);
        let quarters = textured(&output, art);

        assert_eq!(quarters.len(), 4, "four covers tile four quadrants");
        assert_eq!(quarters[0], Rect::from_min_size(art.min, art.size() / 2.0));
        assert_eq!(
            quarters[3].max, art.max,
            "the last quadrant sits bottom-right"
        );
    }

    #[test]
    fn a_card_with_one_cover_draws_the_single_cover() {
        let ctx = context();
        let (_a, cover) = uploaded(&ctx, "a");
        let card = pictured("spotify:playlist:a", cover, hero::Picture::Cover);

        let (rect, output) = card_pass(&ctx, &card);
        let art = art_rect(rect);

        assert_eq!(
            textured(&output, art),
            vec![art],
            "one picture fills the art box whole"
        );
    }

    #[test]
    fn a_row_never_draws_a_mosaic() {
        let ctx = context();
        let (_a, a) = uploaded(&ctx, "a");
        let (_b, b) = uploaded(&ctx, "b");
        let (_c, c) = uploaded(&ctx, "c");
        let (_d, d) = uploaded(&ctx, "d");
        let covers = [a, b, c, d];
        let media = pictured(
            "spotify:playlist:a",
            covers[0],
            hero::Picture::Mosaic(covers),
        );
        let tokens = Tokens::DARK;
        let mut row_rect = Rect::ZERO;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 200.0))),
                ..Default::default()
            },
            |ui| {
                row_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, extent::ROW_H));
                let _ = row(ui, row_rect, &media, &tokens);
            },
        );
        output.textures_delta.clear();

        let art = columns(row_rect).art;
        assert_eq!(
            textured(&output, art),
            vec![art],
            "a 36pt row keeps the single cover: a four-up there is mush"
        );
    }
}
