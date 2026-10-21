//! The search drop-down: the first five matches, the floor row, and the line
//! that says there is nothing to show.
//!
//! The box is the app's one menu box ([`super::menu_box`]), so it is the same
//! raised card, hairline and shadow as a track row's context menu. This view
//! keeps only its own row grammar: the cover, the title over `artist · album`,
//! the action glyph and the floor row.
//!
//! The menu is a foreground `Area` anchored to the field's own box (R7): the
//! same width and `extent::MENU_GAP` below it. It is not clipped by the topbar
//! and it draws over the view, the way the drawer's `Area` does.
//!
//! Two tiers, never three (R9). The keyboard owns the 2px rule, the
//! `row_selected` wash and the action glyph; the pointer owns a `row_hover` wash
//! and a 1rem tick, and never borrows the rule or the glyph. The playing row
//! keeps the accent rule and the equaliser, and identity beats focus: if the
//! playing row is also marked, the accent rule stays and the glyph becomes pause
//! (R11).
//!
//! The washes are the app-wide row tokens (`row_hover` for the pointer,
//! `row_selected` for the mark), not the reference's literal values: its 4% wash
//! predates the row-hover token.
//!
//! The rows are `Sense::CLICK`, never `Sense::click()` (R18): the list is not a
//! tab stop, so the keyboard path is arrows only and the rule always says where
//! Enter will land.

use std::time::Instant;

use egui::{FontId, Order, Rect, Sense, Ui, pos2, vec2};
use takofy_model::TrackEntry;

use crate::cover::Art;
use crate::icon::{self, Icon};
use crate::layout::search_menu as geometry;
use crate::menu::{Device, Tier};
use crate::player::Transport;
use crate::search::{Act, MenuView, Search};
use crate::theme::icon::Size;
use crate::theme::{MonoWeight, Tokens, Weight, fonts, radius, space, stroke, text, tracking};

use super::art;
use super::menu_box::{self, Card, clicked_on, paint_highlight, paint_marker, row_corners};
use super::text as recipe;
use super::track_row::{self, State, state_of};

/// What the menu asked the shell to do this frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Menu {
    /// The action a row click asked for. Closing is the shell's decision, not
    /// this view's.
    pub click: Option<Act>,
    /// The frame the menu was drawn in, so the shell can tell whether a pointer
    /// is on it. `None` when there was nothing true to draw (R14).
    pub rect: Option<Rect>,
}

