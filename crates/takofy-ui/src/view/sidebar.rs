//! The sidebar: the brand, the navigation and the library.
//!
//! The rail and the full sidebar are one function, because they share everything
//! but the label and the padding, and which one is on screen comes from the
//! region model rather than from a mode of its own.

use std::time::Instant;

use egui::{Layout, Pos2, Rect, RichText, Sense, Ui, UiBuilder, Vec2};
use takofy_model::FollowedShow;

use crate::availability::Control;
use crate::cover::{Art, Cover};
use crate::format::sidebar_kind_label;
use crate::icon::{self, Icon};
use crate::layout::{Sidebar, chips};
use crate::library::{Library, Load};
use crate::message::{Command, Failure};
use crate::route::Page;
use crate::sidebar_kind::{self, SidebarKind};
use crate::signin::failure_message;
use crate::theme::icon::Size;
use crate::theme::{
    MonoWeight, Tokens, Weight, extent, fonts, radius, space, stroke, text, tracking,
};

use super::art;
use super::controls;
use super::shell::rule;
use super::text as recipe;

/// A library row: the art tile plus the padding above and below it.
pub(super) const ROW_H: f32 = extent::LIBRARY_ART + 2.0 * space::S2;

/// Draws the sidebar, returning a command if one of its controls asked for one.
pub fn show(
    ui: &mut Ui,
    form: Sidebar,
    library: &mut Library,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<Command> {
    let band = ui.available_rect_before_wrap();
    let y = brand(ui, form, tokens, band, band.top());
    let y = nav(ui, form, tokens, band, y);

    match form {
        Sidebar::Rail => None,
        Sidebar::Full => library_block(ui, library, art, now, tokens, band, y),
    }
}

/// The monogram, and beside it the wordmark the rail has no room for.
fn brand(ui: &mut Ui, form: Sidebar, tokens: &Tokens, band: Rect, top: f32) -> f32 {
    let side = Size::LG.side;
    let row = Rect::from_min_size(
        Pos2::new(band.left(), top + space::S4),
        Vec2::new(band.width(), side),
    );
    let logo = match form {
        Sidebar::Rail => Rect::from_center_size(row.center(), Vec2::splat(side)),
        Sidebar::Full => Rect::from_min_size(
            Pos2::new(row.left() + space::S4, row.top()),
            Vec2::splat(side),
        ),
    };
    icon::paint(
        ui.painter(),
        Icon::Logo,
        logo,
        Size::LG,
        tokens.palette.text,
    );

    if form == Sidebar::Full {
        let mark = Rect::from_min_size(
            Pos2::new(logo.right() + space::S3, row.top()),
            Vec2::new(row.right() - logo.right() - space::S4, side),
        );
        ui.put(mark, egui::Label::new(recipe::wordmark("Takofy", tokens)));
    }

    row.bottom() + space::S3
}

/// Home and Search. Neither view exists yet, so both are drawn waiting.
fn nav(ui: &mut Ui, form: Sidebar, tokens: &Tokens, band: Rect, top: f32) -> f32 {
    let height = Size::LG.side + 2.0 * space::S2;
    let inset = match form {
        Sidebar::Rail => 0.0,
        Sidebar::Full => space::S1,
    };
    let mut y = top;

    for (control, glyph, label) in [
        (Control::NavHome, Icon::Home, "Home"),
        (Control::NavSearch, Icon::Search, "Search"),
    ] {
        let rect = Rect::from_min_size(
            Pos2::new(band.left() + inset, y),
            Vec2::new(band.width() - 2.0 * inset, height),
        );
        controls::nav_item(ui, tokens, control, rect, form, glyph, label);
        y = rect.bottom() + space::PX;
    }

    y
}

/// The library: the rule, the caption, the filter chips and the list.
fn library_block(
    ui: &mut Ui,
    library: &mut Library,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
    band: Rect,
    top: f32,
) -> Option<Command> {
    let inset = space::S1;
    let rule_rect = Rect::from_min_size(
        Pos2::new(band.left() + inset, top + space::S4),
        Vec2::new(band.width() - 2.0 * inset, stroke::HAIRLINE),
    );
    rule(ui, rule_rect, tokens.palette.border);
    let mut y = rule_rect.bottom() + space::S3;

    // The caption, with the reference's two buttons to the right. Creating a
    // playlist and expanding the library both wait for folders.
    let row = Rect::from_min_size(
        Pos2::new(band.left() + space::S4, y),
        Vec2::new(band.width() - 2.0 * space::S4, extent::BUTTON),
    );
    let caption = recipe::spaced(
        ui,
        "YOUR LIBRARY",
        egui::FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XXS, tracking::CAPS),
    );
    ui.painter().galley(
        Pos2::new(row.left(), row.center().y - caption.size().y / 2.0),
        caption,
        tokens.palette.text_subtle,
    );
    let expand = Rect::from_min_size(
        Pos2::new(
            row.right() - extent::BUTTON,
            row.center().y - extent::BUTTON / 2.0,
        ),
        Vec2::splat(extent::BUTTON),
    );
    controls::icon_button(
        ui,
        tokens,
        Control::LibraryExpand,
        expand,
        Icon::ArrowRight,
        Size::MD,
        "Expand your library",
        true,
    );
    let create = Rect::from_min_size(
        Pos2::new(expand.left() - extent::BUTTON - space::S1, expand.top()),
        Vec2::splat(extent::BUTTON),
    );
    controls::icon_button(
        ui,
        tokens,
        Control::LibraryCreate,
        create,
        Icon::Plus,
        Size::MD,
        "Create playlist or folder",
        true,
    );
    y = row.bottom() + space::S2;

    // The kind row, in the reference's order. The pressed chip is the chosen
    // kind; a waiting chip never reports a click, so it stays unpressed.
    let sizes: Vec<Vec2> = sidebar_kind::ROW
        .iter()
        .map(|kind| controls::chip_size(ui, tokens, sidebar_kind_label(*kind)))
        .collect();
    // The sidebar's row is its buttons' pitch, not a chip's height: the chips
    // are shorter than the row they sit in.
    let wrapped = chips::wrap(
        &sizes,
        band.width() - 2.0 * space::S4,
        space::S1,
        extent::BUTTON,
        space::S1,
    );
    let origin = Vec2::new(band.left() + space::S4, y);
    let mut pressed: Option<SidebarKind> = None;
    for (kind, rect) in sidebar_kind::ROW.iter().zip(wrapped.rects) {
        let slot = controls::chip_in(
            ui,
            tokens,
            kind.control(),
            rect.translate(origin),
            sidebar_kind_label(*kind),
            library.sidebar_kind.chosen() == *kind,
        );
        if slot.response.clicked() {
            pressed = Some(*kind);
        }
    }
    if let Some(kind) = pressed {
        library.sidebar_kind.choose(kind);
    }
    y += wrapped.height + space::S3;

    let list = Rect::from_min_size(
        Pos2::new(band.left() + space::S2, y),
        Vec2::new(band.width() - 2.0 * space::S2, band.bottom() - y),
    );
    rows(ui, list, library, art, now, tokens)
}

