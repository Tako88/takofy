//! The application: window, threads, and the wiring between them.
//!
//! This crate is the only place the Spotify session and the UI meet. It runs one
//! egui thread, one tokio runtime for the session, and lets `librespot` own its
//! own playback threads; they talk over channels rather than shared locks, so a
//! slow network cannot stall a frame.
//!
//! The shell below is a placeholder that exists to prove the window, the GPU
//! backend and the token layer work on both target platforms. Phase 1 (`#1`)
//! replaces the placeholder text with a real library and a real transport.

use eframe::egui;
use takofy_ui::{layout, theme};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Takofy")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([420.0, 320.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Takofy",
        options,
        Box::new(|cc| {
            let ctx = &cc.egui_ctx;
            ctx.set_theme(egui::ThemePreference::Dark);
            ctx.set_style_of(egui::Theme::Dark, theme::style(&theme::Tokens::DARK));
            Ok(Box::new(App))
        }),
    )
}

struct App;

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let tokens = theme::Tokens::DARK;
        let shell = layout::shell_for(layout::Width::of(ui.max_rect().width()), &tokens);
        let muted = tokens.palette.text_muted;

        if let Some(sidebar) = shell.sidebar {
            egui::Panel::left("sidebar")
                .exact_size(sidebar)
                .show(ui, |ui| {
                    ui.add_space(theme::space::MD);
                    ui.heading("Library");
                    ui.label(egui::RichText::new("Playlists arrive in phase 1.").color(muted));
                });
        }

        egui::Panel::bottom("now_playing")
            .exact_size(shell.now_playing)
            .show(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new("Nothing playing").color(muted));
                });
            });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(theme::space::MD);
            ui.heading("Takofy");
            ui.label(egui::RichText::new("The Spotify session arrives in phase 1.").color(muted));
        });
    }
}
