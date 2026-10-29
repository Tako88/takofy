//! The eframe application: the window's state and its draw pass.

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local, TimeZone, Timelike};
use eframe::egui;
use takofy_model::{Order, RequestId};
use takofy_ui::cover::Art;
use takofy_ui::device_list::DeviceList;
use takofy_ui::devices::{Devices, Source};
use takofy_ui::drawer::Drawer;
use takofy_ui::layout::PanePref;
use takofy_ui::library::{Library, Load};
use takofy_ui::menu::Menu;
use takofy_ui::message::{Command, RemoteUpdate, Update};
use takofy_ui::player::{REDRAW, Transport, TransportState};
use takofy_ui::prefs::Prefs;
use takofy_ui::queue::Queue;
use takofy_ui::remote::react;
use takofy_ui::report::Reporter;
use takofy_ui::signin::SignIn;
use takofy_ui::theme::{Tokens, fonts, scroll, style};
use takofy_ui::{layout, view};

use crate::bridge::UiEnd;
use crate::preferences::{Loaded, PrefsSync};

/// The shortest delay a wake may ask for. A zero-delay repaint would spin the
/// frame loop, so a clock that is already due waits this long instead.
const WAKE_FLOOR: Duration = Duration::from_millis(1);

/// The application id. Reverse-DNS and permanent: a Wayland compositor tracks
/// the window by it, the desktop entry is named after it, the icons are filed
/// under it, and a store asks for one that never changes.
const APP_ID: &str = "io.github.Tako88.Takofy";

/// Window options. `run_and_return` must stay true: shutdown runs after the
/// window closes, and eframe must return instead of exiting the process.
pub(crate) fn native_options() -> eframe::NativeOptions {
    let mut viewport = egui::ViewportBuilder::default()
        .with_app_id(APP_ID)
        .with_inner_size(layout::WINDOW_DEFAULT)
        .with_min_inner_size(layout::WINDOW_MIN);
    // A missing or undecodable icon is cosmetic: the window opens without one
    // rather than refusing to open.
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    eframe::NativeOptions {
        viewport,
        run_and_return: true,
        // Every glyph is bundled, so a system fallback would hide a gap on this
        // machine and ship boxes on any other.
        system_font_fallback: false,
        ..Default::default()
    }
}