/// The list the chosen kind shows, or the state of the load that fetches it.
fn rows(
    ui: &mut Ui,
    rect: Rect,
    library: &mut Library,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<Command> {
    if rect.height() < ROW_H {
        return None;
    }

    match library.sidebar_kind.chosen() {
        SidebarKind::Playlists => {
            let mut picked: Option<String> = None;
            let mut retry = false;

            match &library.playlists {
                Load::Idle => {}
                Load::Loading => state(ui, rect, recipe::muted("Loading your playlists…", tokens)),
                Load::Ready(playlists) if playlists.is_empty() => {
                    state(ui, rect, recipe::muted("No playlists yet.", tokens));
                }
                Load::Ready(playlists) => {
                    let current = library.selected().map(str::to_owned);
                    let entries = playlists.clone();
                    ui.scope_builder(
                        UiBuilder::new()
                            .id_salt("library-list")
                            .max_rect(rect)
                            .layout(Layout::top_down(egui::Align::Min)),
                        |ui| {
                            ui.spacing_mut().item_spacing.y = space::PX;
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show_rows(ui, ROW_H, entries.len(), |ui, range| {
                                    for index in range {
                                        let Some(entry) = entries.get(index) else {
                                            continue;
                                        };
                                        let (_, place) = ui
                                            .allocate_space(Vec2::new(ui.available_width(), ROW_H));
                                        let selected =
                                            current.as_deref() == Some(entry.uri.as_str());
                                        let row = LibraryRow {
                                            id: playlist_row_id(ui, &entry.uri),
                                            name: &entry.name,
                                            kind: "Playlist",
                                            cover: art.look(entry.cover.as_ref(), now),
                                            selected,
                                        };
                                        if one(ui, place, row, tokens) {
                                            picked = Some(entry.uri.clone());
                                        }
                                    }
                                });
                        },
                    );
                }
                Load::Failed(failure) => {
                    state(ui, rect, recipe::muted(failure_message(*failure), tokens));
                    retry = true;
                }
            }

            if let Some(uri) = picked {
                return Some(library.select(&uri));
            }
            if retry { Some(library.reload()) } else { None }
        }
        SidebarKind::Podcasts => show_rows(ui, rect, library, art, now, tokens),
        // `choose` is the only writer of the choice, and it refuses waiting
        // kinds, so these two kinds cannot be chosen.
        SidebarKind::Artists => {
            debug_assert!(false, "the Artists chip is waiting, so it cannot be chosen");
            None
        }
        SidebarKind::Albums => {
            debug_assert!(false, "the Albums chip is waiting, so it cannot be chosen");
            None
        }
    }
}

/// What the Podcasts list draws: the followed shows, or the state of the
/// loads that fetch them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowsView<'a> {
    /// The playlists or the shows are in flight, or have not been asked for.
    Loading,
    /// A fetch failed; the retry names which load to restart.
    Failed(Failure, Retry),
    /// The user follows no shows.
    Empty,
    /// The shows to draw, newest follow first.
    List(&'a [FollowedShow]),
}

/// Which load a retry restarts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    /// The playlists; the followed list restarts with them.
    Playlists,
    /// The followed list alone.
    Followed,
}

