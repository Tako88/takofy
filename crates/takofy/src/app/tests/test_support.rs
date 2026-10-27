use super::super::*;

use eframe::App;
use std::sync::mpsc::channel;
use takofy_model::{CoverSet, CoverUrl, TrackEntry};

pub(super) fn set(url: &CoverUrl) -> CoverSet {
    CoverSet::single(url.clone())
}

pub(super) fn track(uri: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

/// A window with no worker behind it, so what `logic` sends can be read
/// back from the command channel.
pub(super) fn test_window() -> (TakofyApp, tokio::sync::mpsc::UnboundedReceiver<Command>) {
    test_window_with(Loaded::Absent, PrefsSync::off())
}

/// A window restored from `loaded` and saving through `prefs`.
pub(super) fn test_window_with(
    loaded: Loaded,
    prefs: PrefsSync,
) -> (TakofyApp, tokio::sync::mpsc::UnboundedReceiver<Command>) {
    let (sender, updates) = channel();
    // Nothing will ever arrive; a disconnected channel is what the window
    // already tolerates.
    drop(sender);
    let (commands, received) = tokio::sync::mpsc::unbounded_channel();
    let (transport, queue) = TakofyApp::restored_state(&loaded);
    let app = TakofyApp {
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
        ui_end: UiEnd { updates, commands },
        tokens: Tokens::DARK,
        art: Art::new(),
        since: None,
        prefs,
    };
    (app, received)
}

/// Runs one frame of the real `logic` pass, which is where a search read is
/// armed. `logic` is an [`eframe::App`] method, so the frame is the one
/// eframe's own test harness builds.
pub(super) fn logic_frame(app: &mut TakofyApp) {
    app.logic(
        &egui::Context::default(),
        &mut eframe::Frame::_new_kittest(),
    );
}

/// A window's context, set up the way `TakofyApp::new` sets one up.
pub(super) fn window_context() -> egui::Context {
    let tokens = Tokens::DARK;
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&tokens));
    ctx
}

/// One draw pass, as eframe runs it between two `logic` passes.
pub(super) fn ui_frame(app: &mut TakofyApp, ctx: &egui::Context) -> egui::FullOutput {
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

/// The delay the window asked for, which is egui's own first passes' work
/// until the app stops asking for anything.
pub(super) fn root_delay(output: &egui::FullOutput) -> Duration {
    output
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .expect("the root viewport")
        .repaint_delay
}

/// A signed-in window with nothing loading, so the only thing that can ask
/// for a frame is a value on its way to the file.
pub(super) fn idle_window() -> (TakofyApp, tokio::sync::mpsc::UnboundedReceiver<Command>) {
    let (mut app, received) = test_window();
    app.state = SignIn::SignedIn;
    app.library.playlists = Load::Ready(Vec::new());
    (app, received)
}

/// Every command the frames since the last call sent.
pub(super) fn take_commands(
    received: &mut tokio::sync::mpsc::UnboundedReceiver<Command>,
) -> Vec<Command> {
    let mut commands = Vec::new();
    while let Ok(command) = received.try_recv() {
        commands.push(command);
    }
    commands
}
