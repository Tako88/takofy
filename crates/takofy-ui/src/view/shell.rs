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
use crate::theme::{Tokens, radius, space, stroke};

use super::Screen;
use super::{now_playing, playbar, sidebar, topbar};

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
            |ui| sidebar::show(ui, Sidebar::Full, screen.library, tokens),
        ),
        // Below 900 the rail keeps the monogram and the nav icons and drops the
        // wordmark, the labels and the library, which is what the reference does.
        Sidebar::Rail => region(
            ui,
            "sidebar-rail",
            grid.sidebar,
            grid.sidebar.shrink(inset),
            |ui| sidebar::show(ui, Sidebar::Rail, screen.library, tokens),
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
        |ui| tracks_view(ui, screen.library, screen.transport, screen.queue, tokens),
    ));

    if let Some(panel) = grid.now_playing
        && regions.now_playing != NowPlaying::Hidden
    {
        region(ui, "now-playing", panel, panel.shrink(inset), |ui| {
            now_playing::show(ui, screen.transport, tokens);
        });
    }

    region(
        ui,
        "playbar",
        grid.playbar,
        grid.playbar.shrink(inset),
        |ui| {
            playbar::show(
                ui,
                screen.transport,
                screen.position_ms,
                regions.playbar,
                ui.available_rect_before_wrap(),
                tokens,
            );
        },
    );

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
        sidebar::show(&mut child, Sidebar::Full, screen.library, tokens);
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
pub(super) fn row_of(ui: &mut Ui, id: &str, rect: Rect, add: impl FnOnce(&mut Ui)) {
    ui.scope_builder(
        UiBuilder::new()
            .id_salt(id)
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.set_clip_rect(rect);
            add(ui);
        },
    );
}

/// The same, named for the rows that hold a run of controls.
pub(super) fn row_centred(ui: &mut Ui, id: &str, rect: Rect, add: impl FnOnce(&mut Ui)) {
    row_of(ui, id, rect, add);
}

/// Paints a hairline rule inside `rect`.
pub(super) fn rule(ui: &Ui, rect: Rect, color: Color32) {
    ui.painter().rect_filled(
        Rect::from_min_size(rect.min, Vec2::new(rect.width(), stroke::HAIRLINE)),
        0.0,
        color,
    );
}

/// The main pane: the selected playlist's tracks, or the state of their load.
fn tracks_view(
    ui: &mut Ui,
    library: &mut crate::library::Library,
    transport: &mut crate::player::Transport,
    queue: &mut crate::queue::Queue,
    tokens: &Tokens,
) -> Option<Command> {
    use crate::library::Load;

    ui.add_space(space::S3);

    match &library.tracks {
        Load::Idle => {
            ui.label(super::text::muted("Select a playlist.", tokens));
            return None;
        }
        Load::Loading => {
            ui.spinner();
            return None;
        }
        Load::Ready(tracks) if tracks.tracks.is_empty() => {
            ui.label(super::text::muted("No tracks in this playlist.", tokens));
            return None;
        }
        Load::Failed(failure) => {
            ui.label(super::text::muted(
                crate::signin::failure_message(*failure),
                tokens,
            ));
            if ui.button("Retry").clicked() {
                let playlist = library.selected().map(str::to_owned);
                return playlist.map(|playlist| library.select(&playlist));
            }
            return None;
        }
        Load::Ready(_) => {}
    }

    egui::ScrollArea::vertical()
        .show(ui, |ui| {
            let mut command = None;
            if let Load::Ready(tracks) = &library.tracks {
                for (index, entry) in tracks.tracks.iter().enumerate() {
                    if ui.selectable_label(false, &entry.title).double_clicked() {
                        command = queue.start(&tracks.tracks, index, transport);
                    }
                }
            }
            command
        })
        .inner
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, Vec2};

    use super::*;
    use crate::library::Library;
    use crate::message::{Command, Update};
    use crate::player::Transport;
    use crate::queue::Queue;
    use crate::theme::{Tokens, fonts, space, style};
    use takofy_model::TrackEntry;

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 400.0))),
            events,
            ..Default::default()
        }
    }

    /// Runs one pass and consumes the font-atlas delta it may have produced.
    fn run(ctx: &Context, input: RawInput, draw: impl FnMut(&mut Ui)) {
        let mut output = ctx.run_ui(input, draw);
        output.textures_delta.clear();
    }

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

    fn track(uri: &str, title: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: title.to_owned(),
            artists: String::new(),
            named: true,
        }
    }

    fn library_with(tracks: Vec<TrackEntry>) -> Library {
        let mut library = Library::default();
        library.select("spotify:playlist:a");
        library.apply(&Update::Tracks {
            playlist: "spotify:playlist:a".to_owned(),
            tracks,
        });
        library
    }

    /// The rect the first row of `tracks_view` lands on. The rows share one
    /// layout, so a probe drawn the same way finds the same rect.
    fn first_row_rect(ctx: &Context) -> Rect {
        let mut found = None;
        run(ctx, input(Vec::new()), |ui| {
            ui.add_space(space::S3);
            egui::ScrollArea::vertical().show(ui, |ui| {
                found = Some(ui.selectable_label(false, "t0").rect);
            });
        });
        found.expect("the probe row drew")
    }

    #[test]
    fn a_double_click_on_a_row_starts_that_track_through_the_queue() {
        let tokens = Tokens::DARK;
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));

        let mut library = library_with(vec![track("a", "t0"), track("b", "t1")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let centre = first_row_rect(&ctx).center();

        // Warm up so hit-testing has a previous pass to read.
        run(&ctx, input(Vec::new()), |ui| {
            let _ = tracks_view(ui, &mut library, &mut transport, &mut queue, &tokens);
        });

        let mut command = None;
        for _ in 0..2 {
            for event in [
                Event::PointerMoved(centre),
                button(centre, true),
                button(centre, false),
            ] {
                let command = &mut command;
                let library = &mut library;
                let transport = &mut transport;
                let queue = &mut queue;
                run(&ctx, input(vec![event]), |ui| {
                    if let Some(next) = tracks_view(ui, library, transport, queue, &tokens) {
                        *command = Some(next);
                    }
                });
            }
        }

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
        assert!(
            queue.can_next(),
            "the double-clicked row must start the queue, not only the transport"
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("a")
        );
    }
}