/// Reads what the Podcasts list draws from the two loads.
///
/// The playlists are checked first, because the followed request only fires
/// once the playlists are Ready (`followed_request`), and a retry of the
/// playlists resets the followed list to Idle.
pub fn shows_view(library: &Library) -> ShowsView<'_> {
    match &library.playlists {
        Load::Failed(failure) => ShowsView::Failed(*failure, Retry::Playlists),
        Load::Idle | Load::Loading => ShowsView::Loading,
        Load::Ready(_) => match &library.followed {
            Load::Idle | Load::Loading => ShowsView::Loading,
            Load::Failed(failure) => ShowsView::Failed(*failure, Retry::Followed),
            Load::Ready(shows) if shows.is_empty() => ShowsView::Empty,
            Load::Ready(shows) => ShowsView::List(shows),
        },
    }
}

/// The row's click target and the chrome both kinds of row share: a selected
/// row borrows the reference's language for the current navigation item — a
/// raised fill and a bone rule at the left edge — because the reference defines
/// no selected row.
fn row_chrome(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    label: &str,
    selected: bool,
    tokens: &Tokens,
) -> egui::Response {
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let _ = ui
        .ctx()
        .accesskit_node_builder(response.id, |node| node.set_selected(selected));

    if selected {
        ui.painter()
            .rect_filled(rect, radius::SM, tokens.palette.surface_raised);
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(stroke::RULE, rect.height())),
            radius::SM,
            tokens.palette.text,
        );
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, radius::SM, tokens.palette.surface_hover);
    }

    if response.has_focus() {
        controls::focus_ring(ui, rect, tokens);
    }

    response
}

/// The id of a playlist row. The uri, not the name: two playlists may share a
/// name, and two widgets may not share an id.
fn playlist_row_id(ui: &Ui, uri: &str) -> egui::Id {
    ui.scope_id().with(("library-row", uri))
}

/// The id of a show row, kept apart from a playlist row's even when the uri is
/// the same string.
fn show_row_id(ui: &Ui, uri: &str) -> egui::Id {
    ui.scope_id().with(("library-show", uri))
}

/// The show the page is looking at, so the Podcasts list can highlight it.
///
/// `Library::selected` is about playlists and is `None` on a show page.
fn page_show(library: &Library) -> Option<&str> {
    match library.page() {
        Some(Page::Show(uri)) => Some(uri),
        _ => None,
    }
}

