//! The signed-in shell: the panel grid and the regions it holds.
//!
//! `egui::Panel` is deliberately not used. Its frame margins do not collapse, so
//! a gap between two panels would be the sum of two margins, and its separator
//! line reserves stroke width inside the size that was asked for. The grid is
//! solved as rects instead, which is what the reference's CSS grid does.

use egui::{Align, Color32, Layout, Order, Rect, Stroke, StrokeKind, Ui, UiBuilder, Vec2};

use crate::drawer::{Drawer, Input};
use crate::layout::{self, Grid, NowPlaying, Sidebar};
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
            |ui| {
                sidebar::show(
                    ui,
                    Sidebar::Full,
                    screen.library,
                    screen.art,
                    screen.now,
                    tokens,
                )
            },
        ),
        // Below 900 the rail keeps the monogram and the nav icons and drops the
        // wordmark, the labels and the library, which is what the reference does.
        Sidebar::Rail => region(
            ui,
            "sidebar-rail",
            grid.sidebar,
            grid.sidebar.shrink(inset),
            |ui| {
                sidebar::show(
                    ui,
                    Sidebar::Rail,
                    screen.library,
                    screen.art,
                    screen.now,
                    tokens,
                )
            },
        ),
    };

    let bar = topbar::show(ui, screen.library, regions.sidebar, grid.topbar, tokens);
    if bar.back_to_library {
        screen.library.deselect();
    }

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
        sidebar::show(
            &mut child,
            Sidebar::Full,
            screen.library,
            screen.art,
            screen.now,
            tokens,
        );
    });

    inner.response.clicked_elsewhere()
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

    use takofy_model::{CoverUrl, TrackEntry};

    use super::*;
    use crate::cover::Art;
    use crate::library::Library;
    use crate::player::{NowPlaying, Transport};
    use crate::queue::Queue;
    use crate::signin::SignIn;
    use crate::theme::{fonts, style};

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