/// The window icon: the same 512 tile the packages install, decoded once at
/// startup so the task bar and the window list draw the app's own mark.
fn window_icon() -> Option<egui::IconData> {
    let png =
        include_bytes!("../../../packaging/icons/hicolor/512x512/apps/io.github.Tako88.Takofy.png");
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?;
    let image = image.to_rgba8();
    let (width, height) = image.dimensions();
    Some(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

/// The window and its end of the bridge.
pub(crate) struct TakofyApp {
    state: SignIn,
    library: Library,
    transport: Transport,
    queue: Queue,
    /// The newest state sent to the account, so a frame that changed nothing
    /// sends nothing.
    reporter: Reporter,
    /// The account's devices, the holder's transport and the watch for them.
    devices: Devices,
    /// Whether the device card is open, and above which button.
    list: DeviceList,
    drawer: Drawer,
    /// What the user asked of the Now Playing column. Nothing persists it, so it
    /// starts at the width rule every launch.
    pane: PanePref,
    menu: Menu,
    ui_end: UiEnd,
    tokens: Tokens,
    /// The decoded covers, the uploads and the fetches in flight.
    art: Art,
    /// Restamped whenever the transport's anchor moves, so the interpolated
    /// position cannot drift from it.
    since: Option<Instant>,
    /// The values that survive a restart, and the thread that saves them.
    prefs: PrefsSync,
}

impl TakofyApp {
    /// Sends the search read this frame wants, if one is due.
    ///
    /// The page's query is copied out first, because the library names it and
    /// the search state is then asked mutably. Split from `logic` so a test can
    /// ask without an eframe frame.
    fn request_search(&mut self) {
        let page = self.library.page_search_query().map(str::to_owned);
        if let Some(command) = self.library.search.request(page.as_deref()) {
            let _ = self.ui_end.commands.send(command);
        }
        // The portraits are a second read, so a reply that landed this frame is
        // asked for in this same pass.
        if let Some(command) = self.library.search.request_portraits(page.as_deref()) {
            let _ = self.ui_end.commands.send(command);
        }
    }

    /// Asks for the frame that sends the read the draw just armed.
    ///
    /// `logic` runs before `ui`, so a read armed during the draw — Back or
    /// Forward onto a page that has to ask again — would otherwise wait for the
    /// next input event. The page's query is copied out for the same reason
    /// [`Self::request_search`] copies it.
    fn wake_search(&mut self, ctx: &egui::Context) {
        let page = self.library.page_search_query().map(str::to_owned);
        if self.library.search.wants(page.as_deref()) {
            ctx.request_repaint();
        }
    }

    /// Publishes what this window is playing when the frame changed it.
    fn send_report(&mut self) {
        let position_ms = self.transport.position_ms;
        if let Some(report) = self
            .reporter
            .next(&self.queue, &self.transport, position_ms)
        {
            let _ = self.ui_end.commands.send(Command::Report(report));
        }
    }

    /// Installs the theme and takes ownership of the window's end.
    pub(crate) fn new(
        cc: &eframe::CreationContext<'_>,
        ui_end: UiEnd,
        loaded: Loaded,
        prefs: PrefsSync,
    ) -> Self {
        let tokens = Tokens::DARK;
        cc.egui_ctx.set_fonts(fonts::definitions());
        cc.egui_ctx.set_global_style(style(&tokens));
        // The wheel's own conversion, so a page passed to egui still travels the
        // same distance per line as one this client applies itself.
        cc.egui_ctx.options_mut(|options| {
            options.input_options.line_scroll_speed = scroll::POINTS_PER_LINE;
        });
        let (transport, queue) = Self::restored_state(&loaded);
        Self {
            state: SignIn::default(),
            library: Library::default(),
            transport,
            queue,
            reporter: Reporter::default(),
            devices: Devices::default(),
            list: DeviceList::default(),
            drawer: Drawer::default(),
            pane: PanePref::default(),
            menu: Menu::default(),
            ui_end,
            tokens,
            art: Art::new(),
            since: None,
            prefs,
        }
    }

    /// The transport and queue the file names.
    ///
    /// Only a file that parsed seeds them. Anything else starts on the defaults
    /// and owes the player nothing: the player is already at full, so a command
    /// there would be a level nobody asked for.
    fn restored_state(loaded: &Loaded) -> (Transport, Queue) {
        let mut queue = Queue::seeded(wall_clock_ms());
        let transport = match loaded {
            Loaded::Found(saved) => {
                saved.seed_queue(&mut queue);
                Transport {
                    volume: saved.volume(),
                    ..Transport::default()
                }
            }
            Loaded::Absent | Loaded::Refused(_) | Loaded::Unreadable => Transport::default(),
        };
        (transport, queue)
    }
}

impl eframe::App for TakofyApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let before = self.transport.anchor();
        let mut clock = Playclock::at(now).counting_from(self.since);
        let commands = drain(
            &self.ui_end.updates,
            &mut self.state,
            &mut self.library,
            &mut self.transport,
            &mut self.queue,
            &mut self.devices,
            &mut self.art,
            &mut clock,
        );
        self.since = clock.since;
        for command in commands {
            let _ = self.ui_end.commands.send(command);
        }
        // A lost link reply leaves the held rootlist revision naming a list that
        // may already carry the new playlist, so the read that names both again
        // is asked for once the failed write is out of the way.
        if let Some(command) = self.library.rootlist_request() {
            let _ = self.ui_end.commands.send(command);
        }
        // The followed list is want-driven: it is asked for after the drain, so
        // a playlists reply that just landed is what arms it. The worker never
        // chains it onto the rootlist.
        if let Some(command) = self.library.followed_request() {
            let _ = self.ui_end.commands.send(command);
        }
        // The two other followed lists are want-driven the same way: each asks
        // only once its own chip is the one the user is looking at.
        if let Some(command) = self.library.followed_artists_request() {
            let _ = self.ui_end.commands.send(command);
        }
        if let Some(command) = self.library.saved_albums_request() {
            let _ = self.ui_end.commands.send(command);
        }
        // The liked set is read once the playlists can name a signed-in window;
        // a failed write arms one more read through the same path.
        // A page's own album or show needs its saved state whatever the sidebar
        // is listing, so this is asked for by the page, not by a chip.
        if let Some(command) = self.library.saved_request() {
            let _ = self.ui_end.commands.send(command);
        }
        if let Some(command) = self.library.likes_request() {
            let _ = self.ui_end.commands.send(command);
        }
        // The artist page's likeness row is its second read: it is asked for
        // once the artist's own detail has landed and named somebody.
        if let Some(command) = self.library.related_request() {
            let _ = self.ui_end.commands.send(command);
        }
        // A row edit can leave several playlists' covers wrong at once, and the
        // loop may not run again this frame, so every due read goes out now.
        while let Some(command) = self.library.tracks_request() {
            let _ = self.ui_end.commands.send(command);
        }
        // A play pressed on the shelf asks for its object's rows here, because
        // the view never fetches and the reply is what the next frame applies.
        if let Some(command) = self.library.play_request() {
            let _ = self.ui_end.commands.send(command);
        }
        // The search reads are want-driven like the lists above: the menu while
        // the field is open, the page while it is on screen.
        self.request_search();
        // The device watch is want-driven too: the playlists landing is what
        // says the account can be asked, and the button stays off until an
        // answer arrives. A card that opened asked for one refresh, and a track
        // the holder plays is named once per URI.
        if let Some(command) = self
            .devices
            .watch_request(matches!(self.library.playlists, Load::Ready(_)))
        {
            let _ = self.ui_end.commands.send(command);
        }
        if let Some(command) = self.devices.refresh_request() {
            let _ = self.ui_end.commands.send(command);
        }
        if let Some(command) = self.devices.label_request() {
            let _ = self.ui_end.commands.send(command);
        }
        // A pick a frame could not send waits here, so a press that landed
        // beside the frame's one command is delayed rather than dropped.
        if let Some(command) = self.devices.pick_request() {
            let _ = self.ui_end.commands.send(command);
        }
        // A hand-over adopts rows the phone named only by URI, so the queue asks
        // for their names once the landing is in.
        if let Some(command) = self.queue.label_wants() {
            let _ = self.ui_end.commands.send(command);
        }
        self.devices.expire(now);
        // After the drain and before the draw, so a cover that just arrived is
        // ready in this same frame's `ui()`.
        self.art.upload(ctx, self.tokens.palette.tint.alpha_floor);
        if self.transport.anchor() != before {
            self.since = Some(now);
        }
        if self.transport.state == TransportState::Playing
            || (self.devices.source(&self.transport) == Source::Remote
                && self.devices.display().state == TransportState::Playing)
        {
            ctx.request_repaint_after(REDRAW);
        }
        // A watch waiting for its answer and a notice that is still on screen
        // are the module's own clocks: an idle window wakes for neither early.
        if let Some(delay) = self.devices.next_wake(now) {
            ctx.request_repaint_after(delay.max(WAKE_FLOOR));
        }
        // Last, so an order or a landing drained this frame is published in the
        // same frame it moved the queue.
        self.send_report();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        window_clear_color(&self.tokens)
    }

    // Glow's exit hook carries the GL context; nothing here needs it.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.prefs
            .close(&Prefs::of(&self.transport.volume, &self.queue));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let position_ms = self
            .transport
            .shown_position(self.since.map_or(Duration::ZERO, |since| since.elapsed()));
        let before = self.transport.anchor();
        // Before the draw, so `look` sees a fresh frame's wanted set.
        self.art.begin_frame();
        // The draw's borrow of the library and the art store ends with this
        // block, so the label wants it recorded can be drained below.
        let command = {
            let mut screen = view::Screen {
                state: &mut self.state,
                library: &mut self.library,
                transport: &mut self.transport,
                queue: &mut self.queue,
                devices: &mut self.devices,
                list: &mut self.list,
                drawer: &mut self.drawer,
                pane: &mut self.pane,
                menu: &mut self.menu,
                position_ms,
                art: &mut self.art,
                now,
                now_ms: wall_clock_ms(),
                local_hour: local_hour(),
            };
            view::show(ui, &mut screen, &self.tokens)
        };
        if let Some(command) = command {
            let _ = self.ui_end.commands.send(command);
        }
        // The volume is not the frame's one command: a frame that already asked
        // for something else would drop the level, and the bar would then show a
        // level the player never got.
        if let Some(command) = self.transport.take_volume() {
            let _ = self.ui_end.commands.send(command);
        }
        // After the drain, so the value the frame drew is the one that settles.
        // The wake keeps a value written while the window is otherwise idle.
        if let Some(wake) = self
            .prefs
            .observe(now, &Prefs::of(&self.transport.volume, &self.queue))
        {
            ui.ctx().request_repaint_after(wake.max(WAKE_FLOOR));
        }
        // After the draw, because only the draw arms the read: a navigation
        // that leaves the page idle asks for the frame that sends it.
        self.wake_search(ui.ctx());
        // After the draw, because only the draw knows what was wanted this frame.
        // A label want is not a command returned by the view: the shell may keep
        // only one of those, so the wants are drained from their own store.
        for command in self.library.releases.requests() {
            let _ = self.ui_end.commands.send(command);
        }
        for command in self.art.requests(now) {
            let _ = self.ui_end.commands.send(command);
        }
        schedule_wake(ui.ctx(), &self.art, now);
        // A control in the view can start a track or seek, so the anchor can
        // move during the draw and the clock has to follow it.
        if self.transport.anchor() != before {
            self.since = Some(now);
        }
        // Last, so a click during the draw is published before the next frame.
        self.send_report();
    }
}

