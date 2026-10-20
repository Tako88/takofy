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
                                        let (_, row) = ui
                                            .allocate_space(Vec2::new(ui.available_width(), ROW_H));
                                        let selected =
                                            current.as_deref() == Some(entry.uri.as_str());
                                        let cover = art.look(entry.cover.as_ref(), now);
                                        if one(
                                            ui,
                                            row,
                                            &entry.uri,
                                            &entry.name,
                                            cover,
                                            selected,
                                            tokens,
                                        ) {
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
        // Its chip is still waiting, so the list it would show is not drawn;
        // the arm stands in for it.
        SidebarKind::Podcasts => None,
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

/// One row: a placeholder tile, the name and the kind.
///
/// The tile stands in until covers are fetched. A selected row borrows the
/// reference's language for the current navigation item — a raised fill and a
/// bone rule at the left edge — because the reference defines no selected row.
fn one(
    ui: &mut Ui,
    rect: Rect,
    uri: &str,
    name: &str,
    cover: Cover,
    selected: bool,
    tokens: &Tokens,
) -> bool {
    // The uri, not the name: two playlists may share a name, and two widgets may
    // not share an id.
    let response = ui.interact(
        rect,
        ui.scope_id().with(("library-row", uri)),
        Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, name));

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
        "Playlist",
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

    if response.has_focus() {
        controls::focus_ring(ui, rect, tokens);
    }

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