/// Draws the menu under `field` and returns what its rows asked for.
///
/// The answer is read from the search state, guarded by the query it was asked
/// for: an answer to the query the user has typed past is never drawn (R14).
pub fn show(
    ui: &mut Ui,
    field: Rect,
    search: &mut Search,
    transport: &Transport,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Menu {
    let view = search.menu_view();
    let height = match view {
        MenuView::Rows { tracks, floor, .. } => geometry::rows_height(tracks.len(), floor),
        MenuView::Empty | MenuView::Failed(_) => geometry::line_height(),
        // No skeleton, no empty box: nothing true to say (R14).
        MenuView::Nothing => return Menu::default(),
    };
    let menu = geometry::menu_rect(field, ui.ctx().content_rect(), height);

    // Every state of the menu is one surface, and every row is keyed under one
    // salt, so a test or a screen reader can name what was drawn.
    let base = ui.make_persistent_id("search-menu-row");

    // A pointer that is merely resting over the menu is not the last device; a
    // pointer that moves over it is (R9).
    let pointer_moved = ui.input(|input| {
        input.pointer.is_moving()
            && input
                .pointer
                .hover_pos()
                .is_some_and(|pos| menu.contains(pos))
    });

    let mut click = None;
    let area = egui::Area::new(ui.scope_id().with("search-menu"))
        .order(Order::Foreground)
        .fixed_pos(menu.min)
        .constrain(false)
        .fade_in(false);

    area.show(ui.ctx(), |ui| {
        let card = Card {
            rect: menu,
            id: base.with("surface"),
            // The rows are the only Tab stops a menu of results needs, and it has
            // none: the keyboard path is arrows only (R18).
            sense: Sense::CLICK,
            tokens,
        };
        menu_box::card(ui, card, |ui| match view {
            MenuView::Rows {
                tracks,
                total,
                floor,
            } => {
                let shown = tracks.len();
                for (index, entry) in tracks.iter().enumerate() {
                    if let Some(act) = result_row(
                        ui,
                        search,
                        transport,
                        art,
                        now,
                        tokens,
                        menu,
                        index,
                        !floor && index + 1 == shown,
                        entry,
                        base,
                    ) {
                        click = Some(act);
                    }
                }
                if floor && let Some(act) = floor_row(ui, search, tokens, menu, shown, total, base)
                {
                    click = Some(act);
                }
            }
            MenuView::Empty => {
                let query = search.key().unwrap_or_default();
                line(ui, tokens, menu, &format!("No matches for “{query}”"), base);
            }
            MenuView::Failed(failure) => {
                line(
                    ui,
                    tokens,
                    menu,
                    crate::signin::failure_message(failure),
                    base,
                );
            }
            MenuView::Nothing => {}
        });
    });

    // The pointer moved over the menu, so the pointer owns the highlight (R9).
    if pointer_moved {
        search.pointer_moved();
    }

    Menu {
        click,
        rect: Some(menu),
    }
}

/// Draws one result row and reports the click it took.
#[expect(
    clippy::too_many_arguments,
    reason = "a row needs its place, its state and the frame's cover store"
)]
fn result_row(
    ui: &mut Ui,
    search: &Search,
    transport: &Transport,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
    menu: Rect,
    index: usize,
    last: bool,
    entry: &TrackEntry,
    base: egui::Id,
) -> Option<Act> {
    let row = geometry::row_rect(menu, index);
    let hit = ui.interact(row, base.with(index), Sense::CLICK);
    // The row is one button in the accessibility tree, named by its title: the
    // paint alone says nothing to a screen reader.
    let spoken = if entry.artists.is_empty() {
        entry.title.clone()
    } else {
        format!("{} by {}", entry.title, entry.artists)
    };
    hit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, spoken.clone()));

    let marked = search.mark() == Some(index);
    let hovered = hit.hovered() && !marked && search.device() == Device::Pointer;
    // The glyph and the action Enter fires read the one hold decision (R10):
    // only a track a pause would act on promises a pause. A loaded track that
    // has ended is a restart, so its marked row shows play.
    let hold = transport.hold(&entry.uri);
    let state = state_of(&entry.uri, transport);
    let current = !matches!(state, State::Resting);
    let corners = row_corners(index == 0, last);

    let tier = if marked {
        Tier::Keyboard
    } else if hovered {
        Tier::Pointer
    } else {
        Tier::None
    };
    paint_highlight(ui, tokens, row, corners, tier);

    // Identity beats focus: the playing row keeps the accent rule even while it
    // is marked (R11).
    if current || marked {
        paint_marker(
            ui,
            row,
            corners,
            if current {
                tokens.palette.accent
            } else {
                tokens.palette.text
            },
        );
    }

    let parts = geometry::row_parts(row);
    // A row too narrow to keep the cover does not ask the art store for one.
    if parts.art.width() > 0.0 {
        let cover = art.look(entry.cover.as_ref(), now);
        art::tile(
            ui.painter(),
            parts.art,
            radius::XS,
            tokens.palette.border,
            cover,
            tokens,
        );
    }
    paint_text(ui, parts.text, entry, marked || hovered, tokens);

    // The keyboard tier's glyph: pause for the track a pause would act on, play
    // for any other marked row (R10). The playing row shows the equaliser until
    // it is also marked, when the glyph becomes pause (R11).
    if hold.held && marked {
        icon::paint(
            ui.painter(),
            Icon::Pause,
            parts.glyph,
            Size::SM,
            tokens.palette.text,
        );
    } else if hold.held {
        track_row::equaliser(ui, parts.glyph, state, tokens);
    } else if marked {
        icon::paint(
            ui.painter(),
            Icon::Play,
            parts.glyph,
            Size::SM,
            tokens.palette.text,
        );
    }

    if hit.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    clicked_on(ui, &hit, row).then_some(Act::Play(index))
}

