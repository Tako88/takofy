//! The signed-in shell: the panel grid and the regions it holds.
//!
//! `egui::Panel` is deliberately not used. Its frame margins do not collapse, so
//! a gap between two panels would be the sum of two margins, and its separator
//! line reserves stroke width inside the size that was asked for. The grid is
//! solved as rects instead, which is what the reference's CSS grid does.

use egui::{Align, Color32, Layout, Order, Rect, Stroke, StrokeKind, Ui, UiBuilder, Vec2};

use crate::drawer::{Drawer, Input};
use crate::layout::{self, Grid, NowPlaying, Sidebar};
use crate::library::Library;
use crate::message::Command;
use crate::theme::{Tokens, radius, stroke};

use super::Screen;
use super::{now_playing, playbar, playlist, sidebar, topbar};

/// The signed-in shell: three columns over a playbar, each panel a hairline box
/// on the window background.
pub fn show(ui: &mut Ui, screen: &mut Screen<'_>, tokens: &Tokens) -> Option<Command> {
    let content = ui.ctx().content_rect();
    let regions = layout::regions_for(content.width());
    let shell = layout::shell_for(&regions);
    let grid = layout::grid_for(content, &shell);

    // Chrome first, so each region draws on top of its own panel. The playbar
    // has none: it sits straight on the window.
    panel(ui, grid.sidebar, tokens.palette.surface, tokens);
    panel(ui, main_panel(&grid), tokens.palette.surface, tokens);
    if let Some(now_playing) = grid.now_playing {
        panel(ui, now_playing, tokens.palette.panel, tokens);
    }

    let inset = stroke::HAIRLINE;
    let mut command = match regions.sidebar {
        Sidebar::Full => region(
            ui,
            "sidebar",
            grid.sidebar,
            grid.sidebar.shrink(inset),
            |ui| sidebar::show(ui, screen.library, screen.art, screen.now, tokens),
        ),
        // Below 900 the sidebar is not a column: the layout gives it no width
        // and the drawer toggle puts the same sidebar over the main pane.
        Sidebar::Rail => None,
    };

    // The topbar reports a nav as a request, because the crumbs borrow the
    // library they were built from. It is applied here, before the view, and
    // only if no region has asked for a command yet: a nav can move the page and
    // start a load, and one whose command was then dropped would leave that load
    // marked in flight with nothing fetching it. Side buttons and browser keys
    // ride the same request; a click wins.
    let bar = topbar::show(ui, screen.library, regions.sidebar, grid.topbar, tokens);
    command = apply_nav(
        screen.library,
        bar.nav.or_else(|| nav_from_input(ui)),
        command,
    );

    // Hazard, deferred: if an earlier region already reported a command, this
    // `or` drops whatever the view asked for — including a `LoadAlbum` the
    // library has already marked `Loading`. `settle` sees the want and the
    // in-flight load and refuses to re-issue it, so the album page would spin
    // for a reply nobody asked for. The playlist's `select` has had the same
    // hazard all along; the album page has just newly reached it. Fixing it means
    // restructuring this merge, which is not this slice's work.
    command = command.or(region(
        ui,
        "view",
        main_panel(&grid),
        grid.view.shrink(inset),
        |ui| {
            playlist::tracks_view(
                ui,
                regions.table,
                screen.library,
                screen.transport,
                screen.queue,
                screen.art,
                screen.now,
                screen.now_ms,
                tokens,
            )
        },
    ));

    // The panel and the playbar share one tile: the track the transport names,
    // looked up each frame so a cover that arrives after `play` still reaches
    // them.
    let playing_cover = screen
        .transport
        .now_playing
        .as_ref()
        .and_then(|now| now.cover.as_ref());
    let now_playing_cover = screen.art.look(playing_cover, screen.now);

    let mut clicked_row = None;
    if let Some(panel) = grid.now_playing
        && regions.now_playing != NowPlaying::Hidden
    {
        clicked_row = region(ui, "now-playing", panel, panel.shrink(inset), |ui| {
            now_playing::show(
                ui,
                screen.transport,
                screen.queue,
                screen.art,
                screen.now,
                now_playing_cover,
                tokens,
            )
        });
    }

    command = command.or(region(
        ui,
        "playbar",
        grid.playbar,
        grid.playbar.shrink(inset),
        |ui| {
            playbar::show(
                ui,
                screen.transport,
                screen.queue,
                screen.position_ms,
                regions.playbar,
                ui.available_rect_before_wrap(),
                now_playing_cover,
                tokens,
            )
        },
    ));

    // The panel only reports the row it was clicked on. Applying the jump here,
    // and only when no other region asked for a command, keeps a frame from
    // moving the queue past a click whose `Play` would then be dropped.
    if command.is_none() {
        command = clicked_row.and_then(|index| screen.queue.jump(index, screen.transport));
    }

    // The drawer is stepped after the regions, so a playlist picked from the rail
    // closes it in the same frame it is picked.
    let open = *screen.drawer == Drawer::Open;
    let clicked_outside = open && drawer_overlay(ui, grid, screen, tokens);

    let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
    let next = screen.drawer.step(Input {
        rail: regions.sidebar == Sidebar::Rail,
        toggle: bar.toggle.as_ref().is_some_and(egui::Response::clicked),
        escape,
        clicked_outside,
        selected: command.is_some(),
    });
    // Closing with Escape hands focus back to the control that opened the drawer,
    // so the keyboard does not end up nowhere.
    if open
        && next == Drawer::Closed
        && escape
        && let Some(toggle) = &bar.toggle
    {
        toggle.request_focus();
    }
    *screen.drawer = next;

    command
}

