//! The eframe application: the window's state and its draw pass.

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;
use takofy_ui::cover::Art;
use takofy_ui::drawer::Drawer;
use takofy_ui::library::Library;
use takofy_ui::menu::Menu;
use takofy_ui::message::{Command, Update};
use takofy_ui::player::{REDRAW, Transport, TransportState};
use takofy_ui::queue::Queue;
use takofy_ui::signin::SignIn;
use takofy_ui::theme::{Tokens, fonts, scroll, style};
use takofy_ui::{layout, view};

use crate::bridge::UiEnd;

/// Window options. `run_and_return` must stay true: shutdown runs after the
/// window closes, and eframe must return instead of exiting the process.
pub(crate) fn native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(layout::WINDOW_DEFAULT)
            .with_min_inner_size(layout::WINDOW_MIN),
        run_and_return: true,
        // Every glyph is bundled, so a system fallback would hide a gap on this
        // machine and ship boxes on any other.
        system_font_fallback: false,
        ..Default::default()
    }
}

/// The window and its end of the bridge.
pub(crate) struct TakofyApp {
    state: SignIn,
    library: Library,
    transport: Transport,
    queue: Queue,
    drawer: Drawer,
    menu: Menu,
    ui_end: UiEnd,
    tokens: Tokens,
    /// The decoded covers, the uploads and the fetches in flight.
    art: Art,
    /// Restamped whenever the transport's anchor moves, so the interpolated
    /// position cannot drift from it.
    since: Option<Instant>,
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
    }

    /// Asks for the frame that sends the read the draw just armed.
    ///
    /// `logic` runs before `ui`, so a read armed during the draw — Back or
    /// Forward onto a page that has to ask again — would otherwise wait for the
    /// next input event. The page's query is copied out for the same reason
    /// [`Self::request_search`] copies it.
    fn wake_search(&self, ctx: &egui::Context) {
        let page = self.library.page_search_query().map(str::to_owned);
        if self.library.search.wants(page.as_deref()) {
            ctx.request_repaint();
        }
    }

    /// Installs the theme and takes ownership of the window's end.
    pub(crate) fn new(cc: &eframe::CreationContext<'_>, ui_end: UiEnd) -> Self {
        let tokens = Tokens::DARK;
        cc.egui_ctx.set_fonts(fonts::definitions());
        cc.egui_ctx.set_global_style(style(&tokens));
        // The wheel's own conversion, so a page passed to egui still travels the
        // same distance per line as one this client applies itself.
        cc.egui_ctx.options_mut(|options| {
            options.input_options.line_scroll_speed = scroll::POINTS_PER_LINE;
        });
        Self {
            state: SignIn::default(),
            library: Library::default(),
            transport: Transport::default(),
            queue: Queue::default(),
            drawer: Drawer::default(),
            menu: Menu::default(),
            ui_end,
            tokens,
            art: Art::new(),
            since: None,
        }
    }
}

