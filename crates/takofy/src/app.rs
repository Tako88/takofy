//! The eframe application: the window's state and its draw pass.

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;
use takofy_ui::cover::Art;
use takofy_ui::drawer::Drawer;
use takofy_ui::library::Library;
use takofy_ui::message::{Command, Update};
use takofy_ui::player::{REDRAW, Transport, TransportState};
use takofy_ui::queue::Queue;
use takofy_ui::signin::SignIn;
use takofy_ui::theme::{Tokens, fonts, style};
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
    ui_end: UiEnd,
    tokens: Tokens,
    /// The decoded covers, the uploads and the fetches in flight.
    art: Art,
    /// Restamped whenever the transport's anchor moves, so the interpolated
    /// position cannot drift from it.
    since: Option<Instant>,
}

impl TakofyApp {
    /// Installs the theme and takes ownership of the window's end.
    pub(crate) fn new(cc: &eframe::CreationContext<'_>, ui_end: UiEnd) -> Self {
        let tokens = Tokens::DARK;
        cc.egui_ctx.set_fonts(fonts::definitions());
        cc.egui_ctx.set_global_style(style(&tokens));
        Self {
            state: SignIn::default(),
            library: Library::default(),
            transport: Transport::default(),
            queue: Queue::default(),
            drawer: Drawer::default(),
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

    use takofy_model::{CoverImage, CoverUrl, TrackEntry};
    use takofy_ui::library::Load;
    use takofy_ui::message::{Command, Failure, TransportUpdate};

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
}