/// The frame's wall clock in epoch milliseconds.
///
/// A clock that cannot be read as a positive span since the epoch reads as `0`,
/// which makes every date a future one and so draws no cell.
fn wall_clock_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// The hour the greeting says, from an instant in any time zone.
///
/// The greeting is the clock and nothing else, so this is the whole conversion:
/// `hour()` follows the zone's offset, and a value that cannot be a `u8`
/// saturates, which the greeting reads as an evening hour.
fn local_hour_at<Tz: TimeZone>(at: &DateTime<Tz>) -> u8 {
    u8::try_from(at.hour()).unwrap_or(u8::MAX)
}

/// The local hour of the day, read once per frame.
///
/// The time zone is the machine's, which is what "Good morning" is about; a
/// hand-written `localtime` would need `unsafe`, which is denied.
fn local_hour() -> u8 {
    local_hour_at(&Local::now())
}

/// The window's background. The channels between panels read as the window
/// colour, so the default translucent grey is not wanted.
pub(crate) fn window_clear_color(tokens: &Tokens) -> [f32; 4] {
    tokens.palette.window.to_normalized_gamma_f32()
}

/// Applies every queued update without blocking, in arrival order, and returns
/// the commands the queue derives from them.
///
/// A disconnected sender is not an error: it means the worker is gone and the
/// window can no longer hear anything.
#[expect(
    clippy::too_many_arguments,
    reason = "one store per thing an update can change, in the order it arrived"
)]
pub(crate) fn drain(
    rx: &Receiver<Update>,
    state: &mut SignIn,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    devices: &mut Devices,
    art: &mut Art,
    clock: &mut Playclock,
) -> Vec<Command> {
    let mut commands = Vec::new();
    while let Ok(update) = rx.try_recv() {
        state.apply(update.clone());
        library.apply(&update);
        let before = transport.anchor();
        transport.apply(&update);
        devices.apply(&update, clock.now);
        art.apply(&update, clock.now);
        // A position that moved is an instant the bar counts from, and an order
        // later in this same drain has to read the position it left.
        if transport.anchor() != before {
            clock.since = Some(clock.now);
        }
        // The phone's transport is applied here rather than in the queue's own
        // reducer, because an order needs the clock that reducer cannot see.
        if let Update::Remote(RemoteUpdate::Order { id, order, expires }) = &update {
            commands.extend(order_commands(
                order, *id, *expires, transport, queue, clock,
            ));
            continue;
        }
        if let Some(command) = queue.apply(&update, transport) {
            commands.push(command);
        }
    }
    commands
}

