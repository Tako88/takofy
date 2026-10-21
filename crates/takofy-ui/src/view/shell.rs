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
use crate::route::Page;
use crate::search::Act;
use crate::theme::{Tokens, radius, stroke};

use super::Screen;
use super::{now_playing, playbar, playlist, search_menu, sidebar, topbar};

/// The signed-in shell: three columns over a playbar, each panel a hairline box
/// on the window background.
pub fn show(ui: &mut Ui, screen: &mut Screen<'_>, tokens: &Tokens) -> Option<Command> {
    // Before any region draws: see `take_space`.
    let space = take_space(ui);

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

    // The panel's lyrics follow the transport: a different track drops the load
    // and keeps the tab the reader chose.
    if grid.now_playing.is_some() && regions.now_playing != NowPlaying::Hidden {
        screen
            .library
            .lyrics
            .sync(screen.transport.now_playing.as_ref());
    }

    let mut reply = now_playing::Reply::default();
    if let Some(panel) = grid.now_playing
        && regions.now_playing != NowPlaying::Hidden
    {
        reply = region(ui, "now-playing", panel, panel.shrink(inset), |ui| {
            now_playing::show(
                ui,
                screen.transport,
                screen.queue,
                &screen.library.lyrics,
                screen.position_ms,
                &mut now_playing::Frame {
                    art: screen.art,
                    now: screen.now,
                    cover: now_playing_cover,
                    tokens,
                },
            )
        });
    }
    if let Some(tab) = reply.tab {
        screen.library.lyrics.select(tab);
    }
    if reply.retry {
        screen.library.lyrics.retry();
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

    // The panel only reports the row it was asked to play. Applying the jump
    // here, and only when no other region asked for a command, keeps a frame
    // from moving the queue past a click whose `Play` would then be dropped.
    // The pane's press, lift and drop target are applied with the drag wiring.
    if command.is_none() {
        command = reply
            .queue
            .play
            .and_then(|index| screen.queue.jump(index, screen.transport));
    }
    // The lyrics fetch is the open tab's, and it is asked for only once the
    // frame's own command is settled: a load marked `Loading` for a command that
    // was then dropped would wait forever.
    if command.is_none() {
        command = screen
            .library
            .lyrics
            .request(screen.transport.now_playing.as_ref());
    }

    // The menu draws after every region and before the drawer overlay: it hangs
    // over the view, and a drawer opened on top of it must stay on top. It is
    // anchored to the field rect the topbar reported, so it is the field's own
    // width and never the layout's guess (R7).
    let mut menu = search_menu::Menu::default();
    if screen.library.search.is_open()
        && let Some(field) = bar.search_field
    {
        menu = search_menu::show(
            ui,
            field.rect,
            &mut screen.library.search,
            screen.transport,
            screen.art,
            screen.now,
            tokens,
        );
    }

    // A requested action is applied only when no region took the frame's one
    // command. `queue.start` and `library.open` mutate before they return, so
    // applying one whose command would then be dropped leaves state changed for
    // a command nobody sends. A dropped action changes nothing, and the menu
    // stays open so the user can click again.
    let act = bar
        .search_field
        .map(|field| field.act)
        .filter(|act| *act != Act::None)
        .or(menu.click);
    if command.is_none()
        && let Some(act) = act
    {
        command = match act {
            Act::Play(index) => match screen.library.search.answer() {
                Some(tracks) => {
                    // R10: the loaded row's glyph is pause, and its action is the
                    // transport's toggle rather than a second start of the same
                    // track. A stopped transport has nothing to pause, so it
                    // starts the answer at this row like any other. The one
                    // decision is the transport's, so the glyph the row drew and
                    // the action here cannot disagree.
                    let held = tracks
                        .get(index)
                        .is_some_and(|entry| screen.transport.hold(&entry.uri).held);
                    if held {
                        screen.queue.toggle(screen.transport)
                    } else {
                        // R16: the queue becomes the whole answer, entered at the
                        // hit, so next and previous walk the results.
                        screen.queue.start(tracks, index, screen.transport)
                    }
                }
                None => None,
            },
            Act::OpenResults => {
                // The page takes the keyboard with the menu it closes. Only an
                // applied action does: a dropped `OpenResults` leaves the menu
                // open with the field still focused.
                if let Some(field) = bar.search_field {
                    ui.memory_mut(|memory| memory.surrender_focus(field.id));
                }
                screen
                    .library
                    .search
                    .key()
                    .and_then(|query| screen.library.open(Page::Search(query)))
            }
            Act::None | Act::Close => None,
        };
        screen.library.search.close();
    }

    // The shell is the only place that closes the menu on blur, once per frame,
    // after the field and the menu have both been drawn. The press that starts a
    // row click may surrender the field's focus a frame before the release
    // reports the click, so the pointer's geometry — not egui's surrender timing
    // — is what keeps the menu alive across that frame.
    let engaged = {
        let (position, active) = ui.input(|input| {
            (
                input
                    .pointer
                    .hover_pos()
                    .or_else(|| input.pointer.interact_pos()),
                input.pointer.any_pressed()
                    || input.pointer.any_down()
                    || input.pointer.any_released(),
            )
        });
        let on_field = position.is_some_and(|pos| {
            bar.search_field
                .is_some_and(|field| field.rect.contains(pos))
        });
        let on_menu = position.is_some_and(|pos| menu.rect.is_some_and(|rect| rect.contains(pos)));
        active && (on_field || on_menu)
    };
    let focused = bar.search_field.is_some_and(|field| field.has_focus);
    screen.library.search.settle(focused, engaged);
    // A press and release that ended on the menu without leaving it engaged the
    // menu across the frame that blurred the field, and nothing else asks for
    // the frame that closes it. Ask here, or the menu stays drawn over the view
    // until the next input event.
    if screen.library.search.is_open() && !focused && engaged {
        ui.ctx().request_repaint();
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

    // The space toggles the track the transport already names, and nothing else:
    // a row starts a track on Enter or a double click. It comes after the drawer
    // step so that pausing does not also close a drawer nobody picked from.
    if command.is_none() && space && screen.transport.now_playing.is_some() {
        command = screen.queue.toggle(screen.transport);
    }

    // After every edit: `logic` runs before `ui`, so a read this frame armed —
    // the last keystroke, Back onto a dropped page, a Retry — would otherwise
    // wait for the next input event. `app.rs` asks the same question after the
    // draw; asking here too keeps the contract inside the frame that arms it.
    let page = screen.library.page_search_query().map(str::to_owned);
    if screen.library.search.wants(page.as_deref()) {
        ui.ctx().request_repaint();
    }

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

/// Takes the frame's space for the transport, if there is one to take.
///
/// It is taken before anything draws, because egui turns a space into a click for
/// whatever holds focus: a focused row would start its own track in the same
/// frame the transport paused. The press and the auto-repeats behind it are all
/// taken away for that reason, and only a first press toggles anything.
///
/// Returns whether this frame held that first press. A space with a modifier, or
/// one pressed while a field is typing, is left alone.
fn take_space(ui: &Ui) -> bool {
    if ui.ctx().text_edit_focused() {
        return false;
    }

    let mut owned = false;
    let mut first = false;
    ui.input(|input| {
        for event in &input.events {
            if let Some(repeat) = transport_space(event) {
                owned = true;
                first |= !repeat;
            }
        }
    });
    if !owned {
        return false;
    }

    ui.input_mut(|input| {
        input
            .events
            .retain(|event| transport_space(event).is_none())
    });
    first
}

/// `Some(repeat)` when this event is a space the transport owns.
fn transport_space(event: &egui::Event) -> Option<bool> {
    match event {
        egui::Event::Key {
            key: egui::Key::Space,
            pressed: true,
            repeat,
            modifiers,
            ..
        } if modifiers.is_none() => Some(*repeat),
        _ => None,
    }
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
    use crate::message::{Failure, Update};
    use crate::player::{NowPlaying, Transport, TransportState};
    use crate::queue::Queue;
    use crate::route::Page;
    use crate::search::Device;
    use crate::signin::SignIn;
    use crate::theme::{extent, fonts, icon, space, style};

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

        /// Double clicks `at` over the passes egui's hit test needs, returning
        /// the command the shell reported. A track row plays on a double click,
        /// the way every other table's rows do.
        fn double_click(
            &mut self,
            transport: &mut Transport,
            queue: &mut Queue,
            at: egui::Pos2,
        ) -> Option<Command> {
            let _ = self.frame(transport, queue, Vec::new());
            let mut command = None;
            for _ in 0..2 {
                for event in crate::view::test_support::click_events(at) {
                    if let Some(next) = self.frame(transport, queue, vec![event]).0 {
                        command = Some(next);
                    }
                }
            }
            command
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
        // Only the topbar's band: the library's density toggle draws the same
        // 28-point squares further down the window.
        let band = extent::TOPBAR_H;
        let mut rects = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if shape.stroke.width > 0.0
                        && (shape.rect.width() - extent::BUTTON).abs() < 0.01
                        && (shape.rect.height() - extent::BUTTON).abs() < 0.01
                        && shape.rect.bottom() <= band =>
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
            tracks: None,
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
            tracks: None,
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
            tracks: None,
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

    /// Queues two tracks, so the transport has something to pause.
    fn two_tracks(transport: &mut Transport) -> Queue {
        let tracks = vec![track(0, None), track(1, None)];
        let mut queue = Queue::default();
        queue.start(&tracks, 0, transport);
        queue
    }

    /// One press and its release, so the next press reads as a new one: egui
    /// marks a press with no release behind it as auto-repeat.
    fn tap(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        event: egui::Event,
    ) -> (Option<Command>, egui::FullOutput) {
        let frame = window.frame(transport, queue, vec![event]);
        let _ = window.frame(transport, queue, vec![key(egui::Key::Space, false)]);
        frame
    }

    #[test]
    fn space_pauses_and_resumes_the_transport() {
        let mut transport = Transport::default();
        let mut queue = two_tracks(&mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        for (state, expected) in [
            (TransportState::Playing, Command::Pause),
            (TransportState::Paused, Command::Resume),
            (TransportState::Loading, Command::Pause),
        ] {
            transport.state = state;
            let (command, _) = tap(
                &mut window,
                &mut transport,
                &mut queue,
                key(egui::Key::Space, true),
            );
            assert_eq!(command, Some(expected), "a space on {state:?}");
        }
    }

    #[test]
    fn space_with_nothing_queued_asks_for_nothing() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        let (command, _) = tap(
            &mut window,
            &mut transport,
            &mut queue,
            key(egui::Key::Space, true),
        );
        assert_eq!(command, None, "there is no track to start");
    }

    #[test]
    fn a_held_space_does_not_thrash_the_transport() {
        let mut transport = Transport::default();
        let mut queue = two_tracks(&mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        transport.state = TransportState::Playing;

        let (first, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Space, true)],
        );
        assert_eq!(first, Some(Command::Pause));

        // The same key pressed again with no release: what a held key looks like.
        let (held, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Space, true)],
        );
        assert_eq!(held, None, "a held space must not thrash the transport");
    }

    #[test]
    fn a_clicked_row_keeps_the_keyboard_and_lends_the_space_to_the_transport() {
        // A click leaves the keyboard where the pointer went, so Tab carries on
        // from the row that was picked. The space is still the transport's: the
        // row's own keys are Enter and a double click.
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
        let _ = window.click(&mut transport, &mut queue, at + Vec2::new(6.0, 6.0));
        assert!(
            window.ctx.memory(|memory| memory.focused().is_some()),
            "the clicked row holds the keyboard"
        );

        transport.state = TransportState::Playing;
        let (command, _) = tap(
            &mut window,
            &mut transport,
            &mut queue,
            key(egui::Key::Space, true),
        );
        assert_eq!(
            command,
            Some(Command::Pause),
            "the space is the transport's after a click on a row"
        );
    }

    #[test]
    fn a_held_space_never_reaches_a_focused_row() {
        // A row that holds focus would take egui's space click on every repeat of
        // a held key, so the shell takes the repeats away too: one pause, and
        // then nothing at all — not to the transport, and not as a row activation.
        let mut transport = Transport::default();
        let mut queue = two_tracks(&mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        // A library with something to reach: a sidebar row that Tab can focus and
        // that a space would open, so a leaked repeat would show up as a command.
        ready_playlist(&mut window);
        window.library.to_playlists();
        transport.state = TransportState::Playing;

        let _ = window.frame(&mut transport, &mut queue, vec![key(egui::Key::Tab, true)]);
        assert!(
            window.ctx.memory(|memory| memory.focused().is_some()),
            "Tab has to land somewhere for this to mean anything"
        );

        let (first, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Space, true)],
        );
        assert_eq!(first, Some(Command::Pause));
        let page = window.library.page().cloned();

        for _ in 0..3 {
            let (held, _) = window.frame(
                &mut transport,
                &mut queue,
                vec![key_repeat(egui::Key::Space)],
            );
            assert_eq!(held, None, "a repeat is nobody's press");
        }
        assert_eq!(
            window.library.page().cloned(),
            page,
            "a held space must not open what it holds focus on"
        );
    }

    #[test]
    fn a_space_with_no_loaded_track_asks_for_nothing() {
        // The queue can hold entries without one of them being the transport's,
        // and the space must not pick one: it only ever toggles what is loaded.
        let mut transport = Transport::default();
        let mut queue = two_tracks(&mut transport);
        transport.now_playing = None;
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        let (command, _) = tap(
            &mut window,
            &mut transport,
            &mut queue,
            key(egui::Key::Space, true),
        );
        assert_eq!(command, None, "there is nothing loaded to pause or start");
    }

    #[test]
    fn a_modified_space_is_somebody_elses_shortcut() {
        let mut transport = Transport::default();
        let mut queue = two_tracks(&mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        transport.state = TransportState::Playing;

        let mut event = key(egui::Key::Space, true);
        if let egui::Event::Key { modifiers, .. } = &mut event {
            *modifiers = egui::Modifiers::COMMAND;
        }
        let (command, _) = tap(&mut window, &mut transport, &mut queue, event);
        assert_eq!(command, None, "a modifier is a shortcut of somebody else's");
    }

    #[test]
    fn a_focused_row_leaves_the_space_to_the_transport() {
        // egui turns a space into a click for whatever holds focus, so the shell
        // has to take it away before anything draws: otherwise Tab onto a row and
        // a space would start that row's track while the transport paused.
        let mut transport = Transport::default();
        let mut queue = two_tracks(&mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        transport.state = TransportState::Playing;

        let _ = window.frame(&mut transport, &mut queue, vec![key(egui::Key::Tab, true)]);
        assert!(
            window.ctx.memory(|memory| memory.focused().is_some()),
            "Tab has to land somewhere for this to mean anything"
        );

        let (command, _) = tap(
            &mut window,
            &mut transport,
            &mut queue,
            key(egui::Key::Space, true),
        );
        assert_eq!(
            command,
            Some(Command::Pause),
            "the space is the transport's even on a focused row"
        );
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

    /// The Lyrics tab asks for the playing track's lyrics once, and the shell is
    /// what sends it.
    #[test]
    fn opening_the_lyrics_tab_asks_for_the_lyrics_once() {
        let mut transport = playing(cover("abc"));
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = painted_at(&frame, "LYRICS").expect("the lyrics tab is painted");
        let command = window.click(&mut transport, &mut queue, at + Vec2::new(2.0, 2.0));

        assert_eq!(
            command,
            Some(Command::LoadLyrics {
                track: "spotify:track:x".to_owned()
            }),
            "opening the tab asks for the track the transport names"
        );

        // The next frame does not ask again: the load is in flight.
        let (_, _) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(
            window.frame(&mut transport, &mut queue, Vec::new()).0,
            None,
            "one request per track"
        );
    }

    #[test]
    fn a_single_click_on_a_queue_row_does_not_play() {
        // A single click is a press; the shell applies presses with the drag
        // wiring, so nothing may move the transport here.
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
            None
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("t0"),
            "a single click must not move the transport"
        );
    }

    #[test]
    fn a_double_click_on_a_queue_row_plays_that_entry() {
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
            window.double_click(&mut transport, &mut queue, at + Vec2::new(6.0, 6.0)),
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

    /// Puts a ready search page on screen, answered with `count` labelled
    /// tracks. The page is opened through `library.open`, which is the only way
    /// in while the field cannot navigate yet.
    fn answered(window: &mut Window, query: &str, count: usize) {
        let _ = window.library.open(Page::Search(query.to_owned()));
        window.library.apply(&Update::Search {
            query: query.to_owned(),
            tracks: (0..count).map(|index| track(index, None)).collect(),
        });
    }

    /// Asks the search state for the read a frame wants, the way `logic` does.
    fn search_read(window: &mut Window) -> Option<Command> {
        let page = window.library.page_search_query().map(str::to_owned);
        window.library.search.request(page.as_deref())
    }

    /// The skeleton bars painted inside `area`.
    fn skeleton_rects(output: &egui::FullOutput, area: Rect) -> usize {
        let tokens = Tokens::DARK;
        output
            .shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape) => {
                    shape.fill == tokens.palette.skeleton_rule && area.contains_rect(shape.rect)
                }
                _ => false,
            })
            .count()
    }

    #[test]
    fn the_search_page_renders_its_heading_and_count() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        answered(&mut window, "never gonna", 20);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

        assert!(
            painted_at(&frame, "Results for “never gonna”").is_some(),
            "the heading names the query"
        );
        assert!(
            painted_at(&frame, "20 RESULTS").is_some(),
            "the head's right slot counts what the wire answered"
        );
        assert!(
            painted_at(&frame, "t0").is_some(),
            "the answer's tracks are the page's table"
        );
    }

    /// The page holds its shape while the read is out, says so when the answer
    /// is empty, and offers a Retry when the read failed — the three states the
    /// table has nothing to draw in.
    #[test]
    fn the_search_page_holds_skeleton_empty_and_failure_states() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = window.library.open(Page::Search("never gonna".to_owned()));

        // Idle: nothing true to draw yet, so the page holds rows open.
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(painted_at(&frame, "Results for “never gonna”").is_some());
        assert!(
            skeleton_rects(&frame, view_rect(&window)) > 0,
            "a pending page holds its rows open"
        );
        assert!(
            painted_at(&frame, "t0").is_none(),
            "and draws no answer it does not have"
        );

        // Failed: the failure's own words and a Retry that asks once.
        window.library.apply(&Update::SearchFailed {
            query: "never gonna".to_owned(),
            failure: Failure::Search,
        });
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            painted_at(&frame, crate::signin::failure_message(Failure::Search)).is_some(),
            "the failure is one muted line, not an error box"
        );
        let retry = painted_at(&frame, "Retry").expect("the failure offers a Retry");
        assert_eq!(
            window.click(&mut transport, &mut queue, retry + Vec2::new(4.0, 4.0)),
            Some(Command::Search {
                query: "never gonna".to_owned()
            }),
            "Retry is the same read, asked again"
        );
        assert_eq!(
            window.library.search.page_answer("never gonna"),
            Some(&Load::Loading)
        );

        // Empty: one muted line naming the query, nothing to press, and no
        // rows held open any more.
        window.library.apply(&Update::Search {
            query: "never gonna".to_owned(),
            tracks: Vec::new(),
        });
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            painted_at(&frame, "No tracks match “never gonna”.").is_some(),
            "an empty answer says so"
        );
        assert_eq!(
            skeleton_rects(&frame, view_rect(&window)),
            0,
            "an answered page holds nothing open"
        );
    }

    #[test]
    fn a_click_on_a_result_row_queues_the_whole_answer_at_that_row() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        answered(&mut window, "never gonna", 20);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = painted_at(&frame, "t3").expect("row t3 is painted");

        assert_eq!(
            window.double_click(&mut transport, &mut queue, at + Vec2::new(6.0, 6.0)),
            Some(Command::Play {
                uri: "t3".to_owned()
            }),
            "a row plays itself"
        );
        assert_eq!(
            queue.upcoming().len(),
            16,
            "the queue is the whole answer, entered at the row"
        );
        assert_eq!(
            queue.next(&mut transport),
            Some(Command::Play {
                uri: "t4".to_owned()
            }),
            "next walks the answer, not just the row that was clicked"
        );
    }

    #[test]
    fn back_and_forward_walk_the_search_pages() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        answered(&mut window, "a", 3);
        answered(&mut window, "b", 3);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(painted_at(&frame, "Results for “b”").is_some());
        let buttons = topbar_buttons(&frame);

        // Back onto a's page: its answer was dropped when b replaced it, so the
        // page asks again — and never paints b's rows under a's heading.
        assert_eq!(
            window.click(&mut transport, &mut queue, buttons[1].center()),
            None,
            "a search page never settles with a command"
        );
        assert_eq!(
            window.library.page_search_query(),
            Some("a"),
            "Back walks the history"
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(painted_at(&frame, "Results for “a”").is_some());
        assert!(
            painted_at(&frame, "t0").is_none(),
            "another query's answer must not be drawn"
        );
        assert_eq!(
            search_read(&mut window),
            Some(Command::Search {
                query: "a".to_owned()
            })
        );
        window.library.apply(&Update::Search {
            query: "a".to_owned(),
            tracks: vec![track(0, None), track(1, None), track(2, None)],
        });
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            painted_at(&frame, "t0").is_some(),
            "the re-asked answer is the page's table"
        );

        // Forward onto b's page, which is asked for the same way.
        let buttons = topbar_buttons(&frame);
        window.click(&mut transport, &mut queue, buttons[2].center());
        assert_eq!(window.library.page_search_query(), Some("b"));
        assert_eq!(
            search_read(&mut window),
            Some(Command::Search {
                query: "b".to_owned()
            })
        );
        window.library.apply(&Update::Search {
            query: "b".to_owned(),
            tracks: vec![track(0, None), track(1, None), track(2, None)],
        });
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(painted_at(&frame, "Results for “b”").is_some());
    }

    /// The scroll area is salted with the query, so a new query starts at the
    /// top rather than where the last one was left.
    #[test]
    fn a_new_query_starts_at_the_top_of_the_results() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        answered(&mut window, "a", 20);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let top = painted_at(&frame, "t5").expect("row t5 is painted").y;
        // The wheel only reaches the area the pointer is over.
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::PointerMoved(view_rect(&window).center())],
        );

        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -120.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: egui::Modifiers::NONE,
        };
        // A wheel notch is smoothed over a few passes, and the page lays out
        // with the offset it was left on, so the movement shows a pass later.
        for _ in 0..4 {
            let _ = window.frame(&mut transport, &mut queue, vec![wheel.clone()]);
        }
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let scrolled = painted_at(&frame, "t5").expect("row t5 is still painted").y;
        assert!(
            scrolled < top - 50.0,
            "the wheel moves this query's page: {scrolled} against {top}"
        );

        // The offset is stored under this query's own id: another query's page
        // is a different area, so it starts at the top.
        answered(&mut window, "b", 20);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let fresh = painted_at(&frame, "t5").expect("row t5 is painted").y;
        assert!(
            (fresh - top).abs() < 0.01,
            "a new query starts at the top: {fresh} against {top}"
        );

        // Back onto a, whose answer b dropped: answered again, the page comes
        // back to the offset it was left on, so the salt is per query and not
        // per frame.
        assert_eq!(window.library.back(), None, "a search page never settles");
        assert_eq!(window.library.page_search_query(), Some("a"));
        window.library.apply(&Update::Search {
            query: "a".to_owned(),
            tracks: (0..20).map(|index| track(index, None)).collect(),
        });
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let kept = painted_at(&frame, "t5").expect("row t5 is painted").y;
        assert!(
            (kept - scrolled).abs() < 0.01,
            "a keeps its own offset while b started at the top: {kept} against {scrolled}"
        );
    }

    // -----------------------------------------------------------------------
    // The field, the menu and the frame order (R1–R18).
    // -----------------------------------------------------------------------

    /// The field's painted box this frame.
    fn field_box(window: &mut Window, transport: &mut Transport, queue: &mut Queue) -> Rect {
        let (_, frame) = window.frame(transport, queue, Vec::new());
        search_field(&frame).expect("the topbar paints the search field")
    }

    /// The text edit's own rect, read from egui's widget memory. The id comes
    /// from the focused widget, so the field must have the keyboard: a painted
    /// box is where the view draws, but the text edit's own response rect is
    /// what the width contract and the pointer both read.
    fn edit_box(window: &Window) -> Rect {
        let id = window
            .ctx
            .memory(|memory| memory.focused())
            .expect("the field has the keyboard");
        window
            .ctx
            .read_response(id)
            .expect("the text edit was laid out")
            .rect
    }

    /// Clicks the field and types `query` into it, leaving the read unsent so a
    /// test can assert what it asks for.
    fn type_query(window: &mut Window, transport: &mut Transport, queue: &mut Queue, query: &str) {
        let at = field_box(window, transport, queue).center();
        let _ = window.click(transport, queue, at);
        let _ = window.frame(transport, queue, vec![egui::Event::Text(query.to_owned())]);
    }

    /// The menu's frame for a given number of rows, solved the way the view
    /// solves it.
    fn menu_box(window: &Window, field: Rect, height: f32) -> Rect {
        let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
        layout::search_menu::menu_rect(field, content, height)
    }

    /// Types `query`, sends and answers its read with `count` tracks, and leaves
    /// the menu drawn. Returns the menu's frame and that frame's output.
    fn answered_menu(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        query: &str,
        count: usize,
    ) -> (Rect, egui::FullOutput) {
        type_query(window, transport, queue, query);
        let key = crate::search::collapse(query).expect("a key");
        assert_eq!(
            search_read(window),
            Some(Command::Search { query: key.clone() }),
            "typing arms one read"
        );
        window.library.apply(&Update::Search {
            query: key,
            tracks: (0..count).map(|index| track(index, None)).collect(),
        });
        // The first frame an `Area` is shown is its sizing pass: egui draws
        // nothing and marks it invisible, then asks for the next frame. The
        // second frame is the menu the reader sees.
        let _ = window.frame(transport, queue, Vec::new());
        let (_, frame) = window.frame(transport, queue, Vec::new());
        let field = search_field(&frame).expect("the topbar paints the field");
        let height = layout::search_menu::rows_height(count.min(5), count > 5);
        (menu_box(window, field, height), frame)
    }

    /// One keyboard press, with a release behind it so the next press is a new
    /// one rather than an auto-repeat.
    fn press(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        code: egui::Key,
    ) -> Option<Command> {
        let command = window.frame(transport, queue, vec![key(code, true)]).0;
        let _ = window.frame(transport, queue, vec![key(code, false)]);
        command
    }

    /// Q2's executor check: zeroing the caret's stroke width removes its pixels.
    /// epaint treats a zero-width stroke as empty and paints nothing, so the
    /// marked row's rule is the only thing saying where Enter lands.
    #[test]
    fn a_marked_row_hides_the_caret() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        // The caret blinks; a paused cursor is what a single frame can assert.
        window
            .ctx
            .all_styles_mut(|style| style.visuals.text_cursor.blink = false);
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);

        let caret = |frame: &egui::FullOutput| {
            frame
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::LineSegment { stroke, .. } => Some(stroke.width),
                    _ => None,
                })
        };
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let visible = caret(&frame).expect("a focused field paints its caret");
        assert!(visible > 0.0, "an unmarked field shows the caret");

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            window.ctx.text_edit_focused(),
            "the field still has the keyboard while a row is marked"
        );
        assert_eq!(
            caret(&frame),
            Some(0.0),
            "a marked row hides the caret, so the rule is the only target"
        );
    }

    #[test]
    fn typing_focuses_the_field_and_arms_one_read() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        type_query(&mut window, &mut transport, &mut queue, "a");
        assert!(
            window.ctx.text_edit_focused(),
            "typing reaches the field only when the field has the keyboard"
        );
        assert_eq!(window.library.search.text, "a");
        assert_eq!(
            search_read(&mut window),
            Some(Command::Search {
                query: "a".to_owned()
            }),
            "the first character is a read (R6)"
        );

        // Focus alone is not a query: the click opens nothing and asks nothing.
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let at = field_box(&mut window, &mut transport, &mut queue).center();
        let _ = window.click(&mut transport, &mut queue, at);
        assert!(window.ctx.text_edit_focused());
        assert!(
            !window.library.search.is_open(),
            "focus alone opens nothing"
        );
        assert_eq!(search_read(&mut window), None);
    }

    #[test]
    fn an_answer_draws_five_rows_with_no_mark() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (menu, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);

        assert_eq!(
            window.library.search.mark(),
            None,
            "the menu opens with its first row unmarked (R1)"
        );
        for index in 0..5 {
            assert!(
                text_in(&frame, &format!("t{index}"), menu).is_some(),
                "row {index} is drawn in the menu"
            );
        }
        assert!(
            text_in(&frame, "t5", menu).is_none(),
            "the menu shows five of the answer"
        );
    }

    #[test]
    fn down_and_enter_play_the_marked_row_and_queue_the_whole_answer() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);

        // A non-zero row, so a zeroed index cannot pass for the marked one.
        for _ in 0..2 {
            let _ = window.frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::ArrowDown, true)],
            );
        }
        assert_eq!(window.library.search.mark(), Some(1));

        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
            Some(Command::Play {
                uri: "t1".to_owned()
            })
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("t1")
        );
        assert_eq!(
            queue.upcoming().len(),
            6,
            "the queue is the whole answer, entered at the hit (R16)"
        );
        assert_eq!(
            queue.next(&mut transport),
            Some(Command::Play {
                uri: "t2".to_owned()
            }),
            "next walks the answer from the marked row"
        );
        assert!(
            !window.library.search.is_open(),
            "an applied action closes it"
        );
    }

    #[test]
    fn up_from_the_field_marks_the_last_drawn_row() {
        for (count, expected) in [(5, 4), (6, 5)] {
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let mut window = Window::new(Vec2::new(1400.0, 900.0));
            let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", count);
            let _ = window.frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::ArrowUp, true)],
            );
            assert_eq!(
                window.library.search.mark(),
                Some(expected),
                "with {count} results the last row is {expected}"
            );
        }
    }

    #[test]
    fn typing_over_a_marked_row_returns_to_the_field() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
        assert_eq!(window.library.search.mark(), Some(0));

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::Text("b".to_owned())],
        );
        assert_eq!(window.library.search.text, "ab");
        assert_eq!(window.library.search.mark(), None, "R2 puts the rule back");
        assert!(window.library.search.is_open());
    }

    #[test]
    fn escape_is_two_steps_and_the_second_one_is_the_searchs() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
        assert_eq!(window.library.search.mark(), Some(0));

        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Escape),
            None
        );
        assert_eq!(window.library.search.text, "a", "the query is kept (R3)");
        assert!(window.library.search.is_open(), "and the menu stays up");
        assert_eq!(window.library.search.mark(), None);

        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Escape),
            None
        );
        assert_eq!(window.library.search.text, "");
        assert!(!window.library.search.is_open());
    }

    #[test]
    fn escape_belongs_to_the_drawer_only_when_search_has_nothing_to_undo() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(800.0, 900.0));

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let toggle = topbar_buttons(&frame)[0];
        let _ = window.click(&mut transport, &mut queue, toggle.center());
        assert_eq!(window.drawer, Drawer::Open);
        assert!(
            !window.ctx.text_edit_focused(),
            "the toggle, not the field, has the keyboard"
        );

        // A held key is a repeat in egui, so every press below is released: the
        // field ignores a repeat's Escape, the way it ignores a held Enter.
        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Escape),
            None
        );
        assert_eq!(
            window.drawer,
            Drawer::Closed,
            "an empty, closed search leaves Escape to the drawer"
        );

        // The other way round: an open drawer and an active search at once,
        // reached by the keyboard so no click closes the drawer on the way.
        let _ = window.click(&mut transport, &mut queue, toggle.center());
        assert_eq!(window.drawer, Drawer::Open);
        for _ in 0..24 {
            if window.ctx.text_edit_focused() {
                break;
            }
            let _ = window.frame(&mut transport, &mut queue, vec![key(egui::Key::Tab, true)]);
        }
        assert!(
            window.ctx.text_edit_focused(),
            "Tab reaches the search field"
        );
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::Text("a".to_owned())],
        );
        assert!(window.library.search.is_open());

        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Escape),
            None
        );
        assert_eq!(window.library.search.text, "");
        assert!(!window.library.search.is_open());
        assert_eq!(
            window.drawer,
            Drawer::Open,
            "search consumed the Escape it acted on"
        );

        // With nothing left to undo, the next Escape is the drawer's again.
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);
        assert_eq!(window.drawer, Drawer::Closed);
    }

    #[test]
    fn a_space_typed_in_the_field_is_not_the_transports() {
        let mut transport = Transport::default();
        let mut queue = two_tracks(&mut transport);
        transport.state = TransportState::Playing;
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        let at = field_box(&mut window, &mut transport, &mut queue).center();
        let _ = window.click(&mut transport, &mut queue, at);
        assert!(window.ctx.text_edit_focused());

        let (command, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![
                key(egui::Key::Space, true),
                egui::Event::Text(" ".to_owned()),
            ],
        );
        assert_eq!(command, None, "a space in the field is a character");
        assert_eq!(
            transport.state,
            TransportState::Playing,
            "and it does not pause the transport"
        );
        assert_eq!(window.library.search.text, " ");
    }

    #[test]
    fn the_floor_appears_only_when_the_answer_is_longer_than_the_menu() {
        for (count, floor) in [(5, false), (6, true)] {
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let mut window = Window::new(Vec2::new(1400.0, 900.0));
            let (menu, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", count);
            let label = format!("SEE ALL {count} RESULTS FOR “A”");
            assert_eq!(
                text_in(&frame, &label, menu).is_some(),
                floor,
                "an answer of {count} draws the floor: {floor}"
            );
            assert_eq!(
                menu.height(),
                layout::search_menu::rows_height(5.min(count), floor),
                "and the frame is exactly its rows"
            );
        }
    }

    /// Press a row, then release it: the menu must survive the frame in which
    /// the press surrenders the field's focus (Q4). Both outcomes are staged:
    /// the natural one, and the opposite one forced between press and release.
    #[test]
    fn a_row_click_plays_under_both_focus_outcomes() {
        for keep_focus in [false, true] {
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let mut window = Window::new(Vec2::new(1400.0, 900.0));
            let (menu, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
            let at = text_in(&frame, "t1", menu)
                .expect("row t1 is drawn")
                .center();
            let field = window.ctx.memory(|memory| memory.focused());

            let _ = window.frame(
                &mut transport,
                &mut queue,
                vec![egui::Event::PointerMoved(at)],
            );
            let _ = window.frame(
                &mut transport,
                &mut queue,
                vec![crate::view::test_support::button(at, true)],
            );

            match (keep_focus, field) {
                (true, Some(id)) => window.ctx.memory_mut(|memory| memory.request_focus(id)),
                (false, _) => {
                    if let Some(id) = window.ctx.memory(|memory| memory.focused()) {
                        window.ctx.memory_mut(|memory| memory.surrender_focus(id));
                    }
                }
                (true, None) => panic!("the field had the keyboard before the press"),
            }
            assert!(window.library.search.is_open(), "the press keeps it open");

            let (command, _) = window.frame(
                &mut transport,
                &mut queue,
                vec![crate::view::test_support::button(at, false)],
            );
            assert_eq!(
                command,
                Some(Command::Play {
                    uri: "t1".to_owned()
                }),
                "a click plays the row with keep_focus = {keep_focus}"
            );
            assert_eq!(
                queue.upcoming().len(),
                6,
                "the queue is the whole answer, entered at the clicked row ({keep_focus})"
            );
            assert_eq!(
                queue.next(&mut transport),
                Some(Command::Play {
                    uri: "t2".to_owned()
                }),
                "next walks the answer from the clicked row ({keep_focus})"
            );
        }
    }

    #[test]
    fn a_press_on_one_row_and_a_release_on_another_play_nothing() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (menu, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let one = text_in(&frame, "t1", menu).expect("row t1").center();
        let other = text_in(&frame, "t3", menu).expect("row t3").center();

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::PointerMoved(one)],
        );
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![crate::view::test_support::button(one, true)],
        );
        let (command, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![crate::view::test_support::button(other, false)],
        );
        assert_eq!(command, None, "two rows are not one click");
        assert!(transport.now_playing.is_none());
        assert!(
            window.library.search.is_open(),
            "the release is still engaged"
        );

        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            !window.library.search.is_open(),
            "the next idle frame closes it"
        );
    }

    #[test]
    fn a_click_outside_both_closes_the_menu() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        assert!(window.library.search.is_open());

        let _ = window.click(&mut transport, &mut queue, view_rect(&window).center());
        assert!(!window.library.search.is_open());
    }

    #[test]
    fn tabbing_out_of_the_field_closes_the_menu() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        assert!(window.ctx.text_edit_focused());

        let _ = window.frame(&mut transport, &mut queue, vec![key(egui::Key::Tab, true)]);
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(!window.ctx.text_edit_focused(), "Tab leaves the text edit");
        assert!(!window.library.search.is_open());
    }

    #[test]
    fn enter_on_the_field_opens_the_results_page() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);

        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
            None,
            "a search page never settles with a command"
        );
        assert_eq!(window.library.page(), Some(&Page::Search("a".to_owned())));
        assert!(!window.library.search.is_open());
        assert_eq!(
            window.library.search.page_answer("a"),
            Some(&Load::Ready(
                (0..8).map(|index| track(index, None)).collect()
            )),
            "a ready menu seeds the page without a second read"
        );
        assert_eq!(search_read(&mut window), None);
    }

    #[test]
    fn enter_after_a_failed_menu_asks_exactly_once() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        type_query(&mut window, &mut transport, &mut queue, "a");
        assert_eq!(
            search_read(&mut window),
            Some(Command::Search {
                query: "a".to_owned()
            })
        );
        window.library.apply(&Update::SearchFailed {
            query: "a".to_owned(),
            failure: Failure::Search,
        });
        let _ = window.frame(&mut transport, &mut queue, Vec::new());

        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
        assert_eq!(window.library.page(), Some(&Page::Search("a".to_owned())));
        assert_eq!(
            search_read(&mut window),
            Some(Command::Search {
                query: "a".to_owned()
            }),
            "Enter on a failed menu is the retry"
        );
        assert_eq!(search_read(&mut window), None, "asked exactly once");
    }

    #[test]
    fn a_dropped_action_leaves_the_menu_open_and_the_queue_untouched() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        stale_playlist_behind_an_album(&mut window.library);
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
        assert_eq!(window.library.search.mark(), Some(0));

        // Enter plays the marked row, but a browser Back in the same frame
        // takes the frame's one command: the action must be dropped whole.
        let (command, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![
                key(egui::Key::Enter, true),
                key(egui::Key::BrowserBack, true),
            ],
        );
        assert_eq!(
            command,
            Some(Command::LoadTracks {
                playlist: PLAYLIST.to_owned()
            }),
            "the nav kept the frame"
        );
        assert!(transport.now_playing.is_none(), "the queue was not touched");
        assert_eq!(queue.upcoming().len(), 0);
        assert!(window.library.search.is_open(), "the menu stays open");
    }

    #[test]
    fn a_held_enter_plays_once() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );

        let (first, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Enter, true)],
        );
        assert_eq!(
            first,
            Some(Command::Play {
                uri: "t0".to_owned()
            })
        );
        let (held, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![key_repeat(egui::Key::Enter)],
        );
        assert_eq!(held, None, "an auto-repeat is nobody's press");
        // If the repeat were read as a press it would be the field's Enter, which
        // opens the page: the two assertions below are what the inversion breaks.
        assert_eq!(
            window.library.page(),
            None,
            "a held Enter plays once and opens no page"
        );
        assert!(
            window.ctx.text_edit_focused(),
            "and it must not blur the field it was pressed in"
        );
    }

    #[test]
    fn the_fields_width_never_moves() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        // The text edit's own rect, not the painted box: the box is drawn at a
        // width nothing can change, so it could never fail this.
        let at = field_box(&mut window, &mut transport, &mut queue).center();
        let _ = window.click(&mut transport, &mut queue, at);
        let empty = edit_box(&window);
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::Text("a".to_owned())],
        );
        let typed = edit_box(&window);

        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);
        let cleared = edit_box(&window);

        assert_eq!(empty, typed, "typing does not resize the field (R4)");
        assert_eq!(empty, cleared, "clearing does not resize it either (R5)");
    }

    #[test]
    fn home_and_back_leave_no_stale_open_menu() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        ready_playlist(&mut window);
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        assert!(window.library.search.is_open());

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let buttons = topbar_buttons(&frame);
        let _ = window.click(&mut transport, &mut queue, buttons[0].center());
        assert!(!window.library.search.is_open(), "Home leaves the field");

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let buttons = topbar_buttons(&frame);
        let _ = window.click(&mut transport, &mut queue, buttons[1].center());
        assert!(
            !window.library.search.is_open(),
            "and Back does not reopen it"
        );
        assert_eq!(window.library.search.text, "a");
    }

    #[test]
    fn the_clear_control_empties_the_query_without_moving_the_field() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let field = search_field(&frame).expect("the field is painted");
        // Inside the padded hit rect, which spans the field's height at its right
        // padding (R5).
        let at = egui::Pos2::new(field.right() - 16.0, field.center().y);
        // The text edit's own rect, which the clear control must not move.
        let before = edit_box(&window);

        let _ = window.click(&mut transport, &mut queue, at);
        assert_eq!(window.library.search.text, "");
        assert!(
            !window.library.search.is_open(),
            "clearing closes the menu the query kept open"
        );
        assert!(
            window.ctx.text_edit_focused(),
            "the keyboard goes back to the field"
        );
        assert_eq!(
            edit_box(&window),
            before,
            "clearing never changes the field's width (R5)"
        );
    }

    #[test]
    fn the_menu_says_no_matches_and_a_failure_in_one_muted_line() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        type_query(&mut window, &mut transport, &mut queue, "a");
        assert_eq!(
            search_read(&mut window),
            Some(Command::Search {
                query: "a".to_owned()
            })
        );
        window.library.apply(&Update::Search {
            query: "a".to_owned(),
            tracks: Vec::new(),
        });
        // The first frame an `Area` is shown sizes it; the second draws it.
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let field = search_field(&frame).expect("the field is painted");
        let menu = menu_box(&window, field, layout::search_menu::line_height());
        assert!(
            text_in(&frame, "No matches for “a”", menu).is_some(),
            "an empty answer is one line naming the query (R13)"
        );

        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        type_query(&mut window, &mut transport, &mut queue, "a");
        let _ = search_read(&mut window);
        window.library.apply(&Update::SearchFailed {
            query: "a".to_owned(),
            failure: Failure::Search,
        });
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let field = search_field(&frame).expect("the field is painted");
        let menu = menu_box(&window, field, layout::search_menu::line_height());
        assert!(
            text_in(
                &frame,
                crate::signin::failure_message(Failure::Search),
                menu
            )
            .is_some(),
            "a failed read is one line with its own words (R13)"
        );
    }

    #[test]
    fn enter_on_the_loaded_row_pauses_instead_of_starting_it_again() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);

        // Play row 1, then reopen the menu on the same query and mark it again.
        for _ in 0..2 {
            let _ = window.frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::ArrowDown, true)],
            );
        }
        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
            Some(Command::Play {
                uri: "t1".to_owned()
            })
        );
        let queued = queue.upcoming().len();

        // The arrows re-open the closed menu and mark the rows it draws.
        for _ in 0..2 {
            let _ = window.frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::ArrowDown, true)],
            );
        }
        assert!(window.library.search.is_open(), "an arrow re-opens it (R1)");
        assert_eq!(window.library.search.mark(), Some(1));
        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
            Some(Command::Pause),
            "the loaded row's action is pause, not a second start (R10)"
        );
        assert_eq!(queue.upcoming().len(), queued, "pausing keeps the queue");
        assert_eq!(window.library.search.text, "a", "and keeps the query");
    }

    #[test]
    fn an_arrow_reopens_a_closed_menu_on_the_row_it_would_have_marked() {
        // Down: the first row.
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
        assert!(!window.library.search.is_open());
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
        assert!(window.library.search.is_open());
        assert_eq!(window.library.search.mark(), Some(0));

        // Up: the last drawn row, which is the floor when it is drawn.
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
        assert!(!window.library.search.is_open());
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowUp, true)],
        );
        assert!(window.library.search.is_open());
        assert_eq!(window.library.search.mark(), Some(5), "the floor is last");
    }

    /// R2, for the whole painted box: the text edit is the field, so a click on
    /// the magnifier or the padding beside it focuses the field and re-opens the
    /// menu the query still holds. Before the text edit covered the box, those
    /// clicks were "elsewhere": the field blurred, `settle` closed the menu, and
    /// `clicked_field` never fired.
    #[test]
    fn a_click_on_the_magnifier_or_the_left_padding_is_a_click_on_the_field() {
        let pad = space::S3 + icon::Size::MD.side / 2.0;
        let cases = [("the magnifier", pad), ("the left padding", space::S1)];
        for (name, dx) in cases {
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let mut window = Window::new(Vec2::new(1400.0, 900.0));
            let (_, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
            let field = search_field(&frame).expect("the field is painted");

            // Enter opens the page: the menu closes, the query stays, and the
            // keyboard leaves the field with the menu.
            let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
            assert_eq!(window.library.page(), Some(&Page::Search("a".to_owned())));
            assert!(!window.library.search.is_open());
            assert!(!window.ctx.text_edit_focused());

            let at = egui::Pos2::new(field.left() + dx, field.center().y);
            let _ = window.click(&mut transport, &mut queue, at);
            assert!(
                window.ctx.text_edit_focused(),
                "a click on {name} focuses the field"
            );
            assert!(
                window.library.search.is_open(),
                "and a click on {name} re-opens the menu the query holds (R2)"
            );

            // The reason those points are the field's: the text edit is the
            // painted box, not a narrower widget inside it.
            let edit = edit_box(&window);
            assert_eq!(
                edit.left(),
                field.left(),
                "the text edit starts at the edge"
            );
            assert_eq!(edit.right(), field.right(), "and ends at the other one");
        }
    }

    /// R9: the pointer's tick is a hint on a row that is not the mark. Moving
    /// the pointer makes it the last device; it never takes the keyboard's rule
    /// with it.
    #[test]
    fn the_pointer_tick_skips_the_marked_row() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (menu, _frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let tokens = Tokens::DARK;
        let ticks = |frame: &egui::FullOutput| {
            frame
                .shapes
                .iter()
                .filter(|clipped| match &clipped.shape {
                    egui::Shape::Rect(shape) => {
                        shape.fill == tokens.palette.text_muted
                            && (shape.rect.width() - stroke::RULE).abs() < 0.01
                            && (shape.rect.height() - space::S4).abs() < 0.01
                            && menu.contains_rect(shape.rect)
                    }
                    _ => false,
                })
                .count()
        };

        // The pointer arrives over row 3: it owns the highlight, so row 3 takes
        // the 4% wash and the tick.
        let third = layout::search_menu::row_rect(menu, 3).center();
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::PointerMoved(third)],
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(window.library.search.device(), Device::Pointer);
        assert_eq!(ticks(&frame), 1, "the pointed-at row takes the tick");

        // The keyboard marks the row the pointer is on: the mark's rule is the
        // answer now, so the pointer stops drawing its tick there.
        for _ in 0..4 {
            let _ = window.frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::ArrowDown, true)],
            );
        }
        assert_eq!(window.library.search.mark(), Some(3));
        assert_eq!(window.library.search.device(), Device::Keyboard);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(
            ticks(&frame),
            0,
            "the marked row keeps the rule and takes no tick"
        );

        // The pointer moves on: the device switches back and the new row takes
        // the tick beside the mark, which does not move.
        let second = layout::search_menu::row_rect(menu, 2).center();
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::PointerMoved(second)],
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(window.library.search.device(), Device::Pointer);
        assert_eq!(
            window.library.search.mark(),
            Some(3),
            "the pointer moves no mark"
        );
        assert_eq!(
            ticks(&frame),
            1,
            "a row that is not the mark takes the tick"
        );
    }

    /// R12: the floor row is an action, and a click on it opens the page — the
    /// same page Enter on the floor opens.
    #[test]
    fn a_click_on_the_floor_opens_the_results_page() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (menu, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let floor = layout::search_menu::row_rect(menu, 5);
        assert!(
            text_in(&frame, "SEE ALL 8 RESULTS FOR “A”", menu).is_some(),
            "the answer is long enough to draw the floor"
        );

        assert_eq!(
            window.click(&mut transport, &mut queue, floor.center()),
            None,
            "a search page never settles with a command"
        );
        assert_eq!(window.library.page(), Some(&Page::Search("a".to_owned())));
        assert!(
            !window.library.search.is_open(),
            "the floor closes the menu"
        );
    }

    /// Finding 6: a nav that takes the frame's command drops `OpenResults`, and
    /// a dropped action leaves the menu open *and* the keyboard in the field.
    #[test]
    fn a_dropped_open_results_leaves_the_field_focused() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        stale_playlist_behind_an_album(&mut window.library);
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        // Enter with the field marked, not on a row: it asks for the page.
        assert!(window.ctx.text_edit_focused());

        let (command, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![
                key(egui::Key::Enter, true),
                key(egui::Key::BrowserBack, true),
            ],
        );
        assert_eq!(
            command,
            Some(Command::LoadTracks {
                playlist: PLAYLIST.to_owned()
            }),
            "the nav kept the frame"
        );
        assert_ne!(
            window.library.page(),
            Some(&Page::Search("a".to_owned())),
            "the page was not opened"
        );
        assert!(window.library.search.is_open(), "the menu stays open");
        assert!(
            window.ctx.text_edit_focused(),
            "and a dropped OpenResults does not surrender the field"
        );
    }

    /// R10 with the shared hold decision: the loaded row's action follows the
    /// transport's state, exactly as its glyph does. A paused track resumes; an
    /// ended one is restarted, because pausing it would do nothing.
    #[test]
    fn enter_resumes_a_paused_row_and_restarts_a_stopped_one() {
        for (state, expected) in [
            (TransportState::Paused, Command::Resume),
            (
                TransportState::Stopped,
                Command::Play {
                    uri: "t1".to_owned(),
                },
            ),
        ] {
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let mut window = Window::new(Vec2::new(1400.0, 900.0));
            let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);

            // Load row 1, then put the transport in the state under test and
            // mark the loaded row again from the closed menu.
            for _ in 0..2 {
                let _ = window.frame(
                    &mut transport,
                    &mut queue,
                    vec![key(egui::Key::ArrowDown, true)],
                );
            }
            let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
            assert_eq!(
                transport.now_playing.as_ref().map(|now| now.uri.as_str()),
                Some("t1")
            );
            transport.state = state;
            for _ in 0..2 {
                let _ = window.frame(
                    &mut transport,
                    &mut queue,
                    vec![key(egui::Key::ArrowDown, true)],
                );
            }
            assert_eq!(window.library.search.mark(), Some(1));

            assert_eq!(
                press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
                Some(expected.clone()),
                "{state:?}"
            );
            if state == TransportState::Stopped {
                assert_eq!(
                    queue.upcoming().len(),
                    6,
                    "a restart enters the whole answer at the row"
                );
                assert_eq!(
                    queue.next(&mut transport),
                    Some(Command::Play {
                        uri: "t2".to_owned()
                    }),
                    "and next walks the answer from there"
                );
            }
        }
    }
}
