//! The eframe application: the window's state and its draw pass.

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui;
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
            since: None,
        }
    }
}

impl eframe::App for TakofyApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let before = self.transport.anchor();
        let commands = drain(
            &self.ui_end.updates,
            &mut self.state,
            &mut self.library,
            &mut self.transport,
            &mut self.queue,
        );
        for command in commands {
            let _ = self.ui_end.commands.send(command);
        }
        if self.transport.anchor() != before {
            self.since = Some(Instant::now());
        }
        if self.transport.state == TransportState::Playing {
            ctx.request_repaint_after(REDRAW);
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        window_clear_color(&self.tokens)
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let position_ms = self
            .transport
            .shown_position(self.since.map_or(Duration::ZERO, |since| since.elapsed()));
        let before = self.transport.anchor();
        let mut screen = view::Screen {
            state: &mut self.state,
            library: &mut self.library,
            transport: &mut self.transport,
            queue: &mut self.queue,
            drawer: &mut self.drawer,
            position_ms,
        };
        if let Some(command) = view::show(ui, &mut screen, &self.tokens) {
            let _ = self.ui_end.commands.send(command);
        }
        // A control in the view can start a track or seek, so the anchor can
        // move during the draw and the clock has to follow it.
        if self.transport.anchor() != before {
            self.since = Some(Instant::now());
        }
    }
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
) -> Vec<Command> {
    let mut commands = Vec::new();
    while let Ok(update) = rx.try_recv() {
        state.apply(update.clone());
        library.apply(&update);
        transport.apply(&update);
        if let Some(command) = queue.apply(&update, transport) {
            commands.push(command);
        }
    }
    commands
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;

    use takofy_model::TrackEntry;
    use takofy_ui::library::Load;
    use takofy_ui::message::{Command, Failure, TransportUpdate};

    use super::*;

    fn track(uri: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: uri.to_owned(),
            artists: String::new(),
            album: None,
            duration_ms: None,
            named: true,
        }
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

        let commands = drain(&rx, &mut state, &mut library, &mut transport, &mut queue);

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

        let commands = drain(&rx, &mut state, &mut library, &mut transport, &mut queue);

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

        let commands = drain(&rx, &mut state, &mut library, &mut transport, &mut queue);

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
            drain(&rx, &mut state, &mut library, &mut transport, &mut queue).is_empty(),
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

        assert!(drain(&rx, &mut state, &mut library, &mut transport, &mut queue).is_empty());
        assert_eq!(state, SignIn::SignedIn);
        assert!(drain(&rx, &mut state, &mut library, &mut transport, &mut queue).is_empty());
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

        let commands = drain(&rx, &mut state, &mut library, &mut transport, &mut queue);

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

        let commands = drain(&rx, &mut state, &mut library, &mut transport, &mut queue);

        assert_eq!(
            commands,
            vec![Command::Play {
                uri: "b".to_owned()
            }]
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
}
