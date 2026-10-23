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
    pub cover: Cover,
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
pub fn tile(ui: &mut Ui, rect: Rect, media: &Media<'_>, tokens: &Tokens) -> bool {
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
    if response.hovered() || response.has_focus() {
        ui.painter()
            .rect_filled(rect, radius::MD, tokens.palette.surface_raised);
        ui.painter().rect_stroke(
            rect,
            radius::MD,
            egui::Stroke::new(stroke::HAIRLINE, tokens.palette.border),
            egui::StrokeKind::Inside,
        );
    }
    if response.has_focus() {
        controls::focus_ring(ui, rect, tokens);
    }

    let inner = rect.shrink(space::S2);
    let art = Rect::from_min_size(inner.min, Vec2::splat(inner.width()));
    let radius = if media.round {
        radius::PILL
    } else {
        radius::SM
    };
    art::tile(
        ui.painter(),
        art,
        radius,
        tokens.palette.border,
        media.cover,
        tokens,
    );

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

    response.clicked()
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

/// What one list row reports: the row itself was chosen, or a menu was asked for.
///
/// A row has two things to say, so it reports one or the other; a tile keeps a
/// plain flag, because the grid's cards carry no `⋯`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Press {
    /// The row's own body was chosen.
    Open,
    /// One of the row's doors asked for its menu.
    Menu(Door),
}

/// One list row: the cover, the words, the type, the size and the more button.
///
/// The `⋯` is its own control, so a press on it never falls through to the row.
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
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, radius::SM, tokens.palette.row_hover);
    }
    if response.has_focus() {
        controls::focus_ring(ui, rect, tokens);
    }

    let radius = if media.round {
        radius::PILL
    } else {
        radius::XS
    };
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
    let live = !carry::carrying(ui.ctx());
    let id = controls::slot_id(ui, Control::LibraryMore, media.uri);
    let opener = context_menu::open_opener(ui.ctx());
    let open = opener.is_some_and(|opener| opener == id || opener == response.id);
    let slot = controls::icon_button_open(
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
    let _ = ui.ctx().accesskit_node_builder(slot.response.id, |node| {
        node.set_expanded(open);
        node.set_has_popup(egui::accesskit::HasPopup::Menu);
    });

    let on_more = response
        .interact_pointer_pos()
        .is_some_and(|at| columns.more.contains(at));

    // The three doors report a menu before the row reports itself: a menu opens
    // on the row the user pointed at, and never also selects or opens it.
    if live
        && let Some(door) = context_menu::button_door(ui, &slot, columns.more, id)
            .or_else(|| key_door(ui, &response, &slot, columns.more))
            .or_else(|| context_menu::pointer_door(ui, rect, response.id, columns.more))
    {
        return Some(Press::Menu(door));
    }

    (response.clicked() && !on_more).then_some(Press::Open)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{fonts, style};
    use crate::view::test_support::{click_events, secondary_click_events};
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
            cover: Cover::Missing,
        }
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
            Press::Open => None,
        })
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
}