/// The rail's overflow panel: the full sidebar, over everything, above the
/// playbar so the transport stays reachable.
fn drawer_overlay(ui: &mut Ui, grid: Grid, screen: &mut Screen<'_>, tokens: &Tokens) -> bool {
    let shadow = tokens.palette.shadow_lg.as_shape(grid.drawer, radius::LG);
    ui.painter().add(shadow);

    let area = egui::Area::new(ui.scope_id().with("drawer"))
        .order(Order::Foreground)
        .fixed_pos(grid.drawer.min)
        .constrain(false)
        .fade_in(false);

    let inner = area.show(ui.ctx(), |ui| {
        ui.set_clip_rect(grid.drawer);
        panel(ui, grid.drawer, tokens.palette.surface, tokens);
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt("drawer-sidebar")
                .max_rect(grid.drawer)
                .layout(Layout::top_down(Align::Min)),
        );
        child.set_clip_rect(grid.drawer);
        sidebar::show(&mut child, screen.library, screen.art, screen.now, tokens);
    });

    inner.response.clicked_elsewhere()
}

/// Applies the topbar's nav request, unless another region already asked for a
/// command this frame.
///
/// A nav can move the page and start a load: Back onto a playlist settles it,
/// marking the tracks wanted and loading. The shell keeps the first command of
/// the frame, so a nav that lost to an earlier one would have changed the
/// library for a command nobody sends, and the page would spin for a reply that
/// was never asked for. Yielding instead keeps the page and the load in step.
fn apply_nav(
    library: &mut Library,
    nav: Option<topbar::Nav>,
    command: Option<Command>,
) -> Option<Command> {
    if command.is_some() {
        return command;
    }
    match nav {
        None => None,
        Some(topbar::Nav::Back) => library.back(),
        Some(topbar::Nav::Forward) => library.forward(),
        // The root is local state and never needs a fetch, so it adds no command
        // of its own.
        Some(topbar::Nav::Home) => {
            library.to_playlists();
            None
        }
    }
}

/// The side buttons and browser keys ask for the same nav as the topbar
/// buttons and go through `apply_nav`.
///
/// Auto-repeat is deliberate: `key_pressed` also counts repeat events, so a held
/// browser Back key steps back one page per repeat, the way a browser does.
fn nav_from_input(ui: &Ui) -> Option<topbar::Nav> {
    ui.input(|input| {
        if input.key_pressed(egui::Key::BrowserBack)
            || input.pointer.button_pressed(egui::PointerButton::Extra1)
        {
            Some(topbar::Nav::Back)
        } else if input.key_pressed(egui::Key::BrowserForward)
            || input.pointer.button_pressed(egui::PointerButton::Extra2)
        {
            Some(topbar::Nav::Forward)
        } else {
            None
        }
    })
}

/// The topbar and the view share one panel, so they share one frame.
fn main_panel(grid: &Grid) -> Rect {
    Rect::from_min_max(grid.topbar.min, grid.view.max)
}

/// Paints one panel: a hairline box on the window background.
fn panel(ui: &Ui, rect: Rect, fill: Color32, tokens: &Tokens) {
    ui.painter().rect(
        rect,
        radius::LG,
        fill,
        Stroke::new(stroke::HAIRLINE, tokens.palette.border),
        StrokeKind::Inside,
    );
}

/// Draws one region into its content rect, clipped to the panel it sits on.
fn region<R>(
    ui: &mut Ui,
    id: &str,
    panel: Rect,
    content: Rect,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    ui.scope_builder(
        UiBuilder::new()
            .id_salt(id)
            .max_rect(content)
            .layout(Layout::top_down(Align::Min)),
        |ui| {
            // A child inherits its parent's clip rect, so without this a panel's
            // contents would spill into the channels beside it.
            ui.set_clip_rect(panel);
            add_contents(ui)
        },
    )
    .inner
}