/// Draws the floor row: `See all N results for “q”`, in the menu's mono voice,
/// with the chevron and the same highlight a result row takes (R12).
fn floor_row(
    ui: &mut Ui,
    search: &Search,
    tokens: &Tokens,
    menu: Rect,
    index: usize,
    total: usize,
    base: egui::Id,
) -> Option<Act> {
    let row = geometry::row_rect(menu, index);
    let hit = ui.interact(row, base.with(index), Sense::CLICK);
    let marked = search.mark() == Some(index);
    let hovered = hit.hovered() && !marked && search.device() == Device::Pointer;
    let corners = row_corners(false, true);
    let tier = if marked {
        Tier::Keyboard
    } else if hovered {
        Tier::Pointer
    } else {
        Tier::None
    };

    paint_highlight(ui, tokens, row, corners, tier);
    // The floor is an action, not a result: one hairline separates it from the
    // rows above. It sits over the wash, where the reference's border does.
    ui.painter().rect_filled(
        Rect::from_min_size(row.min, vec2(row.width(), stroke::HAIRLINE)),
        radius::NONE,
        tokens.palette.border,
    );
    if marked {
        paint_marker(ui, row, corners, tokens.palette.text);
    }

    let inner = Rect::from_min_max(
        pos2(row.left() + space::S3, row.top() + space::S1),
        pos2(row.right() - space::S3, row.bottom() - space::S1),
    );
    let glyph = Rect::from_center_size(
        pos2(inner.right() - Size::SM.side / 2.0, inner.center().y),
        vec2(Size::SM.side, Size::SM.side),
    );
    let color = if marked {
        tokens.palette.text
    } else {
        tokens.palette.text_muted
    };
    let query = search.key().unwrap_or_default();
    let label = format!("see all {total} results for “{query}”").to_uppercase();
    hit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label.clone()));
    let width = (glyph.left() - space::S3 - inner.left()).max(0.0);
    let galley = recipe::line(
        ui,
        &label,
        FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
        color,
        recipe::tracking_pts(text::XXS, tracking::WIDE),
        width,
    );
    ui.painter().galley(
        pos2(inner.left(), inner.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    icon::paint(ui.painter(), Icon::ChevronRight, glyph, Size::SM, color);

    if hit.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    clicked_on(ui, &hit, row).then_some(Act::OpenResults)
}

/// The single muted line for no matches or a failed read (R13).
fn line(ui: &mut Ui, tokens: &Tokens, menu: Rect, message: &str, base: egui::Id) {
    let row = geometry::row_rect(menu, 0);
    // The line has no action, but it is still one node in the accessibility
    // tree: an unlabelled card is an empty region to a screen reader.
    let hit = ui.interact(row, base.with("line"), Sense::hover());
    hit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, message));
    let inner = Rect::from_min_max(
        pos2(row.left() + space::S3, row.top() + space::S1),
        pos2(row.right() - space::S3, row.bottom() - space::S1),
    );
    let color = tokens.palette.text_subtle;
    let galley = recipe::line(
        ui,
        message,
        FontId::new(text::XS, fonts::sans(Weight::Regular)),
        color,
        0.0,
        inner.width(),
    );
    ui.painter().galley(
        pos2(inner.left(), inner.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

/// The title over `artist · album`, both truncated to the text column (R8).
fn paint_text(ui: &Ui, rect: Rect, entry: &TrackEntry, lit: bool, tokens: &Tokens) {
    if rect.width() <= 0.0 {
        return;
    }
    let title_color = tokens.palette.text;
    let sub_color = if lit {
        tokens.palette.text_muted
    } else {
        tokens.palette.text_subtle
    };
    let title = recipe::line(
        ui,
        &entry.title,
        FontId::new(text::SM, fonts::sans(Weight::Medium)),
        title_color,
        0.0,
        rect.width(),
    );
    let subtitle = subtitle(entry);
    let sub = (!subtitle.is_empty()).then(|| {
        recipe::line(
            ui,
            &subtitle,
            FontId::new(text::XS, fonts::sans(Weight::Regular)),
            sub_color,
            0.0,
            rect.width(),
        )
    });

    let title_height = title.size().y;
    let height = title_height + sub.as_ref().map_or(0.0, |line| line.size().y);
    let top = rect.center().y - height / 2.0;
    ui.painter()
        .galley(pos2(rect.left(), top), title, title_color);
    if let Some(sub) = sub {
        ui.painter()
            .galley(pos2(rect.left(), top + title_height), sub, sub_color);
    }
}

/// `artist · album`, or whichever half exists.
fn subtitle(entry: &TrackEntry) -> String {
    match (entry.artists.is_empty(), entry.album.as_deref()) {
        (false, Some(album)) => format!("{} · {album}", entry.artists),
        (false, None) => entry.artists.clone(),
        (true, Some(album)) => album.to_owned(),
        (true, None) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use egui::{Pos2, RawInput, Rect, vec2};
    use takofy_model::TrackEntry;

    use super::*;
    use crate::message::{Failure, Update};
    use crate::search::{KeyInput, Search};
    use crate::theme::{extent, fonts, style};

    fn field() -> Rect {
        Rect::from_min_size(Pos2::new(40.0, 12.0), vec2(320.0, extent::SEARCH_H))
    }

    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&Tokens::DARK));
        ctx.enable_accesskit();
        ctx
    }

    fn track(index: usize) -> TrackEntry {
        TrackEntry {
            uri: format!("t{index}"),
            title: format!("t{index}"),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }
    }

    /// A search state with `count` tracks answered for "a", open the way a click
    /// on the field opens it (R2).
    fn answered(count: usize) -> Search {
        let mut search = Search::default();
        search.text = "a".to_owned();
        search.text_changed();
        search.apply(&Update::Search {
            query: "a".to_owned(),
            tracks: (0..count).map(track).collect(),
        });
        assert_eq!(
            search.step(
                KeyInput {
                    focused: true,
                    clicked_field: true,
                    ..KeyInput::default()
                },
                count.min(5),
                count > 5,
            ),
            Act::None
        );
        assert!(search.is_open(), "the menu is drawn");
        search
    }

    fn failed() -> Search {
        let mut search = Search::default();
        search.text = "a".to_owned();
        search.text_changed();
        search.apply(&Update::SearchFailed {
            query: "a".to_owned(),
            failure: Failure::Search,
        });
        let _ = search.step(
            KeyInput {
                focused: true,
                clicked_field: true,
                ..KeyInput::default()
            },
            0,
            false,
        );
        assert!(search.is_open(), "the menu is drawn");
        search
    }

    /// Draws the menu twice — an `Area`'s first frame is its sizing pass — and
    /// runs `read` inside the second pass, where egui keeps both the widget
    /// rects and the AccessKit state.
    fn draw<R>(
        ctx: &egui::Context,
        search: &mut Search,
        read: impl Fn(&egui::Ui, egui::Id, Menu) -> R,
    ) -> R {
        let tokens = Tokens::DARK;
        let mut art = Art::default();
        let now = Instant::now();
        let mut result = None;
        for pass in 0..2 {
            art.begin_frame();
            let mut output = ctx.run_ui(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                    ..Default::default()
                },
                |ui| {
                    let base = ui.make_persistent_id("search-menu-row");
                    let menu = show(
                        ui,
                        field(),
                        search,
                        &crate::player::Transport::default(),
                        &mut art,
                        now,
                        &tokens,
                    );
                    if pass == 1 {
                        result = Some(read(ui, base, menu));
                    }
                },
            );
            output.textures_delta.clear();
        }
        result.expect("two passes")
    }

    /// Finding 1: every state of the menu is one click-swallowing surface, so a
    /// click on the empty line cannot fall through to a track row or a hero
    /// button underneath it.
    #[test]
    fn every_state_of_the_menu_swallows_clicks() {
        for mut search in [answered(3), answered(0), failed()] {
            let ctx = context();
            let (takes_click, focusable, surface_rect, card) =
                draw(&ctx, &mut search, |ui, base, menu| {
                    let surface = ui
                        .ctx()
                        .read_response(base.with("surface"))
                        .expect("the card is a widget");
                    (
                        surface.sense.senses_click(),
                        surface.sense.is_focusable(),
                        surface.rect,
                        menu.rect.expect("the menu was drawn"),
                    )
                });
            assert!(takes_click, "the card takes the click");
            assert!(!focusable, "and is no Tab stop");
            assert_eq!(surface_rect, card, "and covers exactly the card");
        }
    }

    /// Finding 9 and R18: the rows are named buttons that a screen reader can
    /// read, and `Sense::CLICK` keeps them out of the Tab order.
    #[test]
    fn the_rows_are_named_buttons_and_never_tab_stops() {
        let ctx = context();
        let mut search = answered(8);
        let rows = draw(&ctx, &mut search, |ui, base, _menu| {
            (0..6)
                .map(|index| {
                    let response = ui.ctx().read_response(base.with(index)).expect("a row");
                    let (role, label) = ui
                        .ctx()
                        .accesskit_node_builder(base.with(index), |node| {
                            (node.role(), node.label().map(str::to_owned))
                        })
                        .expect("the row has an AccessKit node");
                    (response.sense, role, label)
                })
                .collect::<Vec<_>>()
        });

        // Five result rows and the floor: named by what each one says, and none
        // of them a Tab stop (R18).
        let expected = ["t0", "t1", "t2", "t3", "t4", "SEE ALL 8 RESULTS FOR “A”"];
        assert_eq!(rows.len(), expected.len());
        for (index, ((sense, role, label), name)) in rows.into_iter().zip(expected).enumerate() {
            assert!(!sense.is_focusable(), "row {index} is not a Tab stop (R18)");
            assert!(sense.senses_click(), "row {index} takes a click");
            assert_eq!(role, egui::Role::Button, "row {index}");
            assert_eq!(label.as_deref(), Some(name), "row {index}");
        }
    }
    /// Finding 9: the empty line is a labelled node too, or the card reads as an
    /// empty region. egui puts a `Role::Label`'s text in the node's value.
    #[test]
    fn the_empty_line_announces_why_the_card_is_empty() {
        let ctx = context();
        let mut search = answered(0);
        let (role, label) = draw(&ctx, &mut search, |ui, base, _menu| {
            ui.ctx()
                .accesskit_node_builder(base.with("line"), |node| {
                    (node.role(), node.value().map(str::to_owned))
                })
                .expect("the line has an AccessKit node")
        });
        assert_eq!(role, egui::Role::Label);
        assert_eq!(label.as_deref(), Some("No matches for “a”"));
    }
}
