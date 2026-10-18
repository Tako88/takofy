//! The eframe application: the window's state and its draw pass.

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui;
use takofy_ui::library::Library;
use takofy_ui::message::Update;
use takofy_ui::player::{REDRAW, Transport, TransportState, position_at};
use takofy_ui::signin::SignIn;
use takofy_ui::theme::{Tokens, style};
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
        ..Default::default()
    }
}

/// The window and its end of the bridge.
pub(crate) struct TakofyApp {
    state: SignIn,
    library: Library,
    transport: Transport,
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
        cc.egui_ctx.set_global_style(style(&tokens));
        Self {
            state: SignIn::default(),
            library: Library::default(),
            transport: Transport::default(),
            ui_end,
            tokens,
            since: None,
        }
    }
}

impl eframe::App for TakofyApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let before = self.transport.anchor();
        drain(
            &self.ui_end.updates,
            &mut self.state,
            &mut self.library,
            &mut self.transport,
        );
        if self.transport.anchor() != before {
            self.since = Some(Instant::now());
        }
        if self.transport.state == TransportState::Playing {
            ctx.request_repaint_after(REDRAW);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let position_ms = position_at(
            &self.transport.anchor(),
            self.since.map_or(Duration::ZERO, |since| since.elapsed()),
        );
        if let Some(command) = view::show(
            ui,
            &mut self.state,
            &mut self.library,
            &mut self.transport,
            position_ms,
            &self.tokens,
        ) {
            let _ = self.ui_end.commands.send(command);
        }
    }
}

/// Applies every queued update without blocking, and reports how many landed.
///
/// A disconnected sender is not an error: it means the worker is gone and the
/// window can no longer hear anything.
pub(crate) fn drain(
    rx: &Receiver<Update>,
    state: &mut SignIn,
    library: &mut Library,
    transport: &mut Transport,
) -> usize {
    let mut applied = 0;
    while let Ok(update) = rx.try_recv() {
        state.apply(update.clone());
        library.apply(&update);
        transport.apply(&update);
        applied += 1;
    }
    applied
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;

    use takofy_ui::library::Load;
    use takofy_ui::message::{Failure, TransportUpdate};

    use super::*;

    #[test]
    fn drain_applies_every_update_in_order() {
        let (tx, rx) = channel();
        let mut state = SignIn::Starting;
        let mut library = Library::default();
        let mut transport = Transport::default();
        tx.send(Update::NeedsAuthorization).expect("send");
        tx.send(Update::SignInFailed(Failure::PortBusy))
            .expect("send");

        let applied = drain(&rx, &mut state, &mut library, &mut transport);

        assert_eq!(applied, 2);
        assert_eq!(state, SignIn::NeedsAuthorization);
    }

    #[test]
    fn drain_feeds_the_library() {
        let (tx, rx) = channel();
        let mut state = SignIn::Starting;
        let mut library = Library::default();
        let mut transport = Transport::default();
        tx.send(Update::SignedIn).expect("send");
        tx.send(Update::Playlists(Vec::new())).expect("send");

        assert_eq!(drain(&rx, &mut state, &mut library, &mut transport), 2);
        assert_eq!(library.playlists, Load::Ready(Vec::new()));
    }

    #[test]
    fn drain_feeds_the_transport() {
        let (tx, rx) = channel();
        let mut state = SignIn::SignedIn;
        let mut library = Library::default();
        let mut transport = Transport::default();
        tx.send(Update::Transport(TransportUpdate::Position {
            position_ms: 9,
            paused: false,
        }))
        .expect("send");

        assert_eq!(drain(&rx, &mut state, &mut library, &mut transport), 1);
        assert_eq!(transport.state, TransportState::Playing);
        assert_eq!(transport.position_ms, 9);
    }

    #[test]
    fn drain_returns_zero_on_an_empty_channel_without_blocking() {
        let (_tx, rx) = channel();
        let mut state = SignIn::Starting;
        let mut library = Library::default();
        let mut transport = Transport::default();

        assert_eq!(drain(&rx, &mut state, &mut library, &mut transport), 0);
        assert_eq!(state, SignIn::Starting);
    }

    #[test]
    fn drain_survives_a_disconnected_sender() {
        let (tx, rx) = channel();
        let mut state = SignIn::Starting;
        let mut library = Library::default();
        let mut transport = Transport::default();
        tx.send(Update::SignedIn).expect("send");
        drop(tx);

        assert_eq!(drain(&rx, &mut state, &mut library, &mut transport), 1);
        assert_eq!(state, SignIn::SignedIn);
        assert_eq!(drain(&rx, &mut state, &mut library, &mut transport), 0);
    }

    #[test]
    fn run_and_return_is_pinned() {
        assert!(native_options().run_and_return);
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