/// Carries out one phone order, and always answers it last.
///
/// An order the UI keeps past its expiry is dropped unapplied: the entry's own
/// budget has told the phone `Failure` by then, and a change nobody hears about
/// is worse than none.
fn order_commands(
    order: &Order,
    id: RequestId,
    expires: Instant,
    transport: &mut Transport,
    queue: &mut Queue,
    clock: &Playclock,
) -> Vec<Command> {
    if clock.now >= expires {
        return vec![Command::Answer { id, ok: false }];
    }
    let reaction = react(order, queue, transport, clock.position(transport));
    let mut commands = reaction.commands;
    commands.push(Command::Answer {
        id,
        ok: reaction.ok,
    });
    commands
}

/// The instant the drain ran, and when the transport's anchor last moved.
///
/// The position a phone order acts on is extrapolated from these two, so an
/// order that arrives with a position event in the same drain reads the fresh
/// one rather than the previous frame's.
pub(crate) struct Playclock {
    now: Instant,
    since: Option<Instant>,
}

impl Playclock {
    /// A clock that counts from nothing yet played.
    pub(crate) fn at(now: Instant) -> Self {
        Self { now, since: None }
    }

    /// The same clock, counting from when the transport's anchor last moved.
    fn counting_from(self, since: Option<Instant>) -> Self {
        Self { since, ..self }
    }

    /// The position the transport shows, counted from its anchor.
    fn position(&self, transport: &Transport) -> u32 {
        let elapsed = self.since.map_or(Duration::ZERO, |since| {
            self.now.saturating_duration_since(since)
        });
        transport.shown_position(elapsed)
    }
}

/// Asks egui for the frame the cover store's next retry or deadline needs.
///
/// `logic()` repaints on its own only while a track plays, so without this an
/// idle window would leave a failed cover at `Failed` forever. A delay of at
/// least a millisecond avoids a zero-delay repaint storm.
fn schedule_wake(ctx: &egui::Context, art: &Art, now: Instant) {
    if let Some(delay) = art.next_wake(now) {
        ctx.request_repaint_after(delay.max(WAKE_FLOOR));
    }
}

#[cfg(test)]
mod tests;
