//! The eframe application: the window's state and its draw pass.

use std::sync::mpsc::Receiver;

use eframe::egui;
use takofy_ui::library::Library;
use takofy_ui::message::Update;
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
    ui_end: UiEnd,
    tokens: Tokens,
}

impl TakofyApp {
    /// Installs the theme and takes ownership of the window's end.
    pub(crate) fn new(cc: &eframe::CreationContext<'_>, ui_end: UiEnd) -> Self {
        let tokens = Tokens::DARK;
        cc.egui_ctx.set_global_style(style(&tokens));
        Self {
            state: SignIn::default(),
            library: Library::default(),
            ui_end,
            tokens,
        }
    }
}

impl eframe::App for TakofyApp {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        drain(&self.ui_end.updates, &mut self.state, &mut self.library);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(command) = view::show(ui, &mut self.state, &mut self.library, &self.tokens) {
            let _ = self.ui_end.commands.send(command);
        }
    }
}

/// Applies every queued update without blocking, and reports how many landed.
///
/// A disconnected sender is not an error: it means the worker is gone and the
/// window can no longer hear anything.
pub(crate) fn drain(rx: &Receiver<Update>, state: &mut SignIn, library: &mut Library) -> usize {
    let mut applied = 0;
    while let Ok(update) = rx.try_recv() {
        state.apply(update.clone());
        library.apply(&update);
        applied += 1;
    }
    applied
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;

    use takofy_ui::library::Load;
    use takofy_ui::message::Failure;

    use super::*;

    #[test]
    fn drain_applies_every_update_in_order() {
        let (tx, rx) = channel();
        let mut state = SignIn::Starting;
        let mut library = Library::default();
        tx.send(Update::NeedsAuthorization).expect("send");
        tx.send(Update::SignInFailed(Failure::PortBusy))
            .expect("send");

        let applied = drain(&rx, &mut state, &mut library);

        assert_eq!(applied, 2);
        assert_eq!(state, SignIn::NeedsAuthorization);
    }

    #[test]
    fn drain_feeds_the_library() {
        let (tx, rx) = channel();
        let mut state = SignIn::Starting;
        let mut library = Library::default();
        tx.send(Update::SignedIn).expect("send");
        tx.send(Update::Playlists(Vec::new())).expect("send");

        assert_eq!(drain(&rx, &mut state, &mut library), 2);
        assert_eq!(library.playlists, Load::Ready(Vec::new()));
    }

    #[test]
    fn drain_returns_zero_on_an_empty_channel_without_blocking() {
        let (_tx, rx) = channel();
        let mut state = SignIn::Starting;
        let mut library = Library::default();

        assert_eq!(drain(&rx, &mut state, &mut library), 0);
        assert_eq!(state, SignIn::Starting);
    }

    #[test]
    fn drain_survives_a_disconnected_sender() {
        let (tx, rx) = channel();
        let mut state = SignIn::Starting;
        let mut library = Library::default();
        tx.send(Update::SignedIn).expect("send");
        drop(tx);

        assert_eq!(drain(&rx, &mut state, &mut library), 1);
        assert_eq!(state, SignIn::SignedIn);
        assert_eq!(drain(&rx, &mut state, &mut library), 0);
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
