//! The main pane's tracklist: the selected playlist's tracks, or the state of
//! their load.

use egui::Ui;

use crate::library::Load;
use crate::message::Command;
use crate::player::Transport;
use crate::queue::Queue;
use crate::theme::{Tokens, space};

/// The main pane: the selected playlist's tracks, or the state of their load.
pub fn tracks_view(
    ui: &mut Ui,
    library: &mut crate::library::Library,
    transport: &mut Transport,
    queue: &mut Queue,
    tokens: &Tokens,
) -> Option<Command> {
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