/// Runs `add` inside a left-to-right child at `rect`, so a control that
/// allocates its own space still lands where the design wants it.
pub(super) fn row_of<R>(ui: &mut Ui, id: &str, rect: Rect, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(
        UiBuilder::new()
            .id_salt(id)
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.set_clip_rect(rect);
            add(ui)
        },
    )
    .inner
}

/// A row's vertical centre: a region is the whole column's height, so a row that
/// draws its own rects has to centre them itself rather than trust the cursor.
pub(super) fn row_centre(ui: &Ui) -> f32 {
    ui.max_rect().center().y
}

/// The same, named for the rows that hold a run of controls.
pub(super) fn row_centred<R>(
    ui: &mut Ui,
    id: &str,
    rect: Rect,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    row_of(ui, id, rect, add)
}

/// Paints a hairline rule inside `rect`.
pub(super) fn rule(ui: &Ui, rect: Rect, color: Color32) {
    ui.painter().rect_filled(
        Rect::from_min_size(rect.min, Vec2::new(rect.width(), stroke::HAIRLINE)),
        0.0,
        color,
    );
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use takofy_model::{CoverUrl, PlaylistEntry, TrackEntry};

    use super::*;
    use crate::cover::Art;
    use crate::library::{Library, Load};
    use crate::message::Update;
    use crate::player::{NowPlaying, Transport};
    use crate::queue::Queue;
    use crate::route::Page;
    use crate::signin::SignIn;
    use crate::theme::{extent, fonts, style};

    fn cover(tail: &str) -> CoverUrl {
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
    }

    fn playing(cover: CoverUrl) -> Transport {
        Transport {
            now_playing: Some(NowPlaying {
                uri: "spotify:track:x".to_owned(),
                title: "A Title".to_owned(),
                artists: "A Band".to_owned(),
                cover: Some(cover),
            }),
            ..Transport::default()
        }
    }

    fn track(index: usize, cover: Option<CoverUrl>) -> TrackEntry {
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
            cover,
        }
    }

    /// The window's own frame, kept across passes because egui's hit test reads
    /// the previous one.
    struct Window {
        ctx: egui::Context,
        size: Vec2,
        state: SignIn,
        library: Library,
        drawer: Drawer,
        art: Art,
        now: Instant,
    }

    impl Window {
        fn new(size: Vec2) -> Self {
            let tokens = Tokens::DARK;
            let ctx = egui::Context::default();
            ctx.set_fonts(fonts::definitions());
            ctx.set_global_style(style(&tokens));
            Self {
                ctx,
                size,
                state: SignIn::SignedIn,
                library: Library::default(),
                drawer: Drawer::default(),
                art: Art::default(),
                now: Instant::now(),
            }
        }

        /// Draws one frame, returning the command the shell reported and the
        /// frame's output.
        fn frame(
            &mut self,
            transport: &mut Transport,
            queue: &mut Queue,
            events: Vec<egui::Event>,
        ) -> (Option<Command>, egui::FullOutput) {
            let tokens = Tokens::DARK;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, self.size)),
                events,
                ..Default::default()
            };
            self.art.begin_frame();
            let mut command = None;
            let mut screen = Screen {
                state: &mut self.state,
                library: &mut self.library,
                transport,
                queue,
                drawer: &mut self.drawer,
                position_ms: 0,
                art: &mut self.art,
                now: self.now,
                now_ms: 0,
            };
            let mut output = self.ctx.run_ui(input, |ui| {
                command = show(ui, &mut screen, &tokens);
            });
            output.textures_delta.clear();
            (command, output)
        }

        /// The covers the last frame's draw asked for.
        fn requests(&mut self) -> Vec<CoverUrl> {
            self.art
                .requests(self.now)
                .into_iter()
                .filter_map(|command| match command {
                    Command::Fetch { url } => Some(url),
                    _ => None,
                })
                .collect()
        }

        /// Clicks `at` over the passes egui's hit test needs, returning the
        /// command the shell reported.
        fn click(
            &mut self,
            transport: &mut Transport,
            queue: &mut Queue,
            at: egui::Pos2,
        ) -> Option<Command> {
            let _ = self.frame(transport, queue, Vec::new());
            let mut command = None;
            for event in crate::view::test_support::click_events(at) {
                if let Some(next) = self.frame(transport, queue, vec![event]).0 {
                    command = Some(next);
                }
            }
            command
        }
    }

    /// Where a label was painted, so a click lands on a row the shell drew
    /// rather than on a coordinate a test guessed.
    fn painted_at(output: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text() == label => Some(text.pos),
                _ => None,
            })
    }

    /// The rect the first text shape with `label` inside `area` was painted in.
    fn text_in(output: &egui::FullOutput, label: &str, area: Rect) -> Option<Rect> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text)
                    if text.galley.text() == label && area.contains(text.pos) =>
                {
                    Some(Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
    }

    /// The topbar's rect, solved the same way the shell solves it.
    fn topbar_rect(window: &Window) -> Rect {
        let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
        let regions = layout::regions_for(content.width());
        let shell = layout::shell_for(&regions);
        layout::grid_for(content, &shell).topbar
    }

    /// The view's rect, solved the same way the shell solves it. A same-frame
    /// destination is read from the text the view paints here, never from the
    /// sidebar, which draws before the nav is applied.
    fn view_rect(window: &Window) -> Rect {
        let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
        let regions = layout::regions_for(content.width());
        let shell = layout::shell_for(&regions);
        layout::grid_for(content, &shell).view
    }

    /// One press or release of `key` with no modifiers.
    fn key(key: egui::Key, pressed: bool) -> egui::Event {
        key_with_repeat(key, pressed, false)
    }

    /// One press of `key` as a held key's auto-repeat, never a release.
    fn key_repeat(key: egui::Key) -> egui::Event {
        key_with_repeat(key, true, true)
    }

    fn key_with_repeat(key: egui::Key, pressed: bool, repeat: bool) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// One press or release of `button` at `pos`.
    ///
    /// The side buttons are read globally, so most tests pass `Pos2::ZERO`: a
    /// `Primary` press there only arms a click on whatever sits top-left, which
    /// is benign without a release. A frame that also clicks passes the click's
    /// own position, because that is where the press would arrive on real
    /// hardware. `test_support::button` is `Primary`-only, so it is not used
    /// here.
    fn side(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// The topbar's round buttons, left to right: Home, back, forward. Found from
    /// the buttons' painted frames rather than from coordinates a test guessed.
    fn topbar_buttons(output: &egui::FullOutput) -> Vec<Rect> {
        let mut rects = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if shape.stroke.width > 0.0
                        && (shape.rect.width() - extent::BUTTON).abs() < 0.01
                        && (shape.rect.height() - extent::BUTTON).abs() < 0.01 =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        rects.sort_by(|left, right| left.left().total_cmp(&right.left()));
        rects
    }

    /// The search field's painted box, found by its height rather than by a
    /// coordinate a test guessed.
    fn search_field(output: &egui::FullOutput) -> Option<Rect> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if (shape.rect.height() - extent::SEARCH_H).abs() < 0.01
                        && shape.rect.width() > 100.0 =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
    }

    /// The sidebar's highlighted library rows, which wear a bone rule at the
    /// left edge.
    fn selected_rows(output: &egui::FullOutput) -> usize {
        let tokens = Tokens::DARK;
        output
            .shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape) => {
                    shape.fill == tokens.palette.text
                        && (shape.rect.width() - stroke::RULE).abs() < 0.01
                        && shape.rect.height() > extent::ROW_H
                        && shape.rect.left() < extent::SIDEBAR_W
                }
                _ => false,
            })
            .count()
    }

    const PLAYLIST: &str = "spotify:playlist:a";
    const ALBUM: &str = "spotify:album:b";
    const ALBUM_B: &str = "spotify:album:c";

    /// Puts one named, ready playlist on screen.
    fn ready_playlist(window: &mut Window) {
        window.library.apply(&Update::Playlists(vec![PlaylistEntry {
            uri: PLAYLIST.to_owned(),
            name: "Mix".to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
        }]));
        window.library.select(PLAYLIST);
        window.library.apply(&Update::Tracks {
            playlist: PLAYLIST.to_owned(),
            tracks: Vec::new(),
            cover: None,
        });
    }

    /// The middle of a three-page history: album A on screen, the playlist
    /// behind it and album B ahead.
    fn history_of_three(window: &mut Window) {
        ready_playlist(window);
        window.library.open(Page::Album(ALBUM.to_owned()));
        window.library.open(Page::Album(ALBUM_B.to_owned()));
        window.library.back();
    }

    /// In drawer mode the layout hands the toggle a whole column; the button in
    /// it is still a button, the same square as its neighbours.
    #[test]
    fn the_drawer_toggle_is_a_button_not_a_column() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(800.0, 900.0));
        ready_playlist(&mut window);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let bar = topbar_rect(&window);
        let buttons = topbar_buttons(&frame);
        assert_eq!(buttons.len(), 4, "the toggle, Home, back and forward");
        for button in &buttons {
            assert!(
                (button.height() - extent::BUTTON).abs() < 0.01,
                "a topbar button {button:?} is not one button tall"
            );
        }

        // Nothing drawn as a frame in the bar is taller than a button: a frame
        // stretched to the region came from placing a control on the column
        // rather than in it. The search field is the one box with its own height.
        for clipped in &frame.shapes {
            if let egui::Shape::Rect(shape) = &clipped.shape
                && shape.stroke.width > 0.0
                && bar.contains_rect(shape.rect)
                && (shape.rect.height() - extent::SEARCH_H).abs() > 0.01
            {
                assert!(
                    shape.rect.height() <= extent::BUTTON + 0.01,
                    "the topbar draws a {:?} frame, taller than a button",
                    shape.rect
                );
            }
        }
    }

    #[test]
    fn every_topbar_item_shares_one_baseline() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        ready_playlist(&mut window);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let bar = topbar_rect(&window).center().y;
        let buttons = topbar_buttons(&frame);
        assert_eq!(buttons.len(), 3, "Home, back and forward");
        for button in &buttons {
            assert!(
                (button.center().y - bar).abs() < 0.01,
                "a topbar button at {:?} is not on the bar's centre {bar}",
                button.center()
            );
        }
        let field = search_field(&frame).expect("the topbar paints the search field");
        assert!(
            (field.center().y - bar).abs() < 0.01,
            "the search field at {:?} is not on the bar's centre {bar}",
            field.center()
        );
    }

    #[test]
    fn the_history_buttons_move_between_pages() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        ready_playlist(&mut window);
        window.library.open(Page::Album(ALBUM.to_owned()));

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let buttons = topbar_buttons(&frame);
        assert_eq!(buttons.len(), 3, "the topbar paints Home, back and forward");

        assert_eq!(
            window.click(&mut transport, &mut queue, buttons[1].center()),
            None,
            "back into a ready playlist needs no load"
        );
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist(PLAYLIST.to_owned()))
        );

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let buttons = topbar_buttons(&frame);
        assert_eq!(
            window.click(&mut transport, &mut queue, buttons[2].center()),
            None
        );
        assert_eq!(window.library.page(), Some(&Page::Album(ALBUM.to_owned())));
    }

    #[test]
    fn the_history_buttons_do_nothing_with_no_history() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let buttons = topbar_buttons(&frame);
        assert_eq!(buttons.len(), 3, "the topbar paints Home, back and forward");

        for button in buttons {
            assert_eq!(
                window.click(&mut transport, &mut queue, button.center()),
                None
            );
            assert_eq!(window.library.page(), None);
        }
    }

    #[test]
    fn the_back_side_button_goes_back() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        history_of_three(&mut window);

        let (command, frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![side(egui::Pos2::ZERO, egui::PointerButton::Extra1, true)],
        );
        assert_eq!(command, None, "back onto a ready playlist needs no load");
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist(PLAYLIST.to_owned()))
        );
        assert!(
            text_in(&frame, "Mix", view_rect(&window)).is_some(),
            "the view paints the playlist in the frame the nav applies"
        );

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![side(egui::Pos2::ZERO, egui::PointerButton::Extra1, false)],
        );
    }

    #[test]
    fn the_forward_side_button_goes_forward() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        history_of_three(&mut window);
        window.library.back();
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist(PLAYLIST.to_owned()))
        );

        // The control frame: the predicate the next frame asserts on is live.
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(text_in(&frame, "Mix", view_rect(&window)).is_some());

        let (command, frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![side(egui::Pos2::ZERO, egui::PointerButton::Extra2, true)],
        );
        assert_eq!(command, None);
        assert_eq!(window.library.page(), Some(&Page::Album(ALBUM.to_owned())));
        // The album page draws a spinner until its load settles, and a band and
        // a table after that. Its load has only just started here, so the
        // same-frame proof that the view moved is that the playlist band is no
        // longer painted in the view's own rect. The sidebar and topbar are
        // excluded: both draw before the nav is applied.
        assert!(
            text_in(&frame, "Mix", view_rect(&window)).is_none(),
            "the view leaves the playlist in the frame the nav applies"
        );

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![side(egui::Pos2::ZERO, egui::PointerButton::Extra2, false)],
        );
    }

    #[test]
    fn browser_back_and_forward_keys_do_the_same() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        history_of_three(&mut window);

        let (command, frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::BrowserBack, true)],
        );
        assert_eq!(command, None, "back onto a ready playlist needs no load");
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist(PLAYLIST.to_owned()))
        );
        assert!(text_in(&frame, "Mix", view_rect(&window)).is_some());

        // A fresh window, so the two cases share no input state.
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        history_of_three(&mut window);
        window.library.back();

        // The control frame: the predicate the next frame asserts on is live.
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(text_in(&frame, "Mix", view_rect(&window)).is_some());

        let (command, frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::BrowserForward, true)],
        );
        assert_eq!(command, None);
        assert_eq!(window.library.page(), Some(&Page::Album(ALBUM.to_owned())));
        assert!(text_in(&frame, "Mix", view_rect(&window)).is_none());
    }

    /// Auto-repeat is intended, the way a browser behaves: a held Back key
    /// steps back once per repeat, so each frame moves exactly one page rather
    /// than draining the history in one go.
    #[test]
    fn a_held_browser_key_steps_back_once_per_frame() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        ready_playlist(&mut window);
        window.library.open(Page::Album(ALBUM.to_owned()));
        window.library.open(Page::Album(ALBUM_B.to_owned()));

        let (command, _frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![key_repeat(egui::Key::BrowserBack)],
        );
        assert_eq!(
            command,
            Some(Command::LoadAlbum {
                album: ALBUM.to_owned()
            }),
            "album B's want superseded album A's, so Back asks for A again"
        );
        assert_eq!(
            window.library.page(),
            Some(&Page::Album(ALBUM.to_owned())),
            "one repeat takes one step back, not two"
        );

        let (command, _frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![key_repeat(egui::Key::BrowserBack)],
        );
        assert_eq!(command, None, "back onto a ready playlist needs no load");
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist(PLAYLIST.to_owned())),
            "the next repeat takes the next step, one page per frame"
        );
    }

    #[test]
    fn side_button_back_into_a_stale_playlist_loads_it() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        stale_playlist_behind_an_album(&mut window.library);

        let (command, _frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![side(egui::Pos2::ZERO, egui::PointerButton::Extra1, true)],
        );
        assert_eq!(
            command,
            Some(Command::LoadTracks {
                playlist: PLAYLIST.to_owned()
            })
        );
        assert_eq!(window.library.tracks, Load::Loading);

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![side(egui::Pos2::ZERO, egui::PointerButton::Extra1, false)],
        );
    }

    #[test]
    fn back_beats_forward_in_one_frame() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        history_of_three(&mut window);

        let (command, frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![
                side(egui::Pos2::ZERO, egui::PointerButton::Extra2, true),
                side(egui::Pos2::ZERO, egui::PointerButton::Extra1, true),
            ],
        );
        assert_eq!(command, None);
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist(PLAYLIST.to_owned())),
            "Back is checked first, so one frame gives one action"
        );
        assert!(text_in(&frame, "Mix", view_rect(&window)).is_some());

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![
                side(egui::Pos2::ZERO, egui::PointerButton::Extra1, false),
                side(egui::Pos2::ZERO, egui::PointerButton::Extra2, false),
            ],
        );
    }

    #[test]
    fn side_buttons_do_nothing_with_no_history() {
        let cases = [
            (
                side(egui::Pos2::ZERO, egui::PointerButton::Extra1, true),
                Some(side(egui::Pos2::ZERO, egui::PointerButton::Extra1, false)),
            ),
            (
                side(egui::Pos2::ZERO, egui::PointerButton::Extra2, true),
                Some(side(egui::Pos2::ZERO, egui::PointerButton::Extra2, false)),
            ),
            (key(egui::Key::BrowserBack, true), None),
            (key(egui::Key::BrowserForward, true), None),
        ];

        for (input, release) in cases {
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let mut window = Window::new(Vec2::new(1400.0, 900.0));

            let (command, _frame) = window.frame(&mut transport, &mut queue, vec![input]);
            assert_eq!(command, None, "there is nowhere to go");
            assert_eq!(window.library.page(), None);
            assert_eq!(window.library.tracks, Load::Idle);

            if let Some(release) = release {
                let _ = window.frame(&mut transport, &mut queue, vec![release]);
            }
        }
    }

    #[test]
    fn releases_and_other_buttons_are_ignored() {
        let cases = [
            side(egui::Pos2::ZERO, egui::PointerButton::Extra1, false),
            side(egui::Pos2::ZERO, egui::PointerButton::Primary, true),
            side(egui::Pos2::ZERO, egui::PointerButton::Secondary, true),
            side(egui::Pos2::ZERO, egui::PointerButton::Middle, true),
            key(egui::Key::Escape, true),
            key(egui::Key::ArrowLeft, true),
            key(egui::Key::BrowserBack, false),
        ];

        for input in cases {
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let mut window = Window::new(Vec2::new(1400.0, 900.0));
            history_of_three(&mut window);

            let (command, _frame) = window.frame(&mut transport, &mut queue, vec![input]);
            assert_eq!(command, None);
            assert_eq!(
                window.library.page(),
                Some(&Page::Album(ALBUM.to_owned())),
                "only a press of a nav input moves the page"
            );
        }
    }

    /// The end-to-end pin of click-wins: one frame carries a primary press, a
    /// primary release and an `Extra2` press, and the click's Back must win over
    /// the side button's Forward.
    ///
    /// The `Extra2` press lands at the click's own position, which is where it
    /// would arrive on real hardware, so the frame is shaped like a real one. The
    /// second window is the control: the same events with the click on the inert
    /// search field, so only the `Extra2` press can act, and it must move
    /// Forward. That proves the side press is live in the frame where Back won,
    /// so the test cannot pass with the side buttons unwired.
    #[test]
    fn a_topbar_click_beats_a_side_button() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        history_of_three(&mut window);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let back = topbar_buttons(&frame)[1];
        assert!(window.library.can_back(), "history exists, so Back is live");

        // The hover frame, so frame 3's hit test sees the button there.
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::PointerMoved(back.center())],
        );

        let (command, frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![
                crate::view::test_support::button(back.center(), true),
                crate::view::test_support::button(back.center(), false),
                side(back.center(), egui::PointerButton::Extra2, true),
            ],
        );
        assert_eq!(command, None);
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist(PLAYLIST.to_owned())),
            "the click's Back wins over the side button's Forward"
        );
        assert!(text_in(&frame, "Mix", view_rect(&window)).is_some());

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![side(back.center(), egui::PointerButton::Extra2, false)],
        );

        // The liveness control: no click to win, so the same-frame `Extra2`
        // press must move Forward. The click lands on the topbar's right edge,
        // where the search field paints no widget.
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        history_of_three(&mut window);
        let inert = topbar_rect(&window).right_center() - Vec2::new(40.0, 0.0);

        let (command, _frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![
                crate::view::test_support::button(inert, true),
                crate::view::test_support::button(inert, false),
                side(inert, egui::PointerButton::Extra2, true),
            ],
        );
        assert_eq!(
            command,
            Some(Command::LoadAlbum {
                album: ALBUM_B.to_owned()
            }),
            "album A's want superseded album B's, so Forward asks for B again"
        );
        assert_eq!(
            window.library.page(),
            Some(&Page::Album(ALBUM_B.to_owned())),
            "with no click to win, the same-frame side press moves Forward"
        );
    }

    #[test]
    fn the_playlists_crumb_returns_to_the_root_and_back_loads_the_playlist() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        ready_playlist(&mut window);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let crumb = text_in(&frame, "PLAYLISTS", topbar_rect(&window))
            .expect("the trail's Playlists crumb is painted");
        assert_eq!(
            window.click(&mut transport, &mut queue, crumb.center()),
            None
        );
        assert_eq!(window.library.page(), None, "the crumb returns to the root");

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let back = topbar_buttons(&frame)[1];
        assert_eq!(
            window.click(&mut transport, &mut queue, back.center()),
            Some(Command::LoadTracks {
                playlist: PLAYLIST.to_owned()
            }),
            "the root cleared the wanted load, so Back reloads it"
        );
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist(PLAYLIST.to_owned()))
        );
    }

    #[test]
    fn back_into_a_playlist_whose_tracks_were_cleared_loads_it() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        window.library.apply(&Update::Playlists(vec![PlaylistEntry {
            uri: PLAYLIST.to_owned(),
            name: "Mix".to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
        }]));
        window.library.select(PLAYLIST);
        window.library.to_playlists();
        assert_eq!(window.library.tracks, Load::Idle);
        assert_eq!(window.library.page(), None);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let back = topbar_buttons(&frame)[1];
        assert_eq!(
            window.click(&mut transport, &mut queue, back.center()),
            Some(Command::LoadTracks {
                playlist: PLAYLIST.to_owned()
            })
        );
    }

    #[test]
    fn the_sidebar_highlight_clears_on_an_album_page() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        ready_playlist(&mut window);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(
            selected_rows(&frame),
            1,
            "the current playlist is highlighted"
        );

        window.library.open(Page::Album(ALBUM.to_owned()));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(
            selected_rows(&frame),
            0,
            "no playlist is current on an album page"
        );
    }

    /// A history that holds a stale playlist behind an album, so Back has to
    /// start the playlist's load again.
    fn stale_playlist_behind_an_album(library: &mut Library) {
        library.apply(&Update::Playlists(vec![PlaylistEntry {
            uri: PLAYLIST.to_owned(),
            name: "Mix".to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
        }]));
        library.select(PLAYLIST);
        library.apply(&Update::TracksFailed {
            playlist: PLAYLIST.to_owned(),
            failure: crate::message::Failure::Load,
        });
        library.open(Page::Album(ALBUM.to_owned()));
    }

    /// A nav can settle a page: Back onto a playlist marks its load wanted and
    /// starts it. Keeping an earlier command of the frame and dropping the nav's
    /// would leave that load marked in flight with nothing fetching it, so the
    /// page would spin for a reply nobody asked for. A losing nav must therefore
    /// not touch the library at all.
    ///
    /// Two activations in one frame need a keyboard and a pointer together, which
    /// the harness above cannot stage: egui resolves one click position per
    /// frame. So the collision is driven through the shell's nav step, which is
    /// where the fix lives.
    ///
    /// A nav from a side button or a browser key merges into the same call, so
    /// this guard covers it too. The sidebar-command case is not duplicated with
    /// a second shell frame: the shell keeps the region's command before the nav
    /// is ever looked at.
    #[test]
    fn a_nav_that_loses_the_frame_leaves_the_library_alone() {
        let mut library = Library::default();
        stale_playlist_behind_an_album(&mut library);
        let page = library.page().cloned();
        let tracks = library.tracks.clone();

        let won = Command::LoadTracks {
            playlist: ALBUM.to_owned(),
        };
        let command = apply_nav(&mut library, Some(topbar::Nav::Back), Some(won.clone()));

        assert_eq!(command, Some(won), "the earlier command keeps the frame");
        assert_eq!(
            library.page(),
            page.as_ref(),
            "the losing nav must not move the page"
        );
        assert_eq!(
            library.tracks, tracks,
            "and must not mark a load that nothing would fetch"
        );
        assert_eq!(library.page(), Some(&Page::Album(ALBUM.to_owned())));
    }

    /// The same Back, with no other command in the frame, still settles the page
    /// and reports the load it started.
    #[test]
    fn a_nav_applies_when_no_other_region_asked() {
        let mut library = Library::default();
        stale_playlist_behind_an_album(&mut library);

        assert_eq!(
            apply_nav(&mut library, Some(topbar::Nav::Back), None),
            Some(Command::LoadTracks {
                playlist: PLAYLIST.to_owned()
            })
        );
        assert_eq!(library.page(), Some(&Page::Playlist(PLAYLIST.to_owned())));
        assert_eq!(library.tracks, Load::Loading);
    }

    /// Draws one frame of the whole shell at `screen` and returns the covers it
    /// asked for.
    fn requested(screen: Vec2, transport: &mut Transport, queue: &mut Queue) -> Vec<CoverUrl> {
        let mut window = Window::new(screen);
        let _ = window.frame(transport, queue, Vec::new());
        window.requests()
    }

    #[test]
    fn the_playbar_tile_is_requested_even_with_the_panel_hidden() {
        // Narrower than the panel breakpoint, so the Now Playing panel is not
        // drawn and only the playbar can want a cover — the queue's rows too.
        let mut transport = playing(cover("abc"));
        let tracks = vec![
            track(0, Some(cover("abc"))),
            track(1, Some(cover("row1"))),
            track(2, Some(cover("row2"))),
        ];
        let mut queue = Queue::default();
        queue.start(&tracks, 0, &mut transport);

        assert_eq!(
            requested(Vec2::new(900.0, 600.0), &mut transport, &mut queue),
            vec![cover("abc")],
            "a hidden panel must not ask for its rows' covers"
        );
    }

    #[test]
    fn a_visible_panel_requests_the_visible_rows_covers() {
        let mut transport = playing(cover("abc"));
        let tracks = vec![
            track(0, Some(cover("abc"))),
            track(1, Some(cover("row1"))),
            track(2, Some(cover("row2"))),
        ];
        let mut queue = Queue::default();
        queue.start(&tracks, 0, &mut transport);

        let mut asked = requested(Vec2::new(1400.0, 900.0), &mut transport, &mut queue);
        asked.sort_by(|left, right| left.as_str().cmp(right.as_str()));

        let mut expected = vec![cover("abc"), cover("row1"), cover("row2")];
        expected.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        assert_eq!(
            asked, expected,
            "the panel must ask for the rows it draws, and the playbar for its tile"
        );
    }

    #[test]
    fn a_click_on_a_queue_row_plays_that_entry() {
        // The panel reports the row and the shell is what moves the queue and
        // the transport, so nothing can name a track the transport never got.
        let mut transport = playing(cover("abc"));
        let tracks = vec![
            track(0, Some(cover("abc"))),
            track(1, Some(cover("row1"))),
            track(2, Some(cover("row2"))),
        ];
        let mut queue = Queue::default();
        queue.start(&tracks, 0, &mut transport);

        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = painted_at(&frame, "t2").expect("row t2 is painted");

        assert_eq!(
            window.click(&mut transport, &mut queue, at + Vec2::new(6.0, 6.0)),
            Some(Command::Play {
                uri: "t2".to_owned()
            })
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("t2"),
            "the shell's jump must reach the transport, not only the queue"
        );
    }
}