/// The Podcasts list: the followed shows, or the state of the loads behind them.
fn show_rows(
    ui: &mut Ui,
    rect: Rect,
    library: &mut Library,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<Command> {
    if rect.height() < ROW_H {
        return None;
    }

    let mut pressed: Option<String> = None;
    let mut retry: Option<Retry> = None;

    match shows_view(library) {
        ShowsView::Loading => state(ui, rect, recipe::muted("Loading your shows…", tokens)),
        ShowsView::Empty => state(ui, rect, recipe::muted("No shows followed yet.", tokens)),
        ShowsView::Failed(failure, which) => {
            state(ui, rect, recipe::muted(failure_message(failure), tokens));
            retry = Some(which);
        }
        ShowsView::List(shows) => {
            let current = page_show(library).map(str::to_owned);
            let entries = shows.to_vec();
            ui.scope_builder(
                UiBuilder::new()
                    .id_salt("library-shows")
                    .max_rect(rect)
                    .layout(Layout::top_down(egui::Align::Min)),
                |ui| {
                    ui.spacing_mut().item_spacing.y = space::PX;
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show_rows(ui, ROW_H, entries.len(), |ui, range| {
                            for index in range {
                                let Some(show) = entries.get(index) else {
                                    continue;
                                };
                                let (_, place) =
                                    ui.allocate_space(Vec2::new(ui.available_width(), ROW_H));
                                let selected = current.as_deref() == Some(show.uri.as_str());
                                let row = LibraryRow {
                                    id: show_row_id(ui, &show.uri),
                                    name: &show.name,
                                    kind: "Podcast",
                                    cover: art.look(show.cover.as_ref(), now),
                                    selected,
                                };
                                if one(ui, place, row, tokens) {
                                    pressed = Some(show.uri.clone());
                                }
                            }
                        });
                },
            );
        }
    }

    if let Some(uri) = pressed {
        return library.open(Page::Show(uri));
    }
    match retry {
        Some(Retry::Playlists) => Some(library.reload()),
        Some(Retry::Followed) => Some(library.reload_followed()),
        None => None,
    }
}

/// What one library row draws, whatever its kind.
struct LibraryRow<'a> {
    id: egui::Id,
    name: &'a str,
    kind: &'a str,
    cover: Cover,
    selected: bool,
}

/// One library row: a placeholder tile, the name and the kind.
///
/// The tile stands in until covers are fetched.
fn one(ui: &mut Ui, rect: Rect, row: LibraryRow<'_>, tokens: &Tokens) -> bool {
    let LibraryRow {
        id,
        name,
        kind,
        cover,
        selected,
    } = row;
    let response = row_chrome(ui, rect, id, name, selected, tokens);

    let tile = Rect::from_min_size(
        Pos2::new(
            rect.left() + space::S2,
            rect.center().y - extent::LIBRARY_ART / 2.0,
        ),
        Vec2::splat(extent::LIBRARY_ART),
    );
    art::tile(
        ui.painter(),
        tile,
        radius::SM,
        tokens.palette.border,
        cover,
        tokens,
    );

    let name_x = tile.right() + space::S3;
    let name_width = (rect.right() - name_x - space::S2).max(0.0);
    let name_line = recipe::line(
        ui,
        name,
        egui::FontId::new(text::SM, fonts::sans(Weight::Medium)),
        tokens.palette.text,
        0.0,
        name_width,
    );
    let kind_line = recipe::line(
        ui,
        kind,
        egui::FontId::new(text::XXS, fonts::mono(MonoWeight::Regular)),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XXS, tracking::SLIGHT),
        name_width,
    );
    let top = rect.center().y - (name_line.size().y + kind_line.size().y) / 2.0;
    ui.painter().galley(
        Pos2::new(name_x, top),
        name_line.clone(),
        tokens.palette.text,
    );
    ui.painter().galley(
        Pos2::new(name_x, top + name_line.size().y),
        kind_line,
        tokens.palette.text_subtle,
    );

    response.clicked()
}