impl eframe::App for TakofyApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let before = self.transport.anchor();
        let commands = drain(
            &self.ui_end.updates,
            &mut self.state,
            &mut self.library,
            &mut self.transport,
            &mut self.queue,
            &mut self.art,
            now,
        );
        for command in commands {
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
        // The artist page's likeness row is its second read: it is asked for
        // once the artist's own detail has landed and named somebody.
        if let Some(command) = self.library.related_request() {
            let _ = self.ui_end.commands.send(command);
        }
        // The search reads are want-driven like the lists above: the menu while
        // the field is open, the page while it is on screen.
        self.request_search();
        // After the drain and before the draw, so a cover that just arrived is
        // ready in this same frame's `ui()`.
        self.art.upload(ctx);
        if self.transport.anchor() != before {
            self.since = Some(now);
        }
        if self.transport.state == TransportState::Playing {
            ctx.request_repaint_after(REDRAW);
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        window_clear_color(&self.tokens)
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
                drawer: &mut self.drawer,
                menu: &mut self.menu,
                position_ms,
                art: &mut self.art,
                now,
                now_ms: wall_clock_ms(),
            };
            view::show(ui, &mut screen, &self.tokens)
        };
        if let Some(command) = command {
            let _ = self.ui_end.commands.send(command);
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
pub(crate) fn drain(
    rx: &Receiver<Update>,
    state: &mut SignIn,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
    now: Instant,
) -> Vec<Command> {
    let mut commands = Vec::new();
    while let Ok(update) = rx.try_recv() {
        state.apply(update.clone());
        library.apply(&update);
        transport.apply(&update);
        art.apply(&update, now);
        if let Some(command) = queue.apply(&update, transport) {
            commands.push(command);
        }
    }
    commands
}

/// Asks egui for the frame the cover store's next retry or deadline needs.
///
/// `logic()` repaints on its own only while a track plays, so without this an
/// idle window would leave a failed cover at `Failed` forever. A delay of at
/// least a millisecond avoids a zero-delay repaint storm.
fn schedule_wake(ctx: &egui::Context, art: &Art, now: Instant) {
    if let Some(delay) = art.next_wake(now) {
        ctx.request_repaint_after(delay.max(Duration::from_millis(1)));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;

    use eframe::App;
    use takofy_model::{CoverImage, CoverUrl, TrackEntry};
    use takofy_ui::library::Load;
    use takofy_ui::message::{Command, Failure, TransportUpdate};
    use takofy_ui::route::Page;

    use super::*;

    fn track(uri: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: uri.to_owned(),
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

    #[test]
    fn wall_clock_ms_is_within_a_minute_of_the_system_clock() {
        let system = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the system clock is after the epoch")
            .as_millis();
        let reading = u128::from(wall_clock_ms());
        assert!(
            system.abs_diff(reading) < 60_000,
            "read {reading} against the system's {system}"
        );
        assert!(reading > 1_704_067_200_000, "a 2024 floor, read {reading}");
    }

    #[test]
    fn drain_applies_every_update_in_order() {
        let (tx, rx) = channel();
        let mut state = SignIn::Starting;
        let mut library = Library::default();
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        tx.send(Update::NeedsAuthorization).expect("send");
        tx.send(Update::SignInFailed(Failure::PortBusy))
            .expect("send");

        let commands = drain(
            &rx,
            &mut state,
            &mut library,
            &mut transport,
            &mut queue,
            &mut Art::default(),
            Instant::now(),
        );

        assert!(commands.is_empty());
        assert_eq!(state, SignIn::NeedsAuthorization);
    }

    #[test]
    fn drain_feeds_the_library() {
        let (tx, rx) = channel();
        let mut state = SignIn::Starting;
        let mut library = Library::default();
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        tx.send(Update::SignedIn).expect("send");
        tx.send(Update::Playlists(Vec::new())).expect("send");

        let commands = drain(
            &rx,
            &mut state,
            &mut library,
            &mut transport,
            &mut queue,
            &mut Art::default(),
            Instant::now(),
        );

        assert!(commands.is_empty());
        assert_eq!(library.playlists, Load::Ready(Vec::new()));
    }

    #[test]
    fn drain_feeds_the_transport() {
        let (tx, rx) = channel();
        let mut state = SignIn::SignedIn;
        let mut library = Library::default();
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        tx.send(Update::Transport(TransportUpdate::Position {
            position_ms: 9,
            paused: false,
        }))
        .expect("send");

        let commands = drain(
            &rx,
            &mut state,
            &mut library,
            &mut transport,
            &mut queue,
            &mut Art::default(),
            Instant::now(),
        );

        assert!(commands.is_empty());
        assert_eq!(transport.state, TransportState::Playing);
        assert_eq!(transport.position_ms, 9);
    }

    #[test]
    fn drain_returns_nothing_on_an_empty_channel_without_blocking() {
        let (_tx, rx) = channel();
        let mut state = SignIn::Starting;
        let mut library = Library::default();
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        assert!(
            drain(
                &rx,
                &mut state,
                &mut library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now()
            )
            .is_empty(),
            "an empty channel yields no commands"
        );
        assert_eq!(state, SignIn::Starting);
    }

    #[test]
    fn drain_survives_a_disconnected_sender() {
        let (tx, rx) = channel();
        let mut state = SignIn::Starting;
        let mut library = Library::default();
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        tx.send(Update::SignedIn).expect("send");
        drop(tx);

        assert!(
            drain(
                &rx,
                &mut state,
                &mut library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now()
            )
            .is_empty()
        );
        assert_eq!(state, SignIn::SignedIn);
        assert!(
            drain(
                &rx,
                &mut state,
                &mut library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now()
            )
            .is_empty()
        );
    }

    #[test]
    fn drain_returns_the_next_play_when_a_track_ends() {
        let (tx, rx) = channel();
        let mut state = SignIn::SignedIn;
        let mut library = Library::default();
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let all = vec![track("a"), track("b")];
        assert_eq!(
            queue.start(&all, 0, &mut transport),
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
        tx.send(Update::Transport(TransportUpdate::Loading))
            .expect("send");
        tx.send(Update::Transport(TransportUpdate::Ended))
            .expect("send");

        let commands = drain(
            &rx,
            &mut state,
            &mut library,
            &mut transport,
            &mut queue,
            &mut Art::default(),
            Instant::now(),
        );

        assert_eq!(
            commands,
            vec![Command::Play {
                uri: "b".to_owned()
            }]
        );
    }

    #[test]
    fn drain_skips_an_unavailable_track() {
        let (tx, rx) = channel();
        let mut state = SignIn::SignedIn;
        let mut library = Library::default();
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let all = vec![track("a"), track("b")];
        assert_eq!(
            queue.start(&all, 0, &mut transport),
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
        tx.send(Update::Transport(TransportUpdate::Loading))
            .expect("send");
        tx.send(Update::Transport(TransportUpdate::Unavailable))
            .expect("send");

        let commands = drain(
            &rx,
            &mut state,
            &mut library,
            &mut transport,
            &mut queue,
            &mut Art::default(),
            Instant::now(),
        );

        assert_eq!(
            commands,
            vec![Command::Play {
                uri: "b".to_owned()
            }]
        );
    }

    #[test]
    fn logic_requests_the_followed_list_once_playlists_are_ready() {
        // `logic()` only forwards this return value into the command queue, so
        // the contract is `Library::followed_request` itself.
        let mut library = Library::default();
        assert_eq!(library.followed_request(), None, "nothing has loaded yet");

        library.apply(&Update::Playlists(Vec::new()));
        assert_eq!(library.followed_request(), Some(Command::LoadFollowedShows));
        assert_eq!(library.followed, Load::Loading);
        assert_eq!(
            library.followed_request(),
            None,
            "the load is in flight, so the next frame sends nothing"
        );
    }

    /// The artists and albums lists are want-driven like the shows: one send
    /// per chip press, and none before the chip is the chosen one.
    #[test]
    fn logic_requests_the_artists_and_albums_only_once_their_chip_is_chosen() {
        let mut library = Library::default();
        assert_eq!(library.followed_artists_request(), None);
        assert_eq!(library.saved_albums_request(), None);

        // Ready playlists alone ask for nothing: the root still draws All.
        library.apply(&Update::Playlists(Vec::new()));
        assert_eq!(library.followed_artists_request(), None);
        assert_eq!(library.saved_albums_request(), None);

        assert!(
            library
                .root_filter
                .choose(takofy_ui::library_filter::RootKind::Artists)
        );
        assert_eq!(
            library.followed_artists_request(),
            Some(Command::LoadFollowedArtists)
        );
        assert_eq!(library.followed_artists, Load::Loading);
        assert_eq!(
            library.followed_artists_request(),
            None,
            "the load is in flight, so the next frame sends nothing"
        );

        assert!(
            library
                .root_filter
                .choose(takofy_ui::library_filter::RootKind::Albums)
        );
        assert_eq!(
            library.saved_albums_request(),
            Some(Command::LoadSavedAlbums)
        );
        assert_eq!(library.saved_albums_request(), None);

        // The sidebar's chips are the other surface that names the same lists.
        let mut sidebar = Library::default();
        sidebar.apply(&Update::Playlists(Vec::new()));
        assert!(
            sidebar
                .sidebar_kind
                .choose(takofy_ui::sidebar_kind::SidebarKind::Artists)
        );
        assert_eq!(
            sidebar.followed_artists_request(),
            Some(Command::LoadFollowedArtists)
        );
        assert!(
            sidebar
                .sidebar_kind
                .choose(takofy_ui::sidebar_kind::SidebarKind::Albums)
        );
        assert_eq!(
            sidebar.saved_albums_request(),
            Some(Command::LoadSavedAlbums)
        );
    }

    #[test]
    fn the_clear_colour_is_the_window_token() {
        let tokens = Tokens::DARK;
        assert_eq!(
            window_clear_color(&tokens),
            tokens.palette.window.to_normalized_gamma_f32()
        );
    }

    #[test]
    fn run_and_return_is_pinned() {
        assert!(native_options().run_and_return);
    }

    #[test]
    fn the_system_font_fallback_stays_off() {
        assert!(
            !native_options().system_font_fallback,
            "a fallback would hide a missing glyph here and show one elsewhere"
        );
    }

    #[test]
    fn the_shipping_chain_keeps_egui_icons() {
        // This binary links eframe, which is what turns on egui's `default_fonts`;
        // a crate depending on egui alone has no builtin fonts at all. So this is
        // the only place the configuration that actually ships can be asserted.
        let defs = takofy_ui::theme::fonts::definitions();
        for (family, chain) in &defs.families {
            assert!(
                chain.iter().any(|name| name == "egui-icons"),
                "{family:?} would leave egui's own icons undrawn"
            );
        }
    }

    #[test]
    fn the_viewport_uses_the_layout_sizes() {
        let options = native_options();
        assert_eq!(
            options.viewport.inner_size,
            Some(egui::vec2(
                layout::WINDOW_DEFAULT[0],
                layout::WINDOW_DEFAULT[1]
            ))
        );
        assert_eq!(
            options.viewport.min_inner_size,
            Some(egui::vec2(layout::WINDOW_MIN[0], layout::WINDOW_MIN[1]))
        );
    }

    #[test]
    fn drain_routes_a_failed_cover_into_the_store() {
        let (tx, rx) = channel();
        let mut state = SignIn::SignedIn;
        let mut library = Library::default();
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let now = Instant::now();
        let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url");
        // The URL must be known before its failure arrives, as it is in a frame:
        // the draw looks it, sends the fetch, and the failure answers it.
        assert_eq!(art.look(Some(&url), now), takofy_ui::cover::Cover::Loading);
        let _ = art.requests(now);
        tx.send(Update::CoverFailed { url: url.clone() })
            .expect("send");

        let commands = drain(
            &rx,
            &mut state,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
            now,
        );

        assert!(commands.is_empty());
        assert_eq!(art.look(Some(&url), now), takofy_ui::cover::Cover::Failed);
    }

    #[test]
    fn drain_routes_a_cover_into_the_store() {
        let (tx, rx) = channel();
        let mut state = SignIn::SignedIn;
        let mut library = Library::default();
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let now = Instant::now();
        let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url");
        art.look(Some(&url), now);
        let _ = art.requests(now);
        tx.send(Update::Cover {
            url: url.clone(),
            image: CoverImage::new(1, 1, vec![0; 4]).expect("a 1x1 image"),
        })
        .expect("send");

        let _ = drain(
            &rx,
            &mut state,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
            now,
        );
        let ctx = egui::Context::default();
        art.upload(&ctx);

        assert!(matches!(
            art.look(Some(&url), now),
            takofy_ui::cover::Cover::Ready(_)
        ));
    }

    #[test]
    fn a_failed_cover_asks_for_a_wake_at_its_retry() {
        let mut art = Art::default();
        let now = Instant::now();
        let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url");
        art.begin_frame();
        art.look(Some(&url), now);
        let _ = art.requests(now);
        art.apply(&Update::CoverFailed { url: url.clone() }, now);
        art.begin_frame();
        assert_eq!(art.look(Some(&url), now), takofy_ui::cover::Cover::Failed);

        let ctx = egui::Context::default();
        // Warm up so egui's own first-frame work (the font atlas and its
        // immediate repaint) is not mistaken for a cover wake.
        for _ in 0..2 {
            let mut warm = ctx.run_ui(egui::RawInput::default(), |_ui| {});
            warm.textures_delta.clear();
        }
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            schedule_wake(ui.ctx(), &art, now);
        });
        output.textures_delta.clear();
        let delay = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .expect("the root viewport")
            .repaint_delay;
        assert!(
            delay <= Duration::from_secs(5),
            "a retry in five seconds woke the window in {delay:?}"
        );

        // Nothing pending leaves egui's own default alone. A fresh context is
        // used because the one above is holding the repaint it was asked for.
        let idle = Art::default();
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let mut warm = ctx.run_ui(egui::RawInput::default(), |_ui| {});
            warm.textures_delta.clear();
        }
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            schedule_wake(ui.ctx(), &idle, now);
        });
        output.textures_delta.clear();
        assert_eq!(
            output
                .viewport_output
                .get(&egui::ViewportId::ROOT)
                .expect("the root viewport")
                .repaint_delay,
            Duration::MAX,
            "an idle window must not be woken"
        );
    }

    /// A window with no worker behind it, so what `logic` sends can be read
    /// back from the command channel.
    fn test_window() -> (TakofyApp, tokio::sync::mpsc::UnboundedReceiver<Command>) {
        let (sender, updates) = channel();
        // Nothing will ever arrive; a disconnected channel is what the window
        // already tolerates.
        drop(sender);
        let (commands, received) = tokio::sync::mpsc::unbounded_channel();
        let app = TakofyApp {
            state: SignIn::default(),
            library: Library::default(),
            transport: Transport::default(),
            queue: Queue::default(),
            drawer: Drawer::default(),
            menu: Menu::default(),
            ui_end: UiEnd { updates, commands },
            tokens: Tokens::DARK,
            art: Art::new(),
            since: None,
        };
        (app, received)
    }

    /// Types `text` into the field, as one frame of the real path would.
    fn type_search(library: &mut Library, text: &str) {
        library.search.text = text.to_owned();
        let input = takofy_ui::search::KeyInput {
            typed: true,
            focused: true,
            ..takofy_ui::search::KeyInput::default()
        };
        let _ = library.search.step(input, 0, false);
    }

    #[test]
    fn drain_feeds_the_search_state() {
        let (tx, rx) = channel();
        let mut state = SignIn::SignedIn;
        let mut library = Library::default();
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        type_search(&mut library, "never gonna");
        assert_eq!(
            library.search.request(None),
            Some(Command::Search {
                query: "never gonna".to_owned()
            })
        );
        tx.send(Update::Search {
            query: "never gonna".to_owned(),
            tracks: vec![track("spotify:track:a")],
        })
        .expect("send");

        let commands = drain(
            &rx,
            &mut state,
            &mut library,
            &mut transport,
            &mut queue,
            &mut Art::default(),
            Instant::now(),
        );

        assert!(commands.is_empty());
        assert_eq!(
            library.search.menu_view(),
            takofy_ui::search::MenuView::Rows {
                tracks: &[track("spotify:track:a")],
                total: 1,
                floor: false,
            },
            "a drained reply reaches the search state"
        );
    }

    /// Runs one frame of the real `logic` pass, which is where a search read is
    /// armed. `logic` is an [`eframe::App`] method, so the frame is the one
    /// eframe's own test harness builds.
    fn logic_frame(app: &mut TakofyApp) {
        app.logic(
            &egui::Context::default(),
            &mut eframe::Frame::_new_kittest(),
        );
    }

    #[test]
    fn logic_asks_for_the_typed_query_once() {
        let (mut app, mut received) = test_window();
        type_search(&mut app.library, "never gonna");

        logic_frame(&mut app);
        assert_eq!(
            received.try_recv().expect("the search read"),
            Command::Search {
                query: "never gonna".to_owned()
            }
        );
        logic_frame(&mut app);
        assert!(
            received.try_recv().is_err(),
            "one read on the wire at a time, so a burst cannot queue twenty"
        );
    }

    #[test]
    fn focus_alone_asks_for_no_search() {
        let (mut app, mut received) = test_window();
        let focused = takofy_ui::search::KeyInput {
            focused: true,
            ..takofy_ui::search::KeyInput::default()
        };
        let _ = app.library.search.step(focused, 0, false);

        logic_frame(&mut app);
        assert!(
            received.try_recv().is_err(),
            "an empty query fetches nothing (R6)"
        );
    }

    /// An open results page is a read the `logic` pass asks for, keyed by the
    /// page's own query. It is the seam the route and the page meet at: the
    /// field is not involved at all.
    #[test]
    fn logic_asks_for_the_open_results_page_once() {
        let (mut app, mut received) = test_window();
        let _ = app
            .library
            .open(takofy_ui::route::Page::Search("never gonna".to_owned()));

        logic_frame(&mut app);
        assert_eq!(
            received.try_recv().expect("the page's read"),
            Command::Search {
                query: "never gonna".to_owned()
            }
        );
        logic_frame(&mut app);
        assert!(
            received.try_recv().is_err(),
            "one read on the wire at a time"
        );
    }

    /// A window's context, set up the way `TakofyApp::new` sets one up.
    fn window_context() -> egui::Context {
        let tokens = Tokens::DARK;
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        ctx
    }

    /// One draw pass, as eframe runs it between two `logic` passes.
    fn ui_frame(app: &mut TakofyApp, ctx: &egui::Context) -> egui::FullOutput {
        let mut frame = eframe::Frame::_new_kittest();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(layout::WINDOW_DEFAULT[0], layout::WINDOW_DEFAULT[1]),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| app.ui(ui, &mut frame));
        output.textures_delta.clear();
        output
    }

    /// A dropped results page is asked for after the draw, but `logic` has
    /// already run this frame: the draw must ask for the next one, or the page
    /// sits on its skeleton until the user moves the mouse.
    #[test]
    fn back_onto_a_dropped_search_page_asks_for_the_frame_that_sends_it() {
        let (mut app, mut received) = test_window();
        app.state = SignIn::SignedIn;
        let _ = app.library.open(Page::Search("never gonna".to_owned()));
        app.library.apply(&Update::Search {
            query: "never gonna".to_owned(),
            tracks: vec![track("spotify:track:a")],
        });
        let _ = app.library.open(Page::Album("spotify:album:a".to_owned()));
        // A second query drops the first page's answer.
        let _ = app.library.open(Page::Search("give you up".to_owned()));
        // Back onto the album, then onto the dropped page, as the topbar does.
        let _ = app.library.back();
        assert_eq!(app.library.back(), None);
        assert_eq!(app.library.page_search_query(), Some("never gonna"));

        let ctx = window_context();
        // Warm up, so egui's own first-pass work is not read as this draw's.
        for _ in 0..2 {
            let mut warm = ctx.run_ui(egui::RawInput::default(), |_ui| {});
            warm.textures_delta.clear();
        }
        let _ = ui_frame(&mut app, &ctx);
        // egui reports a pass's repaint causes on the pass after it, which is
        // the frame this test is about: the one that sends the read.
        let mut wake = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        wake.textures_delta.clear();
        let causes = ctx.repaint_causes();
        assert!(
            causes.iter().any(|cause| cause.file.ends_with("app.rs")),
            "the draw armed a read, so it must ask for the frame that sends it: {causes:?}"
        );

        app.logic(&ctx, &mut eframe::Frame::_new_kittest());
        assert_eq!(
            received.try_recv().expect("the read the draw armed"),
            Command::Search {
                query: "never gonna".to_owned()
            }
        );
    }
}
