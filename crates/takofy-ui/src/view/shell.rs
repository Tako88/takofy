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

    if let Some(panel) = grid.now_playing
        && regions.now_playing != NowPlaying::Hidden
    {
        command = command.or(region(
            ui,
            "now-playing",
            panel,
            panel.shrink(inset),
            |ui| {
                now_playing::show(
                    ui,
                    screen.transport,
                    screen.queue,
                    screen.art,
                    screen.now,
                    now_playing_cover,
                    tokens,
                )
            },
        ));
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
            duration_ms: None,
            named: true,
            cover,
        }
    }

    /// Draws one frame of the whole shell at `screen` and returns the covers it
    /// asked for.
    fn requested(screen: Vec2, transport: &mut Transport, queue: &mut Queue) -> Vec<CoverUrl> {
        let tokens = Tokens::DARK;
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));

        let mut state = SignIn::SignedIn;
        let mut library = Library::default();
        let mut drawer = Drawer::default();
        let mut art = Art::default();
        let now = Instant::now();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, screen)),
            ..Default::default()
        };

        art.begin_frame();
        {
            let mut screen = Screen {
                state: &mut state,
                library: &mut library,
                transport,
                queue,
                drawer: &mut drawer,
                position_ms: 0,
                art: &mut art,
                now,
            };
            let mut output = ctx.run_ui(input, |ui| {
                let _ = show(ui, &mut screen, &tokens);
            });
            output.textures_delta.clear();
        }

        art.requests(now)
            .into_iter()
            .filter_map(|command| match command {
                Command::Fetch { url } => Some(url),
                _ => None,
            })
            .collect()
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
}