/// A line of text where a list would be, for the empty and failed states.
fn state(ui: &mut Ui, rect: Rect, text: RichText) {
    ui.scope_builder(
        UiBuilder::new()
            .id_salt("library-state")
            .max_rect(rect)
            .layout(Layout::top_down(egui::Align::Center)),
        |ui| {
            ui.add_space(rect.height() / 4.0);
            ui.label(text);
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Pos2, Vec2};
    use takofy_model::{FollowedShow, PlaylistEntry};

    use crate::message::{Failure, Update};
    use crate::theme::{fonts, style};
    use crate::view::test_support::click_events;

    /// The chip rect and label rect the real sidebar paints, per label.
    ///
    /// A chip's label is painted at the chip's top-left plus its padding, so
    /// this recovers both boxes from what the draw actually produced.
    fn painted_chips(width: f32) -> Vec<(&'static str, Rect)> {
        let tokens = Tokens::DARK;
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![PlaylistEntry {
            uri: "spotify:playlist:a".to_owned(),
            name: "A".to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
        }]));
        let mut art = Art::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 800.0))),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let _ = show(
                ui,
                Sidebar::Full,
                &mut library,
                &mut art,
                Instant::now(),
                &tokens,
            );
        });
        output.textures_delta.clear();

        let mut chips = Vec::new();
        for label in ["PLAYLISTS", "ARTISTS", "ALBUMS", "PODCASTS"] {
            let Some((pos, size)) = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                        Some((text.pos, text.galley.size()))
                    }
                    _ => None,
                })
            else {
                continue;
            };
            let rect = Rect::from_min_size(
                Pos2::new(pos.x - extent::CHIP_PAD_X, pos.y - extent::CHIP_PAD_Y),
                Vec2::new(
                    size.x + 2.0 * extent::CHIP_PAD_X,
                    size.y + 2.0 * extent::CHIP_PAD_Y,
                ),
            );
            let label: &'static str = match label {
                "PLAYLISTS" => "Playlists",
                "ARTISTS" => "Artists",
                "ALBUMS" => "Albums",
                _ => "Podcasts",
            };
            chips.push((label, rect));
        }
        chips
    }

    /// The rects the real sidebar paints, per label and band width. The same
    /// positions are pinned in [`chips::wrap`]'s own tests; this is the half
    /// that catches a bad call site: the width, the origin or the row pitch
    /// handed to `wrap` wrong.
    #[test]
    fn the_drawn_chips_keep_their_characterised_positions() {
        for (width, expected) in [
            (
                254.0,
                vec![
                    ("Playlists", 16.0, 193.0),
                    ("Artists", 93.213, 193.0),
                    ("Albums", 157.206, 193.0),
                    ("Podcasts", 16.0, 225.0),
                ],
            ),
            (
                200.0,
                vec![
                    ("Playlists", 16.0, 193.0),
                    ("Artists", 93.213, 193.0),
                    ("Albums", 16.0, 225.0),
                    ("Podcasts", 73.4, 225.0),
                ],
            ),
            (
                140.0,
                vec![
                    ("Playlists", 16.0, 193.0),
                    ("Artists", 16.0, 225.0),
                    ("Albums", 16.0, 257.0),
                    ("Podcasts", 16.0, 289.0),
                ],
            ),
            (
                100.0,
                vec![
                    ("Playlists", 16.0, 193.0),
                    ("Artists", 16.0, 225.0),
                    ("Albums", 16.0, 257.0),
                    ("Podcasts", 16.0, 289.0),
                ],
            ),
        ] {
            let chips = painted_chips(width);
            assert_eq!(chips.len(), expected.len(), "band width {width}");
            for ((label, rect), (expected, left, top)) in chips.iter().zip(&expected) {
                assert_eq!(label, expected, "band width {width}");
                assert!(
                    (rect.left() - left).abs() < 0.01 && (rect.top() - top).abs() < 0.01,
                    "band width {width}: {label} at {rect:?}, not ({left}, {top})"
                );
            }
        }
    }

    /// One show, the way the followed list carries it.
    fn followed_show(uri: &str, name: &str) -> FollowedShow {
        FollowedShow {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            publisher: None,
            cover: None,
        }
    }

    #[test]
    fn a_failed_playlist_load_wins_over_a_ready_followed_list() {
        let mut library = Library::default();
        library.playlists = Load::Failed(Failure::Load);
        library.followed = Load::Ready(vec![followed_show("spotify:show:a", "A")]);

        assert_eq!(
            shows_view(&library),
            ShowsView::Failed(Failure::Load, Retry::Playlists)
        );
    }

    #[test]
    fn an_unstarted_or_inflight_playlist_load_is_loading() {
        for playlists in [Load::Idle, Load::Loading] {
            let load = format!("{playlists:?}");
            let mut library = Library::default();
            library.playlists = playlists;
            assert_eq!(shows_view(&library), ShowsView::Loading, "{load}");
        }
    }

    #[test]
    fn a_reload_does_not_flash_the_empty_state() {
        // The retry reset the followed list to Idle while the playlists load.
        let mut library = Library::default();
        library.playlists = Load::Loading;
        library.followed = Load::Idle;
        assert_eq!(shows_view(&library), ShowsView::Loading);

        // One beat later the playlists have landed, but the list has not been
        // asked for yet.
        let mut library = Library::default();
        library.playlists = Load::Ready(Vec::new());
        library.followed = Load::Idle;
        assert_eq!(shows_view(&library), ShowsView::Loading);
    }

    #[test]
    fn an_inflight_followed_list_is_loading() {
        let mut library = Library::default();
        library.playlists = Load::Ready(Vec::new());
        library.followed = Load::Loading;
        assert_eq!(shows_view(&library), ShowsView::Loading);
    }

    #[test]
    fn a_failed_followed_list_carries_its_own_retry() {
        let mut library = Library::default();
        library.playlists = Load::Ready(Vec::new());
        library.followed = Load::Failed(Failure::Followed);

        assert_eq!(
            shows_view(&library),
            ShowsView::Failed(Failure::Followed, Retry::Followed)
        );
    }

    #[test]
    fn a_ready_followed_list_is_empty_when_it_follows_nothing() {
        let mut library = Library::default();
        library.playlists = Load::Ready(Vec::new());
        library.followed = Load::Ready(Vec::new());
        assert_eq!(shows_view(&library), ShowsView::Empty);
    }

    #[test]
    fn a_ready_followed_list_is_the_shows_to_draw() {
        let shows = vec![
            followed_show("spotify:show:a", "A"),
            followed_show("spotify:show:b", "B"),
        ];
        let mut library = Library::default();
        library.playlists = Load::Ready(Vec::new());
        library.followed = Load::Ready(shows.clone());

        assert_eq!(shows_view(&library), ShowsView::List(shows.as_slice()));
    }

    /// The context the sidebar draws into, the way the window builds it.
    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&Tokens::DARK));
        ctx
    }

    /// One frame of the full sidebar over the characterised band: `events` go
    /// into the pass, and the frame's output and command come back.
    fn run(
        ctx: &egui::Context,
        events: Vec<Event>,
        library: &mut Library,
        art: &mut Art,
    ) -> (egui::FullOutput, Option<Command>) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, 800.0))),
            events,
            ..Default::default()
        };
        let tokens = Tokens::DARK;
        let mut command = None;
        let mut output = ctx.run_ui(input, |ui| {
            if let Some(next) = show(ui, Sidebar::Full, library, art, Instant::now(), &tokens) {
                command = Some(next);
            }
        });
        output.textures_delta.clear();
        (output, command)
    }

    /// The three passes of one click at `pos`, one pass each; the last command
    /// any of them returned.
    fn click(
        ctx: &egui::Context,
        pos: Pos2,
        library: &mut Library,
        art: &mut Art,
    ) -> Option<Command> {
        let mut command = None;
        for event in click_events(pos) {
            let (_, next) = run(ctx, vec![event], library, art);
            if next.is_some() {
                command = next;
            }
        }
        command
    }

    /// The rect the chip whose label was painted as `label` was drawn into:
    /// the label text rect, expanded by the chip's padding.
    fn chip_rect(output: &egui::FullOutput, label: &str) -> Rect {
        let (pos, size) = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some((text.pos, text.galley.size()))
                }
                _ => None,
            })
            .expect("the chip is drawn");
        Rect::from_min_size(pos, size).expand2(Vec2::new(extent::CHIP_PAD_X, extent::CHIP_PAD_Y))
    }

    /// The playlist list's one row, ready to draw.
    fn playlist(uri: &str, name: &str) -> PlaylistEntry {
        PlaylistEntry {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
        }
    }

    /// Where a label was painted, and how big it was.
    fn painted_text(output: &egui::FullOutput, label: &str) -> (Pos2, Vec2) {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some((text.pos, text.galley.size()))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{label:?} is drawn"))
    }

    #[test]
    fn a_playlist_row_click_loads_its_tracks() {
        let ctx = context();
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist(
            "spotify:playlist:a",
            "A list",
        )]));
        let mut art = Art::default();
        let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);

        let (name, name_size) = painted_text(&output, "A list");
        let (_, kind_size) = painted_text(&output, "Playlist");
        let command = click(
            &ctx,
            Pos2::new(name.x + 10.0, name.y + (name_size.y + kind_size.y) / 2.0),
            &mut library,
            &mut art,
        );

        assert_eq!(
            command,
            Some(Command::LoadTracks {
                playlist: "spotify:playlist:a".to_owned()
            })
        );
        assert_eq!(library.selected(), Some("spotify:playlist:a"));
    }

    #[test]
    fn a_playlist_name_starts_past_the_art_slot() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, 800.0))),
                ..Default::default()
            },
            |ui| {
                let rect = Rect::from_min_size(Pos2::new(20.0, 0.0), Vec2::new(200.0, ROW_H));
                let row = LibraryRow {
                    id: playlist_row_id(ui, "spotify:playlist:a"),
                    name: "A list",
                    kind: "Playlist",
                    cover: Cover::Missing,
                    selected: false,
                };
                one(ui, rect, row, &tokens);
            },
        );
        output.textures_delta.clear();

        let (name, _) = painted_text(&output, "A list");
        let (kind, _) = painted_text(&output, "Playlist");
        let expected = 20.0 + space::S2 + extent::LIBRARY_ART + space::S3;
        assert!((name.x - expected).abs() < 0.01, "name at {}", name.x);
        assert!((kind.x - expected).abs() < 0.01, "kind at {}", kind.x);
    }

    #[test]
    fn a_playlist_row_announces_its_selection() {
        let ctx = context();
        ctx.enable_accesskit();
        let tokens = Tokens::DARK;
        let uri = "spotify:playlist:a";
        let mut announced = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, 800.0))),
                ..Default::default()
            },
            |ui| {
                let id = playlist_row_id(ui, uri);
                let rect = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(200.0, ROW_H));
                let row = LibraryRow {
                    id,
                    name: "A list",
                    kind: "Playlist",
                    cover: Cover::Missing,
                    selected: true,
                };
                one(ui, rect, row, &tokens);
                announced = ui
                    .ctx()
                    .accesskit_node_builder(id, |node| node.is_selected())
                    .expect("the row has an AccessKit node");
            },
        );
        output.textures_delta.clear();

        assert_eq!(announced, Some(true), "a selected row says so");
    }

    /// A library with both lists ready, the way the window hands them over.
    fn library_with(playlists: Vec<PlaylistEntry>, shows: Vec<FollowedShow>) -> Library {
        let mut library = Library::default();
        library.apply(&Update::Playlists(playlists));
        library.apply(&Update::FollowedShows(shows));
        library
    }

    /// Whether a line of text was painted at all.
    fn painted(output: &egui::FullOutput, label: &str) -> bool {
        output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape,
                egui::epaint::Shape::Text(text) if text.galley.text() == label)
        })
    }

    #[test]
    fn the_podcasts_chip_lists_the_shows_and_a_row_opens_the_show() {
        let ctx = context();
        let mut library = library_with(
            vec![playlist("spotify:playlist:a", "A list")],
            vec![followed_show("spotify:show:s", "A show")],
        );
        let mut art = Art::default();
        let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);

        // This click is the harness's proof that its rects hit the drawn chip:
        // a switch is observable, unlike re-pressing the choice already made.
        let command = click(
            &ctx,
            chip_rect(&output, "PODCASTS").center(),
            &mut library,
            &mut art,
        );
        assert_eq!(library.sidebar_kind.chosen(), SidebarKind::Podcasts);
        assert_eq!(command, None, "choosing a kind loads nothing by itself");

        let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);
        let (name, size) = painted_text(&output, "A show");
        assert!(!painted(&output, "A list"), "one kind is drawn at a time");

        let command = click(
            &ctx,
            Pos2::new(name.x + 10.0, name.y + size.y / 2.0),
            &mut library,
            &mut art,
        );
        assert_eq!(
            command,
            Some(Command::LoadShow {
                show: "spotify:show:s".to_owned()
            })
        );
        assert_eq!(
            library.page(),
            Some(&Page::Show("spotify:show:s".to_owned()))
        );
    }

    #[test]
    fn a_show_name_lines_up_with_a_playlist_name() {
        let ctx = context();
        let shows = vec![followed_show("spotify:show:s", "A show")];
        let playlists = vec![playlist("spotify:playlist:a", "A list")];
        let mut art = Art::default();

        let mut library = library_with(playlists.clone(), shows.clone());
        let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);
        let (name, _) = painted_text(&output, "A list");
        let playlist_x = name.x;

        let mut library = library_with(playlists, shows);
        assert!(library.sidebar_kind.choose(SidebarKind::Podcasts));
        let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);
        let (name, _) = painted_text(&output, "A show");

        assert!(
            (name.x - playlist_x).abs() < 0.01,
            "{} vs {playlist_x}",
            name.x
        );
        assert!(
            (playlist_x - (space::S2 + space::S2 + extent::LIBRARY_ART + space::S3)).abs() < 0.01,
            "a name starts past the art slot: {playlist_x}"
        );
    }

    #[test]
    fn a_show_row_asks_for_its_cover() {
        use takofy_model::CoverUrl;

        let ctx = context();
        let mut show = followed_show("spotify:show:s", "A show");
        let url = "https://i.scdn.co/image/0123456789abcdef0123456789abcdef01234567";
        let cover = CoverUrl::from_wire(url).expect("the CDN url is admitted");
        show.cover = Some(cover.clone());
        let mut library = library_with(Vec::new(), vec![show]);
        assert!(library.sidebar_kind.choose(SidebarKind::Podcasts));
        let mut art = Art::default();
        let now = Instant::now();

        let _ = run(&ctx, Vec::new(), &mut library, &mut art);
        let requested = art.requests(now);
        assert!(
            requested
                .iter()
                .any(|command| matches!(command, Command::Fetch { url: asked } if *asked == cover)),
            "the row looks its cover up through the art store"
        );
    }

    #[test]
    fn a_show_row_names_its_kind() {
        let ctx = context();
        let mut library = library_with(Vec::new(), vec![followed_show("spotify:show:s", "A show")]);
        assert!(library.sidebar_kind.choose(SidebarKind::Podcasts));
        let mut art = Art::default();

        let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);
        assert!(painted(&output, "Podcast"), "the row says what it is");
    }

    #[test]
    fn the_open_show_is_drawn_selected() {
        let ctx = context();
        let mut library = library_with(
            Vec::new(),
            vec![
                followed_show("spotify:show:a", "A show"),
                followed_show("spotify:show:b", "B show"),
            ],
        );
        assert!(library.sidebar_kind.choose(SidebarKind::Podcasts));
        library.open(Page::Show("spotify:show:b".to_owned()));
        let mut art = Art::default();

        let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);
        let (name, size) = painted_text(&output, "B show");
        let (_, kind) = painted_text(&output, "Podcast");
        let row = Rect::from_min_size(
            Pos2::new(space::S2, name.y + (size.y + kind.y) / 2.0 - ROW_H / 2.0),
            Vec2::new(254.0 - 2.0 * space::S2, ROW_H),
        );
        let selected = output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::epaint::Shape::Rect(rect)
                if rect.rect == row && rect.fill == Tokens::DARK.palette.surface_raised)
        });
        assert!(selected, "the open show's row carries the selected fill");
    }

    #[test]
    fn a_failed_playlist_load_retries_from_the_podcasts_list() {
        let ctx = context();
        let mut library = Library::default();
        assert!(library.sidebar_kind.choose(SidebarKind::Podcasts));
        library.playlists = Load::Failed(Failure::Load);
        let mut art = Art::default();

        let (_, command) = run(&ctx, Vec::new(), &mut library, &mut art);
        assert_eq!(command, Some(Command::ReloadLibrary));

        let (_, command) = run(&ctx, Vec::new(), &mut library, &mut art);
        assert_eq!(command, None, "the retry starts the load, so it asks once");
    }

    #[test]
    fn a_failed_followed_load_retries_itself() {
        let ctx = context();
        let mut library = Library::default();
        assert!(library.sidebar_kind.choose(SidebarKind::Podcasts));
        library.playlists = Load::Ready(Vec::new());
        library.followed = Load::Failed(Failure::Followed);
        let mut art = Art::default();

        let (_, command) = run(&ctx, Vec::new(), &mut library, &mut art);
        assert_eq!(command, Some(Command::LoadFollowedShows));

        let (_, command) = run(&ctx, Vec::new(), &mut library, &mut art);
        assert_eq!(command, None, "the retry starts the load, so it asks once");
    }

    #[test]
    fn a_show_and_a_playlist_row_never_share_an_id() {
        let ctx = context();
        let mut checked = false;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, 800.0))),
                ..Default::default()
            },
            |ui| {
                assert_ne!(show_row_id(ui, "x"), playlist_row_id(ui, "x"));
                checked = true;
            },
        );
        output.textures_delta.clear();
        assert!(checked);
    }

    #[test]
    fn clicking_the_playlists_chip_keeps_the_choice() {
        let ctx = context();
        let mut library = Library::default();
        let mut art = Art::default();
        let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);

        let command = click(
            &ctx,
            chip_rect(&output, "PLAYLISTS").center(),
            &mut library,
            &mut art,
        );

        assert_eq!(
            library.sidebar_kind.chosen(),
            SidebarKind::Playlists,
            "the pressed chip is still the chosen kind"
        );
        assert_eq!(command, None, "pressing the choice asks for nothing");
    }

    #[test]
    fn clicking_a_waiting_chip_changes_nothing() {
        for label in ["ARTISTS", "ALBUMS"] {
            let ctx = context();
            let mut library = Library::default();
            let mut art = Art::default();
            let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);

            let command = click(
                &ctx,
                chip_rect(&output, label).center(),
                &mut library,
                &mut art,
            );

            assert_eq!(
                library.sidebar_kind.chosen(),
                SidebarKind::Playlists,
                "{label} is still waiting, so it cannot be pressed"
            );
            assert_eq!(command, None, "{label}");
        }
    }
}
